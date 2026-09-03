//! 转写/翻译后台任务：封装 ffmpeg→whisper→翻译 的调用，负责与数据库交互。
//! 耗时操作在独立线程执行，通过 `tauri::AppHandle` 事件向前端推送进度。

use crate::db::MediaDb;
use anyhow::Result;
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use tauri::{AppHandle, Emitter};

/// 前端所需的字幕行结构（序列化给前端）
#[derive(Debug, Clone, Serialize)]
pub struct SubtitleRow {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub translation: String,
    pub ordinal: i64,
}

/// whisper 对「无语音/纯音乐/音效」段会输出占位文本（如 [BLANK_AUDIO]/[MUSIC]/(music)/♪）。
/// 这类行不是真实对白，写库与 emit 前应丢弃，避免污染字幕列表。
fn is_noise_text(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    if matches!(
        t.to_lowercase().as_str(),
        "[blank_audio]" | "[music]" | "(music)"
    ) {
        return true;
    }
    // 纯音符/空白（如 "♪ ♪"）
    t.chars().all(|c| c == '♪' || c == ' ')
}

/// 转写进度事件负载
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressPayload {
    media_id: i64,
    stage: String,  // "extract" | "transcribe" | "translate" | "done"
    progress: u8,   // 0-100
    message: String,
}

/// 转写块事件负载：mediaId(外层 camelCase) + 该块新增字幕行(行内保持 snake_case，与 get_subtitles 一致)。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SegmentsPayload {
    media_id: i64,
    rows: Vec<SubtitleRow>,
}

const EVENT_PROGRESS: &str = "transcribe://progress";
const EVENT_DONE: &str = "transcribe://done";
const EVENT_ERROR: &str = "transcribe://error";
const EVENT_CANCELED: &str = "transcribe://canceled";
const EVENT_SEGMENTS: &str = "transcribe://segments";

/// 正在运行的转写任务集合（media_id）。
/// 1) 防止同一媒体并发触发多个转写任务；
/// 2) 支持取消：请求取消 = 从集合移除，任务在下一个检查点自行退出。
///    （whisper 推理为单次整体调用不可中断，取消最迟在其结束后生效）
static RUNNING_TRANSCRIPTIONS: LazyLock<Mutex<HashSet<i64>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// 查询某媒体的转写任务是否正在运行
pub fn transcription_running(media_id: i64) -> bool {
    RUNNING_TRANSCRIPTIONS
        .lock()
        .map(|g| g.contains(&media_id))
        .unwrap_or(false)
}

/// 登记新任务；该媒体已在跑则返回 false（拒绝重复触发）。
fn register_transcription(media_id: i64) -> bool {
    let mut guard = match RUNNING_TRANSCRIPTIONS.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    if guard.contains(&media_id) {
        false
    } else {
        guard.insert(media_id);
        true
    }
}

/// 注销/请求取消（两种含义合一：任务结束时也用它注销自己）
fn unregister_transcription(media_id: i64) {
    if let Ok(mut guard) = RUNNING_TRANSCRIPTIONS.lock() {
        guard.remove(&media_id);
    }
}

/// 请求取消某媒体的转写任务（在下一个检查点生效）
pub fn request_cancel_transcription(media_id: i64) {
    unregister_transcription(media_id);
}

/// 翻译 API 配置
pub struct ApiConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
}

/// 解析 API 配置：优先环境变量，其次设置表（DB）
pub fn resolve_api_config(db: &MediaDb) -> ApiConfig {
    let env_base = std::env::var("ASPLAYER_API_BASE").ok().filter(|s| !s.trim().is_empty());
    let env_key = std::env::var("ASPLAYER_API_KEY").ok().filter(|s| !s.trim().is_empty());
    let env_model = std::env::var("ASPLAYER_API_MODEL").ok().filter(|s| !s.trim().is_empty());

    let db_base = db.get_setting("api_base").ok().flatten();
    let db_key = db.get_setting("api_key").ok().flatten();
    let db_model = db.get_setting("api_model").ok().flatten();

    ApiConfig {
        api_base: env_base.or(db_base).unwrap_or_else(|| "https://api.deepseek.com/v1".into()),
        api_key: env_key.or(db_key).unwrap_or_default(),
        model: env_model.or(db_model).unwrap_or_else(|| "deepseek-chat".into()),
    }
}

