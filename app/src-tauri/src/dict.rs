//! 内置词典下载基础设施：ECDICT(英，CSV) + JMdict(日，gzip XML)。
//! 数据按需下载，原始文件落 ~/.asplayer/dict/；建库(build_dictionary_db)在后续任务，命令接线也在后续任务。
//! 这里只提供下载、取消、状态与事件广播，复用模型下载器(models.rs)的 statics+事件 范式。

use crate::{AppState, CmdResult};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};

pub const EN_SOURCE_URL: &str =
    "https://raw.githubusercontent.com/skywind3000/ECDICT/master/ecdict.csv";
pub const JA_SOURCE_URL: &str = "http://ftp.edrdg.org/pub/Nihongo/JMdict_e.gz";

// 中国网络下 raw.githubusercontent.com 常被阻断，内置可用的 GitHub 代理作为镜像，按 官方→镜像 依次回落。
// 普通用户无需自己知道镜像地址；设置页的镜像源只是高级覆盖项。
const EN_MIRRORS: &[&str] = &[
    // 经本机实测（中国网络）仅 gh-proxy.com 能直接返回文件；其余常见代理要么 000 要么 JS 挑战页/404。
    // 保持单一可靠镜像 + 清晰错误提示（提示可到设置页配置镜像源），可覆盖绝大多数普通用户。
    "https://gh-proxy.com/https://raw.githubusercontent.com/skywind3000/ECDICT/master/ecdict.csv",
];
const JA_MIRRORS: &[&str] = &[
    // https 版（官方同源），比 http 官方源更安全
    "https://www.edrdg.org/pub/Nihongo/JMdict_e.gz",
];

pub const EVENT_STATUS: &str = "dict://status";
pub const EVENT_PROGRESS: &str = "dict://progress";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DictLang {
    En,
    Ja,
}

impl DictLang {
    pub fn as_str(self) -> &'static str {
        match self {
            DictLang::En => "en",
            DictLang::Ja => "ja",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "en" => Some(DictLang::En),
            "ja" => Some(DictLang::Ja),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DlStatus {
    Idle,
    Downloading,
    Done,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize)]
pub struct Download {
    pub lang: String,
    pub status: DlStatus,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DictStatus {
    pub lang: String,
    pub raw_exists: bool,
    pub raw_bytes: u64,
    pub db_exists: bool,
    pub db_bytes: u64,
    pub status: DlStatus,
    pub error: Option<String>,
}

/// 查词结果 DTO（前端字典卡片用）。asplayer-dict 的 LookupResult 未 Serialize，此结构体做序列化映射。
#[derive(Debug, Clone, Serialize)]
pub struct DictLookup {
    pub term: String,
    pub lang: String,
    pub phonetic: Option<String>,  // en: 音标
    pub reading: Option<String>,    // ja: 假名读音
    pub pos: Option<String>,        // 词性
    pub definitions: Vec<String>,   // en: 中文释义；ja: 英文 gloss
    pub suggestions: Vec<String>,   // 未命中时的相似词
}

#[derive(Debug, Clone, Serialize)]
struct Progress {
    lang: String,
    bytes_downloaded: u64,
    total_bytes: u64,
    percent: u8,
}

static DOWNLOADS: LazyLock<Mutex<HashMap<String, Download>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ACTIVE: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));
static CANCEL: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 词典根目录：~/.asplayer/dict（与模型 ~/.asplayer/models 一致）
pub fn dict_dir() -> PathBuf {
    home().join(".asplayer").join("dict")
}

/// 原始数据文件路径（en: ecdict.csv；ja: JMdict_e.gz，建库时解压）
pub fn raw_path(lang: DictLang) -> PathBuf {
    match lang {
        DictLang::En => dict_dir().join("ecdict.csv"),
        DictLang::Ja => dict_dir().join("JMdict_e.gz"),
    }
}

fn with_dl<R>(lang: &str, f: impl FnOnce(&mut Download) -> R) -> Option<R> {
    let mut g = DOWNLOADS.lock().ok()?;
    Some(f(g.entry(lang.to_string()).or_insert_with(|| Download {
        lang: lang.to_string(),
        status: DlStatus::Idle,
        bytes_downloaded: 0,
        total_bytes: 0,
        error: None,
    })))
}