/// 是否指向本机服务（localhost / 127.x.x.x / [::1]）。
/// 本机服务（如本地 Ollama）通常无需 API Key，翻译时允许空 key 放行；云端服务仍强制要求。
fn is_local_base(base: &str) -> bool {
    let b = base.trim().to_ascii_lowercase();
    if b.is_empty() {
        return false;
    }
    let host_port = b.split("://").nth(1).unwrap_or(&b).split('/').next().unwrap_or("");
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    if host == "localhost" || host == "::1" || host == "0.0.0.0" {
        return true;
    }
    host.parse::<std::net::Ipv4Addr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

fn emit_progress(app: &AppHandle, media_id: i64, stage: &str, progress: u8, message: &str) {
    let _ = app.emit(
        EVENT_PROGRESS,
        ProgressPayload { media_id, stage: stage.into(), progress, message: message.into() },
    );
}

/// 在 Arc<Mutex<MediaDb>> 上执行一次加锁操作（短锁，避免后台任务长期占用）。
/// 闭包返回 rusqlite::Result，这里自动转成 anyhow::Result。
fn with_db<T>(
    db: &Arc<Mutex<MediaDb>>,
    f: impl FnOnce(&MediaDb) -> rusqlite::Result<T>,
) -> Result<T> {
    let guard = db.lock().map_err(|e| anyhow::anyhow!("数据库锁异常: {e}"))?;
    f(&guard).map_err(anyhow::Error::from)
}

/// 解析转写源语言：调用方参数优先，其次 DB 设置 `whisper_lang`，两者皆空 → 空串（whisper 自动检测）。
fn source_lang(db: &Arc<Mutex<MediaDb>>, param: &str) -> String {
    if !param.trim().is_empty() {
        return param.to_string();
    }
    with_db(db, |d| Ok(d.get_setting("whisper_lang").ok().flatten().unwrap_or_default()))
        .unwrap_or_default()
}

const MAX_PROMPT_CHARS: usize = 160;

/// 把 whisper 返回的相对时间戳段偏移到绝对毫秒。
pub(crate) fn offset_absolute(
    segs: &[asplayer_transcribe::srt::Segment],
    w_start_ms: u64,
) -> Vec<asplayer_transcribe::srt::Segment> {
    segs.iter()
        .map(|s| asplayer_transcribe::srt::Segment {
            start_ms: s.start_ms + w_start_ms,
            end_ms: s.end_ms + w_start_ms,
            text: s.text.clone(),
        })
        .collect()
}

/// 构造下一窗口的 `initial_prompt`：取本窗口已解码文本的最近 ~160 字符。
pub(crate) fn build_prompt_tail(window_text: &str) -> Option<String> {
    let s = window_text.trim();
    if s.is_empty() {
        return None;
    }
    let tail: String = s.chars().rev().take(MAX_PROMPT_CHARS).collect::<Vec<_>>().into_iter().rev().collect();
    Some(tail)
}

/// 转写切块参数：固定时长窗口（LLPlayer 式），窗口长度取 `vad_max_chunk_ms`，缺省 20s。
fn chunk_config(db: &MediaDb) -> asplayer_transcribe::vad::VadConfig {
    let window_ms = db
        .get_setting("vad_max_chunk_ms")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(20_000);
    asplayer_transcribe::vad::VadConfig::fixed_windows(window_ms)
}

/// 转写中止原因
enum TranscribeStop {
    Canceled,
    Error(String),
}

/// 任务级失败：置 error 状态后向事件层传播
fn fail(db: &Arc<Mutex<MediaDb>>, media_id: i64, msg: String) -> TranscribeStop {
    let _ = with_db(db, |d| d.set_subtitle_status(media_id, "error", ""));
    TranscribeStop::Error(msg)
}

/// 转写步骤主体（锁外耗时操作 + 数据库短锁）。resume=true 从断点继续，false 清空重转。
fn transcribe_inner(
    app: &AppHandle,
    db: &Arc<Mutex<MediaDb>>,
    media_id: i64,
    lang_opt: Option<&str>,
    resume: bool,
    path: &str,
    tmp: &std::path::Path,
) -> Result<(), TranscribeStop> {
    // 清空/断点策略：fresh 则清字幕与断点；resume 保持已有断点
    with_db(db, |d| {
        if !resume {
            d.clear_subtitles(media_id)?;
            d.set_transcribe_next_ms(media_id, 0)?;
        }
        d.set_subtitle_status(media_id, "transcribing", lang_opt.unwrap_or(""))
    })
    .map_err(|e| fail(db, media_id, format!("数据库异常: {e}")))?;
    emit_progress(app, media_id, "extract", 5, "抽取音轨…");

    // 抽音轨到临时目录
    let _ = std::fs::create_dir_all(tmp);
    let wav = match asplayer_transcribe::audio::extract_wav(&PathBuf::from(path), tmp) {
        Ok(w) => w,
        Err(e) => return Err(fail(db, media_id, format!("抽音轨失败: {e}"))),
    };
    if !transcription_running(media_id) {
        return Err(TranscribeStop::Canceled);
    }

    emit_progress(app, media_id, "transcribe", 15, "Whisper 转写准备…");
    let samples = match asplayer_transcribe::audio::read_samples_f32(&wav) {
        Ok(s) => s,
        Err(e) => return Err(fail(db, media_id, format!("读取音频失败: {e}"))),
    };
    if !transcription_running(media_id) {
        return Err(TranscribeStop::Canceled);
    }

    let (model, cfg) = {
        let g = db.lock().map_err(|e| fail(db, media_id, format!("数据库锁异常: {e}")))?;
        (
            crate::models::resolve_model_path(&g).to_string_lossy().into_owned(),
            chunk_config(&g),
        )
    };

    let chunks = asplayer_transcribe::vad::split_samples(&samples, &cfg);
    if chunks.is_empty() {
        return Err(fail(db, media_id, "音频为空，无法转写".to_string()));
    }
    let total = chunks.len();

    // 断点：跳过 end_ms <= next_ms 的已完成块（每次都在块边界落盘，故无重复）。
    let next_ms = with_db(db, |d| d.get_transcribe_next_ms(media_id)).unwrap_or(0);
    let start_idx = chunks.iter().position(|c| c.end_ms > next_ms).unwrap_or(total);

    let mut whisper = match asplayer_transcribe::whisper::Whisper::load(&model) {
        Ok(w) => w,
        Err(e) => return Err(fail(db, media_id, format!("加载模型失败: {e}"))),
    };
    let mut prompt_tail: Option<String> = None;
    let mut ordinal: i64 = 0;

    for (idx, ch) in chunks.iter().enumerate().skip(start_idx) {
        // 块间检查点：取消最迟在一个块内生效
        if !transcription_running(media_id) {
            return Err(TranscribeStop::Canceled);
        }
        let chunk_samples = &samples[ch.start_sample..ch.end_sample];
        let segs = match whisper.transcribe(lang_opt, prompt_tail.as_deref(), chunk_samples) {
            Ok(s) => s,
            Err(e) => return Err(fail(db, media_id, format!("转写失败: {e}"))),
        };
        // 相对 → 绝对毫秒；丢弃无语音/纯音乐占位
        let segs = offset_absolute(&segs, ch.start_ms as u64);
        let segs: Vec<_> = segs.into_iter().filter(|s| !is_noise_text(&s.text)).collect();
        let chunk_text: String = segs.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");

        // 每个 whisper 段即一行字幕（LLPlayer 式：不按时间间隙合并，避免 run-on 句号堆叠）
        let mut saved: Vec<(asplayer_transcribe::srt::Segment, i64)> = Vec::new();
        for s in segs {
            saved.push((s, ordinal));
            ordinal += 1;
        }
        with_db(db, |d| {
            for (s, ord) in &saved {
                d.save_subtitle(media_id, s.start_ms as i64, s.end_ms as i64, &s.text, "", *ord)?;
            }
            d.set_transcribe_next_ms(media_id, ch.end_ms)?;
            Ok(())
        })
        .map_err(|e| fail(db, media_id, format!("回写字幕失败: {e}")))?;

        // 逐句推送实时事件（时间已为绝对毫秒）
        for (s, ord) in &saved {
            let rows = vec![SubtitleRow {
                start_ms: s.start_ms as i64,
                end_ms: s.end_ms as i64,
                text: s.text.clone(),
                translation: String::new(),
                ordinal: *ord,
            }];
            let _ = app.emit(EVENT_SEGMENTS, SegmentsPayload { media_id, rows });
        }

        prompt_tail = build_prompt_tail(&chunk_text);

        let prog = 15 + 65 * (idx + 1) as u64 / total as u64;
        emit_progress(app, media_id, "transcribe", prog as u8, &format!("转写 {}/{} 段", idx + 1, total));
    }

    // 全部完成：清断点、置 done。
    with_db(db, |d| {
        d.set_transcribe_next_ms(media_id, 0)?;
        d.set_subtitle_status(media_id, "done", lang_opt.unwrap_or(""))
    })
    .map_err(|e| fail(db, media_id, format!("数据库异常: {e}")))?;
    Ok(())
}

/// 转写任务（后台线程调用）：抽音轨 → 固定窗口切块 → 带上下文逐块 whisper → 每段即一行字幕写库 + 每块断点 → done。
/// 取消落地：有断点 → partial（可续跑）；无断点 → none。
pub fn run_transcription(
    app: AppHandle,
    db: Arc<Mutex<MediaDb>>,
    media_id: i64,
    lang: Option<String>,
    resume: bool,
) {
    // 同一媒体同时只允许一个转写任务
    if !register_transcription(media_id) {
        let _ = app.emit(EVENT_ERROR, format!("媒体 #{media_id} 已有转写任务在进行中"));
        return;
    }

    let path = match with_db(&db, |d| d.media_path(media_id)).map(|v| v.0) {
        Ok(p) => p,
        Err(e) => {
            unregister_transcription(media_id);
            let _ = app.emit(EVENT_ERROR, format!("找不到媒体: {e}"));
            return;
        }
    };

    let lang_str = source_lang(&db, &lang.unwrap_or_default());
    let lang_opt = if lang_str.is_empty() { None } else { Some(lang_str.as_str()) };
    let tmp = std::env::temp_dir().join(format!("asplayer-{media_id}"));
    let result = transcribe_inner(&app, &db, media_id, lang_opt, resume, &path, &tmp);

    let _ = std::fs::remove_dir_all(&tmp);
    unregister_transcription(media_id);

    match result {
        Ok(()) => {
            emit_progress(&app, media_id, "done", 100, "转写完成");
            let _ = app.emit(EVENT_DONE, media_id);
        }
        Err(TranscribeStop::Canceled) => {
            // 有断点 → partial（保留已完成块，可续跑）；否则 none
            let next = with_db(&db, |d| d.get_transcribe_next_ms(media_id)).unwrap_or(0);
            let _ = with_db(&db, |d| {
                if next > 0 {
                    d.set_subtitle_status(media_id, "partial", &lang_str)
                } else {
                    d.set_subtitle_status(media_id, "none", "")
                }
            });
            let _ = app.emit(EVENT_CANCELED, media_id);
        }
        Err(TranscribeStop::Error(msg)) => {
            let _ = app.emit(EVENT_ERROR, msg);
        }
    }
}


/// 翻译任务（后台线程调用）：读取未翻译段 → 批量翻译 → 回写。
/// force=true 时先清空该媒体全部译文，再全量重译（用于修正错位的旧译文）。
pub fn run_translation(app: AppHandle, db: Arc<Mutex<MediaDb>>, media_id: i64, force: bool) {
    // 数据库读写短锁；翻译 API 调用在锁外。
    let (status, lang) = match with_db(&db, |d| d.get_subtitle_status(media_id)) {
        Ok(v) => v,
        Err(e) => {
            let _ = app.emit(EVENT_ERROR, format!("读取状态失败: {e}"));
            return;
        }
    };
    if status != "done" {
        let _ = app.emit(EVENT_ERROR, "请先完成转写再翻译".to_string());
        return;
    }
    // 重新翻译：先把旧译文全部清空，使 get_untranslated_subtitles 覆盖所有行。
    if force {
        if let Err(e) = with_db(&db, |d| d.clear_translations(media_id)) {
            let _ = app.emit(EVENT_ERROR, format!("清空旧译文失败: {e}"));
            return;
        }
    }

    let cfg = with_db(&db, |d| Ok(resolve_api_config(d))).unwrap_or_else(|_| {
        ApiConfig { api_base: String::new(), api_key: String::new(), model: "deepseek-chat".into() }
    });
    if cfg.api_key.is_empty() && !is_local_base(&cfg.api_base) {
        let _ = app.emit(EVENT_ERROR, "未配置翻译 API Key（请设置 ASPLAYER_API_KEY 或在设置面板填写）".to_string());
        return;
    }

    let rows = match with_db(&db, |d| d.get_untranslated_subtitles(media_id)) {
        Ok(r) => r,
        Err(e) => {
            let _ = app.emit(EVENT_ERROR, format!("读取待翻译段失败: {e}"));
            return;
        }
    };
    if rows.is_empty() {
        let _ = app.emit(EVENT_DONE, media_id);
        return;
    }

    let _ = with_db(&db, |d| d.set_subtitle_status(media_id, "translating", &lang));
    emit_progress(&app, media_id, "translate", 5, &format!("翻译 {} 段…", rows.len()));

    let segments: Vec<asplayer_transcribe::srt::Segment> = rows
        .iter()
        .map(|r| asplayer_transcribe::srt::Segment {
            start_ms: r.start_ms as u64,
            end_ms: r.end_ms as u64,
            text: r.text.clone(),
        })
        .collect();

    let map = match asplayer_transcribe::translate::translate_segments(
        &segments,
        &cfg.api_base,
        &cfg.api_key,
        &cfg.model,
        "Simplified Chinese",
    ) {
        Ok(m) => m,
        Err(e) => {
            let _ = with_db(&db, |d| d.set_subtitle_status(media_id, "done", &lang));
            let _ = app.emit(EVENT_ERROR, format!("翻译失败: {e}"));
            return;
        }
    };

    emit_progress(&app, media_id, "translate", 90, "回写译文…");
    let _ = with_db(&db, |d| {
        for (idx, row) in rows.iter().enumerate() {
            if let Some(trans) = map.get(&idx) {
                d.update_translation(media_id, row.start_ms, trans)?;
            }
        }
        d.set_subtitle_status(media_id, "done", &lang)?;
        Ok(())
    });

    emit_progress(&app, media_id, "done", 100, "翻译完成");
    let _ = app.emit(EVENT_DONE, media_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MediaDb;
    use std::sync::{Arc, Mutex};

    #[test]
    fn source_lang_param_overrides_setting() {
        let db = MediaDb::open_in_memory().unwrap();
        db.save_setting("whisper_lang", "zh").unwrap();
        let db = Arc::new(Mutex::new(db));
        assert_eq!(source_lang(&db, "en"), "en"); // 参数优先
        assert_eq!(source_lang(&db, ""), "zh"); // 回退设置
        db.lock().unwrap().save_setting("whisper_lang", "ja").unwrap();
        assert_eq!(source_lang(&db, ""), "ja");
    }

    #[test]
    fn local_bases_allow_empty_key() {
        assert!(is_local_base("http://localhost:11434/v1"));
        assert!(is_local_base("localhost:11434"));
        assert!(is_local_base("http://127.0.0.1:11434/v1"));
        assert!(is_local_base("http://[::1]:11434/v1"));
    }

    #[test]
    fn remote_bases_require_key() {
        assert!(!is_local_base("https://api.deepseek.com/v1"));
        assert!(!is_local_base("https://api.openai.com/v1"));
        assert!(!is_local_base("http://192.168.1.5:11434/v1"));
        assert!(!is_local_base(""));
    }

    #[test]
    fn is_noise_text_flags_placeholders() {
        assert!(is_noise_text("[BLANK_AUDIO]"));
        assert!(is_noise_text("  [blank_audio]  "));
        assert!(is_noise_text("[MUSIC]"));
        assert!(is_noise_text("(music)"));
        assert!(is_noise_text("♪"));
        assert!(is_noise_text("♪ ♪"));
        assert!(is_noise_text("   "));

        // 真实对白/音效标注应保留
        assert!(!is_noise_text("I knew you were still awake."));
        assert!(!is_noise_text("(sighs)"));
        assert!(!is_noise_text("♪-adjacent words are content"));
    }

    // ---- 流式整句 helper 单测 ----
    use asplayer_transcribe::srt::Segment as Seg;

    fn seg(start: u64, end: u64, text: &str) -> Seg {
        Seg { start_ms: start, end_ms: end, text: text.into() }
    }

    #[test]
    fn offset_absolute_adds_window_start() {
        let segs = vec![seg(0, 500, "a"), seg(600, 1200, "b")];
        let out = offset_absolute(&segs, 3000);
        assert_eq!(out[0].start_ms, 3000);
        assert_eq!(out[0].end_ms, 3500);
        assert_eq!(out[1].start_ms, 3600);
        assert_eq!(out[1].end_ms, 4200);
    }

    #[test]
    fn build_prompt_tail_takes_tail_chars() {
        // 200 字节内容：头尾不同，确保取的是「尾部」而非「头部」。
        let long = format!("{}::::{}", "a".repeat(100), "b".repeat(100));
        let t = build_prompt_tail(&long).unwrap();
        assert_eq!(t.chars().count(), 160);
        // 尾部应保留末尾的 100 个 b；若错误地取头部则只会留下 56 个 b
        assert!(t.ends_with(&"b".repeat(100)), "tail should keep trailing b's");
        // 空输入 → None
        assert_eq!(build_prompt_tail("   "), None);
        assert_eq!(build_prompt_tail(""), None);
    }

    #[test]
    fn source_lang_defaults_to_empty() {
        let db = MediaDb::open_in_memory().unwrap();
        let db = Arc::new(Mutex::new(db));
        assert_eq!(source_lang(&db, ""), "");
    }
}