fn get_dl(lang: &str) -> Option<Download> {
    DOWNLOADS.lock().ok().and_then(|g| g.get(lang).cloned())
}

fn is_canceled(lang: &str) -> bool {
    CANCEL.lock().map(|g| g.contains(lang)).unwrap_or(false)
}

fn request_cancel(lang: &str) {
    if let Ok(mut g) = CANCEL.lock() {
        g.insert(lang.to_string());
    }
}

fn clear_cancel(lang: &str) {
    if let Ok(mut g) = CANCEL.lock() {
        g.remove(lang);
    }
}

fn release_active(lang: &str) {
    if let Ok(mut g) = ACTIVE.lock() {
        if g.as_deref() == Some(lang) {
            *g = None;
        }
    }
}

fn emit_progress(app: &AppHandle, lang: &str, bytes: u64, total: u64) {
    let percent = if total > 0 {
        ((bytes as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as u8
    } else {
        0
    };
    let _ = app.emit(
        EVENT_PROGRESS,
        Progress {
            lang: lang.to_string(),
            bytes_downloaded: bytes,
            total_bytes: total,
            percent,
        },
    );
}

/// 解压 gzip（JMdict_e.gz → XML 文本）
pub fn gunzip_bytes(gz: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(gz);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

/// 构建词典 SQLite 库（低层，接受任意连接，便于用内存库测试）。
/// 表结构严格按实施计划：en_entries / en_fts(外部内容) / ja_entries。
///
/// 注意：asplayer-dict 的 EnEntry 无 freq 字段，故 freq 列一律写 0（占位）。
/// FTS 为外部内容表，基表插入不自动填充，插入完成后统一 rebuild。
pub fn build_db(
    conn: &Connection,
    en_rows: &[asplayer_dict::types::EnEntry],
    ja_rows: &[asplayer_dict::types::JaEntry],
) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS en_entries (
           word TEXT PRIMARY KEY, phonetic TEXT, definition TEXT, translation TEXT,
           pos TEXT, exchange TEXT, freq INTEGER
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS en_fts USING fts5(
           word, definition, content='en_entries', content_rowid='rowid'
         );
         CREATE TABLE IF NOT EXISTS ja_entries (
           surface TEXT, reading TEXT, pos TEXT, gloss TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_ja_reading ON ja_entries(reading);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_ja_surface_reading ON ja_entries(surface, reading);",
    )?;

    let tx = conn.unchecked_transaction()?;
    {
        let mut en_stmt = tx.prepare(
            "INSERT OR REPLACE INTO en_entries
               (word, phonetic, definition, translation, pos, exchange, freq)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
        )?;
        for e in en_rows {
            en_stmt.execute(rusqlite::params![
                e.word, e.phonetic, e.definition, e.translation, e.pos, e.exchange
            ])?;
        }
    }
    {
        let mut ja_stmt = tx.prepare(
            "INSERT OR REPLACE INTO ja_entries (surface, reading, pos, gloss) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for j in ja_rows {
            ja_stmt.execute(rusqlite::params![j.surface, j.reading, j.pos, j.gloss])?;
        }
    }
    tx.commit()?;

    // 外部内容 FTS 不随基表插入自动填充，统一在此重建索引
    conn.execute("INSERT INTO en_fts(en_fts) VALUES('rebuild')", [])?;
    Ok(())
}

/// 从原始文件构建词典数据库（dict_dir()/dictionary.db）。
/// 读 ecdict.csv 解析英文（表头由实现按内容自动判定），读 JMdict_e.gz 解压后解析日文。
/// 容忍缺失某一语言：仅对存在的原始文件建对应表；若两者都不存在则报错。
/// 如此字典可增量演进 —— 先下载英文建英文索引，之后再下载日文重新建库即可补充。
pub fn build_dictionary_db() -> anyhow::Result<()> {
    let db_path = dict_dir().join("dictionary.db");
    let en_path = raw_path(DictLang::En);
    let ja_path = raw_path(DictLang::Ja);
    if !en_path.is_file() && !ja_path.is_file() {
        anyhow::bail!("无词典原始文件，无法建库");
    }
    let en_rows = if en_path.is_file() {
        let en_csv = fs::read_to_string(&en_path)?;
        asplayer_dict::ingest::parse_en_csv("", &en_csv)?
    } else {
        Vec::new()
    };
    let ja_rows = if ja_path.is_file() {
        let ja_gz = fs::read(&ja_path)?;
        let xml = String::from_utf8(gunzip_bytes(&ja_gz)?)?;
        asplayer_dict::ingest::parse_jm_dict(&xml)?
    } else {
        Vec::new()
    };
    let conn = Connection::open(&db_path)?;
    build_db(&conn, &en_rows, &ja_rows)?;
    Ok(())
}

fn stream_download(
    client: &reqwest::blocking::Client,
    url: &str,
    lang: &str,
    part: &PathBuf,
    app: &AppHandle,
) -> Result<u64, String> {
    let mut resp = client
        .get(url)
        .send()
        .map_err(|e| format!("请求失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    // 镜像站若返回 JS 挑战页（text/html），按失败处理去回落下一个源，而不是把页面当文件写盘
    if let Some(ct) = resp.headers().get(reqwest::header::CONTENT_TYPE) {
        if let Ok(ct) = ct.to_str() {
            if ct.to_ascii_lowercase().starts_with("text/html") {
                return Err("镜像返回网页而非文件".into());
            }
        }
    }
    let total = resp.content_length().unwrap_or(0);
    with_dl(lang, |d| d.total_bytes = total);
    let mut file = std::fs::File::create(part).map_err(|e| format!("写入失败: {e}"))?;
    let mut buf = [0u8; 64 * 1024];
    let mut cur = 0u64;
    let mut last = Instant::now();
    loop {
        if is_canceled(lang) {
            return Err("canceled".into());
        }
        let n = resp
            .read(&mut buf)
            .map_err(|e| format!("读取失败: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("写入失败: {e}"))?;
        cur += n as u64;
        with_dl(lang, |d| d.bytes_downloaded = cur);
        if last.elapsed().as_millis() >= 250 {
            emit_progress(app, lang, cur, total);
            last = Instant::now();
        }
    }
    if cur == 0 {
        return Err("下载内容为空".into());
    }
    Ok(cur)
}

fn e2<E: std::fmt::Display>(e: E) -> String {
    format!("{e}")
}

/// 由 lang 构造当前 DictStatus（raw/db 文件状态 + 下载状态），供 dict_status 与事件广播共用。
fn make_dict_status(lang: &str) -> DictStatus {
    let (raw_exists, raw_bytes) = match DictLang::parse(lang) {
        Some(l) => match std::fs::metadata(raw_path(l)) {
            Ok(m) => (true, m.len()),
            Err(_) => (false, 0),
        },
        None => (false, 0),
    };
    let db = dict_dir().join("dictionary.db");
    let (db_exists, db_bytes) = match std::fs::metadata(&db) {
        Ok(m) => (true, m.len()),
        Err(_) => (false, 0),
    };
    let dl = get_dl(lang);
    DictStatus {
        lang: lang.to_string(),
        raw_exists,
        raw_bytes,
        db_exists,
        db_bytes,
        status: dl.as_ref().map(|d| d.status.clone()).unwrap_or(DlStatus::Idle),
        error: dl.as_ref().and_then(|d| d.error.clone()),
    }
}

fn emit_dict_status(app: &AppHandle, lang: &str) {
    let st = make_dict_status(lang);
    let _ = app.emit(EVENT_STATUS, st);
}

/// 查询内置词典状态：en/ja 各一条，含原始文件与库文件是否就绪及下载状态。
#[tauri::command]
pub fn dict_status(_state: State<AppState>) -> CmdResult<Vec<DictStatus>> {
    let langs = [DictLang::En, DictLang::Ja];
    Ok(langs.iter().map(|l| make_dict_status(l.as_str())).collect())
}

/// 触发后台下载词典原始文件（en: ecdict.csv；ja: JMdict_e.gz），随后建库。
#[tauri::command]
pub fn dict_download(lang: String, app: AppHandle) -> CmdResult<()> {
    if DictLang::parse(&lang).is_none() {
        return Err(format!("未知语言 {lang}"));
    }
    {
        let mut g = ACTIVE.lock().map_err(e2)?;
        if g.is_some() {
            return Err("已有词典下载在进行中".into());
        }
        *g = Some(lang.clone());
    }
    clear_cancel(&lang);
    with_dl(&lang, |d| {
        d.status = DlStatus::Downloading;
        d.error = None;
    });
    // 立即广播，否则 UI 要等下载线程首条事件（首个源连接失败/超时前）才看到「下载中」，表现为点击无反应
    emit_dict_status(&app, &lang);
    std::thread::spawn(move || download_dict_inner(app, lang));
    Ok(())
}

/// 请求取消某语言的词典下载。返回 true 表示取消请求已受理；不在下载中返回 false。
#[tauri::command]
pub fn cancel_dict_download(lang: String) -> CmdResult<bool> {
    if DictLang::parse(&lang).is_none() {
        return Err(format!("未知语言 {lang}"));
    }
    let st = get_dl(&lang).map(|d| d.status).unwrap_or(DlStatus::Idle);
    if st != DlStatus::Downloading {
        return Ok(false);
    }
    request_cancel(&lang);
    Ok(true)
}

/// 后台下载主流程：原始文件已存在则幂等 Done；否则流式下载到 .part 再改名，成功后建库。
fn download_dict_inner(app: AppHandle, lang: String) {
    let Some(lang_enum) = DictLang::parse(&lang) else {
        release_active(&lang);
        return;
    };
    let target = raw_path(lang_enum);
    let part = target.with_extension("part");
    let _ = std::fs::create_dir_all(dict_dir());

    // 已完整下载 → 幂等 Done（同时补建库，容忍重复调用）
    if target.is_file() && std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0) > 0 {
        let bytes = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
        with_dl(&lang, |d| {
            d.status = DlStatus::Done;
            d.bytes_downloaded = bytes;
            d.total_bytes = bytes;
            d.error = None;
        });
        if let Err(e) = build_dictionary_db() {
            with_dl(&lang, |d| d.error = Some(format!("原始文件存在但建库失败: {e}")));
        }
        emit_dict_status(&app, &lang);
        release_active(&lang);
        return;
    }

    // 候选源顺序：用户配置的镜像（settings: dict_url_en / dict_url_ja）→ 官方默认源 → 内置镜像。
    // 内置镜像保证中国网络下无感可用，普通用户无需知道镜像地址。
    let (default_url, mirrors): (&str, &[&str]) = match lang_enum {
        DictLang::En => (EN_SOURCE_URL, EN_MIRRORS),
        DictLang::Ja => (JA_SOURCE_URL, JA_MIRRORS),
    };
    let mut urls: Vec<String> = Vec::new();
    {
        let key = match lang_enum {
            DictLang::En => "dict_url_en",
            DictLang::Ja => "dict_url_ja",
        };
        let state = app.state::<AppState>();
        // 拿到自有数据的 Vec，避免 MutexGuard 存活跨过 state 生命周期
        let settings = state
            .db
            .lock()
            .map(|db| db.all_settings().unwrap_or_default())
            .unwrap_or_default();
        if let Some((_, u)) = settings.iter().find(|(k, _)| k == key) {
            let u = u.trim();
            if !u.is_empty() {
                urls.push(u.to_string());
            }
        }
    }
    // 官方源 + 内置镜像，去重后追加
    if !urls.iter().any(|x| x.as_str() == default_url) {
        urls.push(default_url.to_string());
    }
    for m in mirrors {
        if !urls.iter().any(|x| x.as_str() == *m) {
            urls.push((*m).to_string());
        }
    }

    // connect_timeout：被阻断的源（如 raw.githubusercontent.com）可快速失败并回落镜像，而不是阻塞整个下载
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new());
    let mut last_err = String::new();
    let mut canceled = false;
    let mut ok_bytes: Option<u64> = None;
    for u in &urls {
        match stream_download(&client, u, &lang, &part, &app) {
            Ok(bytes) => {
                ok_bytes = Some(bytes);
                break;
            }
            Err(e) if e == "canceled" => {
                canceled = true;
                break;
            }
            Err(e) => last_err = format!("{u}: {e}"),
        }
    }
    match ok_bytes {
        Some(bytes) => {
            let _ = std::fs::remove_file(&target);
            if std::fs::rename(&part, &target).is_err() {
                with_dl(&lang, |d| {
                    d.status = DlStatus::Failed;
                    d.error = Some("重命名原始文件失败".into());
                });
                emit_dict_status(&app, &lang);
                release_active(&lang);
                return;
            }
            with_dl(&lang, |d| {
                d.status = DlStatus::Done;
                d.bytes_downloaded = bytes;
                d.total_bytes = bytes;
                d.error = None;
            });
            if let Err(e) = build_dictionary_db() {
                with_dl(&lang, |d| d.error = Some(format!("下载成功但建库失败: {e}")));
            }
        }
        None => {
            if canceled {
                let _ = std::fs::remove_file(&part);
                clear_cancel(&lang);
                with_dl(&lang, |d| {
                    d.status = DlStatus::Canceled;
                    d.error = None;
                });
            } else {
                with_dl(&lang, |d| {
                    d.status = DlStatus::Failed;
                    d.error = Some(format!(
                        "下载失败：{last_err}\n已尝试官方源与内置镜像，请检查网络后重试，或到设置页配置可用的镜像源。"
                    ));
                });
            }
        }
    }
    emit_dict_status(&app, &lang);
    release_active(&lang);
}

/// 精确查英文词条（word 唯一命中，取首行）。
fn look_up_en(conn: &Connection, cand: &str) -> Option<DictLookup> {
    conn.query_row(
        "SELECT word, phonetic, definition, translation, pos FROM en_entries WHERE word = ?1",
        rusqlite::params![cand],
        |r| {
            let word: String = r.get(0)?;
            let phonetic: Option<String> = r.get(1)?;
            let definition: String = r.get(2)?;
            let translation: String = r.get(3)?;
            let pos: Option<String> = r.get(4)?;
            let mut definitions = Vec::new();
            if !definition.is_empty() {
                definitions.push(definition);
            }
            if !translation.is_empty() {
                definitions.push(translation);
            }
            if definitions.is_empty() {
                definitions.push(String::new());
            }
            Ok(DictLookup {
                term: word,
                lang: "en".to_string(),
                phonetic: phonetic.filter(|s| !s.is_empty()),
                reading: None,
                pos: pos.filter(|s| !s.is_empty()),
                definitions,
                suggestions: vec![],
            })
        },
    )
    .ok()
}

/// 精确查日文词条（surface 唯一命中，取首行）。
fn look_up_ja(conn: &Connection, cand: &str) -> Option<DictLookup> {
    conn.query_row(
        "SELECT surface, reading, pos, gloss FROM ja_entries WHERE surface = ?1",
        rusqlite::params![cand],
        |r| {
            let surface: String = r.get(0)?;
            let reading: Option<String> = r.get(1)?;
            let pos: Option<String> = r.get(2)?;
            let gloss: String = r.get(3)?;
            Ok(DictLookup {
                term: surface,
                lang: "ja".to_string(),
                phonetic: None,
                reading: reading.filter(|s| !s.is_empty()),
                pos: pos.filter(|s| !s.is_empty()),
                definitions: if gloss.is_empty() { vec![] } else { vec![gloss] },
                suggestions: vec![],
            })
        },
    )
    .ok()
}

/// 清洗 FTS5 MATCH 查询串：仅保留字母数字与单引号，剥掉 FTS 运算符/语法字符。
/// 保证用户输入（如 `"bad OR good"`、`a NOT b`、`a*`、`a(`）不会改变 FTS 查询语义，只留字面词。
fn sanitize_fts_token(s: &str) -> Option<String> {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '\'' {
            out.push(c);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 未命中时的相似词提示：en 走 FTS 前缀，ja 走 LIKE 前缀（CJK 的 FTS 前缀不可靠）。
/// 结果统一去重并封顶 6 条（整段结果，而非逐候选）。
fn suggest(conn: &Connection, term: &str, lang: &'static str) -> Vec<String> {
    let mut out = Vec::new();
    if lang == "ja" {
        // LIKE 通配符转义（ESCAPE '\'）：用户输入里的 %/_/反斜杠不会充当通配符
        let escaped = term
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let Ok(mut stmt) = conn.prepare(
            "SELECT surface FROM ja_entries WHERE surface LIKE ?1 || '%' ESCAPE '\\' LIMIT 6",
        ) else {
            return out;
        };
        let rows = stmt.query_map(rusqlite::params![escaped], |r| r.get::<_, String>(0));
        if let Ok(rows) = rows {
            for r in rows.flatten() {
                if !out.contains(&r) {
                    out.push(r);
                }
                if out.len() >= 6 {
                    break;
                }
            }
        }
    } else {
        let Ok(mut stmt) =
            conn.prepare("SELECT word FROM en_fts WHERE en_fts MATCH ?1 LIMIT 6")
        else {
            return out;
        };
        'outer: for cand in asplayer_dict::query::lemma_candidates(term, lang) {
            let Some(sanitized) = sanitize_fts_token(&cand) else {
                continue;
            };
            let q = format!("{sanitized}*");
            let rows = stmt.query_map(rusqlite::params![q.as_str()], |r| {
                r.get::<_, String>(0)
            });
            if let Ok(rows) = rows {
                for r in rows.flatten() {
                    if !out.contains(&r) {
                        out.push(r);
                    }
                    if out.len() >= 6 {
                        break 'outer;
                    }
                }
            }
        }
    }
    out
}

/// 在给定 SQLite 连接上执行查词（纯函数，便于内存库测试）。
/// 命中返回词条；完全未命中返回带 suggestions 的占位词条。
fn lookup_in_db(conn: &Connection, term: &str) -> Vec<DictLookup> {
    let lang = asplayer_dict::query::detect_lang(term);
    let candidates = asplayer_dict::query::lemma_candidates(term, lang);
    let mut seen = HashSet::new();
    for cand in candidates {
        if !seen.insert(cand.clone()) {
            continue;
        }
        let hit = if lang == "ja" {
            look_up_ja(conn, &cand)
        } else {
            look_up_en(conn, &cand)
        };
        if let Some(hit) = hit {
            return vec![hit];
        }
    }
    vec![DictLookup {
        term: term.to_string(),
        lang: lang.to_string(),
        phonetic: None,
        reading: None,
        pos: None,
        definitions: vec![],
        suggestions: suggest(conn, term, lang),
    }]
}

/// 查询内置词典。词典未就绪（无 dictionary.db）返回空列表，由前端提示先下载。
#[tauri::command]
pub fn dict_lookup(term: String, _state: State<AppState>) -> CmdResult<Vec<DictLookup>> {
    let p = dict_dir().join("dictionary.db");
    if !p.is_file() {
        return Ok(vec![]);
    }
    let conn = Connection::open_with_flags(&p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(e2)?;
    Ok(lookup_in_db(&conn, &term))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gunzip_roundtrip() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        let data = br#"<?xml version="1.0"?><JMdict></JMdict>"#;
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(data).unwrap();
        let gz = enc.finish().unwrap();
        let out = gunzip_bytes(&gz).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn dict_lang_parse() {
        assert_eq!(DictLang::parse("en"), Some(DictLang::En));
        assert_eq!(DictLang::parse("ja"), Some(DictLang::Ja));
        assert_eq!(DictLang::parse("zh"), None);
    }

    #[test]
    fn raw_path_locations() {
        assert!(raw_path(DictLang::En).ends_with("ecdict.csv"));
        assert!(raw_path(DictLang::Ja).ends_with("JMdict_e.gz"));
    }

    #[test]
    fn build_db_inserts_en_and_ja_and_fts() -> rusqlite::Result<()> {
        let en_header = "word,phonetic,definition,translation,pos,collins,oxford,tag,bnc,frq,exchange,detail,audio\n";
        let en_data = "run,rʌn,跑；奔跑,赛跑,vi,2,1,3,500,1000,p:ran/i:running/3:runs,,\nwalk,wɔːk,走；步行,走路,vi,2,1,3,500,1000,p:walked/i:walking/3:walks,,\n";
        let en_rows = asplayer_dict::ingest::parse_en_csv(en_header, en_data).unwrap();

        let ja_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<JMdict>
  <entry>
    <k_ele><keb>食べる</keb></k_ele>
    <r_ele><reb>たべる</reb></r_ele>
    <sense>
      <pos>v1</pos>
      <gloss>to eat</gloss>
      <gloss>to have a meal</gloss>
    </sense>
  </entry>
</JMdict>"#;
        let ja_rows = asplayer_dict::ingest::parse_jm_dict(ja_xml).unwrap();

        let conn = Connection::open_in_memory()?;
        build_db(&conn, &en_rows, &ja_rows)?;

        // 英文：word 命中且 freq 占位为 0
        let (word, freq): (String, i64) = conn.query_row(
            "SELECT word, freq FROM en_entries WHERE word='run'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(word, "run");
        assert_eq!(freq, 0);

        // 日文：样例命中时长不为零
        let ja_count: i64 = conn.query_row("SELECT COUNT(*) FROM ja_entries", [], |r| r.get(0))?;
        assert!(ja_count > 0);

        // FTS 可查询（外部内容表 rebuild 后按词命中）
        let fts_rowid: i64 = conn.query_row(
            "SELECT rowid FROM en_fts WHERE en_fts MATCH 'run'",
            [],
            |r| r.get(0),
        )?;
        assert!(fts_rowid > 0);

        // 幂等：重复建库不报错，且日文行数不变（不翻倍）
        build_db(&conn, &en_rows, &ja_rows)?;
        let ja_count2: i64 = conn.query_row("SELECT COUNT(*) FROM ja_entries", [], |r| r.get(0))?;
        assert_eq!(ja_count2, ja_count);
        Ok(())
    }

    #[test]
    fn ja_entries_or_replace_dedup_on_surface_reading() -> rusqlite::Result<()> {
        // 两个 <entry> 共享 (surface, reading)：解析器保留两行，去重由建库层负责
        let ja_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<JMdict>
  <entry>
    <k_ele><keb>読む</keb></k_ele>
    <r_ele><reb>よむ</reb></r_ele>
    <sense><gloss>to read</gloss></sense>
  </entry>
  <entry>
    <k_ele><keb>読む</keb></k_ele>
    <r_ele><reb>よむ</reb></r_ele>
    <sense><gloss>to read (alt)</gloss></sense>
  </entry>
</JMdict>"#;
        let ja_rows = asplayer_dict::ingest::parse_jm_dict(ja_xml).unwrap();
        assert_eq!(ja_rows.len(), 2); // 建库前的原始行数

        let conn = Connection::open_in_memory()?;
        build_db(&conn, &[], &ja_rows)?;

        let count: i64 = conn.query_row("SELECT COUNT(*) FROM ja_entries", [], |r| r.get(0))?;
        assert_eq!(count, 1); // OR REPLACE 只留一行

        // 后写胜出
        let gloss: String = conn.query_row(
            "SELECT gloss FROM ja_entries WHERE surface='読む' AND reading='よむ'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(gloss, "to read (alt)");
        Ok(())
    }

    #[test]
    fn lookup_in_db_queries_en_and_ja() -> rusqlite::Result<()> {
        let en_header = "word,phonetic,definition,translation,pos,collins,oxford,tag,bnc,frq,exchange,detail,audio\n";
        let en_data = "run,rʌn,跑；奔跑,赛跑,vi,2,1,3,500,1000,p:ran/i:running/3:runs,,\nwalk,wɔːk,走；步行,走路,vi,2,1,3,500,1000,p:walked/i:walking/3:walks,,\n";
        let en_rows = asplayer_dict::ingest::parse_en_csv(en_header, en_data).unwrap();

        let ja_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<JMdict>
  <entry>
    <k_ele><keb>食べる</keb></k_ele>
    <r_ele><reb>たべる</reb></r_ele>
    <sense><pos>v1</pos><gloss>to eat</gloss></sense>
  </entry>
</JMdict>"#;
        let ja_rows = asplayer_dict::ingest::parse_jm_dict(ja_xml).unwrap();

        let conn = Connection::open_in_memory()?;
        build_db(&conn, &en_rows, &ja_rows)?;

        // 英文直接命中
        let en = lookup_in_db(&conn, "run");
        assert_eq!(en.len(), 1);
        assert_eq!(en[0].lang, "en");
        assert_eq!(en[0].term, "run");
        assert_eq!(en[0].phonetic.as_deref(), Some("rʌn"));
        assert!(en[0].definitions.iter().any(|d| d.contains("跑")));

        // 英文变形词：running → 词形还原命中 run
        let running = lookup_in_db(&conn, "running");
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].term, "run");

        // 日文命中
        let ja = lookup_in_db(&conn, "食べる");
        assert_eq!(ja.len(), 1);
        assert_eq!(ja[0].lang, "ja");
        assert_eq!(ja[0].reading.as_deref(), Some("たべる"));
        assert!(ja[0].definitions.iter().any(|d| d.contains("to eat")));

        // 完全未命中：返回占位词条（含 suggestions 字段）而不崩溃
        let miss = lookup_in_db(&conn, "zzzznotaword");
        assert_eq!(miss.len(), 1);
        assert_eq!(miss[0].term, "zzzznotaword");
        Ok(())
    }
}
