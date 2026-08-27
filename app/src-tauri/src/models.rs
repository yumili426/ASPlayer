//! 模型下载器：whisper.cpp 五档 GGUF 模型的下载/校验/取消/选择。
//! 同步流式下载在后台线程跑，事件经 `app.emit` 广播，DB 设置 `whisper_model` 存选中档。

use crate::AppState;
use crate::db::MediaDb;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};

pub const MODEL_SIZES: [&str; 5] = ["tiny", "base", "small", "medium", "large-v3"];
const SETTING_MODEL: &str = "whisper_model";
const MAGIC: &[u8] = b"ggml";
const URL_OFFICIAL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin";
const URL_MIRROR: &str = "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin";

const EVENT_PROGRESS: &str = "model://progress";
const EVENT_DONE: &str = "model://done";
const EVENT_ERROR: &str = "model://error";
const EVENT_CANCELED: &str = "model://canceled";
const EVENT_SELECTED: &str = "model://selected";

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
    pub size: String,
    pub status: DlStatus,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub size: String,
    pub file_exists: bool,
    pub file_bytes: u64,
    pub selected: bool,
    pub status: DlStatus,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ModelProgress {
    size: String,
    bytes_downloaded: u64,
    total_bytes: u64,
    percent: u8,
}

#[derive(Debug, Clone, Serialize)]
struct ModelError {
    size: String,
    error: String,
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

/// 模型根目录：~/.asplayer/models
pub fn models_dir() -> PathBuf {
    home().join(".asplayer").join("models")
}

/// ggml-{size}.bin 的完整路径
pub fn model_file_path(dir: &Path, size: &str) -> PathBuf {
    dir.join(format!("ggml-{size}.bin"))
}

/// 从 DB 读选中档；未设置回退 "small"
fn selected_size(db: &MediaDb) -> String {
    db.get_setting(SETTING_MODEL)
        .ok()
        .flatten()
        .unwrap_or_else(|| "small".into())
}

/// 解析模型路径：环境变量 ASPLAYER_MODEL > DB 设置 whisper_model > 默认小模型
pub fn resolve_model_path(db: &MediaDb) -> PathBuf {
    if let Some(m) = std::env::var_os("ASPLAYER_MODEL") {
        if !m.is_empty() {
            return PathBuf::from(m);
        }
    }
    model_file_path(&models_dir(), &selected_size(db))
}

/// 校验 GGUF：非空 + 头 4 字节魔数 == b"ggml"
fn verify_file(path: &Path) -> bool {
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if len == 0 {
        return false;
    }
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 4];
    f.read_exact(&mut head).is_ok() && &head == MAGIC
}

/// 对某档状态做一次改造（不存在则插入默认 Idle 档）
fn with_dl<R>(size: &str, f: impl FnOnce(&mut Download) -> R) -> Option<R> {
    let mut g = DOWNLOADS.lock().ok()?;
    Some(f(g.entry(size.to_string()).or_insert_with(|| Download {
        size: size.to_string(),
        status: DlStatus::Idle,
        bytes_downloaded: 0,
        total_bytes: 0,
        error: None,
    })))
}

/// 读取某档（深拷贝），无记录则 None
fn get_dl(size: &str) -> Option<Download> {
    DOWNLOADS.lock().ok().and_then(|g| g.get(size).cloned())
}

/// 是否收到取消请求
fn is_canceled(size: &str) -> bool {
    CANCEL.lock().map(|g| g.contains(size)).unwrap_or(false)
}

fn request_cancel(size: &str) {
    if let Ok(mut g) = CANCEL.lock() {
        g.insert(size.to_string());
    }
}

fn clear_cancel(size: &str) {
    if let Ok(mut g) = CANCEL.lock() {
        g.remove(size);
    }
}

fn release_active(size: &str) {
    if let Ok(mut g) = ACTIVE.lock() {
        if g.as_deref() == Some(size) {
            *g = None;
        }
    }
}

enum StreamErr {
    Canceled,
    Network(String),
}

/// 流式下载单条源。`cur` 为断点字节；成功返回内容总长。
fn stream_download(
    client: &reqwest::blocking::Client,
    url: &str,
    size: &str,
    part: &Path,
    cur: &mut u64,
    app: &AppHandle,
) -> Result<u64, StreamErr> {
    let mut req = client.get(url);
    if *cur > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={}-", cur));
    }
    let resp = req.send().map_err(|e| StreamErr::Network(format!("请求失败: {e}")))?;
    if !resp.status().is_success() {
        return Err(StreamErr::Network(format!("HTTP {}", resp.status())));
    }
    let total = resp.content_length().unwrap_or(0);
    with_dl(size, |d| d.total_bytes = total);

    // 206 为断点续传（追加）；服务器忽略 Range 返回 200 则从头覆盖
    let resume = *cur > 0 && resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut file = if resume {
        std::fs::OpenOptions::new().create(true).append(true).open(part)
            .map_err(|e| StreamErr::Network(format!("写入失败: {e}")))?
    } else {
        *cur = 0;
        std::fs::File::create(part).map_err(|e| StreamErr::Network(format!("写入失败: {e}")))?
    };

    let mut reader = resp; // blocking Response 实现 Read
    let mut buf = [0u8; 64 * 1024];
    let mut last = Instant::now();
    loop {
        if is_canceled(size) {
            return Err(StreamErr::Canceled);
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| StreamErr::Network(format!("读取失败: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| StreamErr::Network(format!("写入失败: {e}")))?;
        *cur += n as u64;
        with_dl(size, |d| d.bytes_downloaded = *cur);
        if last.elapsed().as_millis() >= 250 {
            emit_progress(app, size, *cur, total);
            last = Instant::now();
        }
    }
    Ok(total)
}

fn emit_progress(app: &AppHandle, size: &str, bytes: u64, total: u64) {
    let percent = if total > 0 {
        ((bytes as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as u8
    } else {
        0
    };
    let _ = app.emit(
        EVENT_PROGRESS,
        ModelProgress {
            size: size.into(),
            bytes_downloaded: bytes,
            total_bytes: total,
            percent,
        },
    );
}

/// 后台下载主流程：官方 → 镜像自动回退，校验通过即完工，任一路由取消则终止。
fn download_model_inner(app: AppHandle, size: String) {
    let target = model_file_path(&models_dir(), &size);
    // 已完整且校验通过 → 直接 Done（幂等）
    if target.is_file() && verify_file(&target) {
        let bytes = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
        with_dl(&size, |d| {
            d.status = DlStatus::Done;
            d.error = None;
            d.bytes_downloaded = bytes;
            d.total_bytes = bytes;
        });
        let _ = app.emit(EVENT_DONE, &size);
        release_active(&size);
        return;
    }

    let part = target.with_extension("bin.part");
    let _ = std::fs::create_dir_all(models_dir());
    let mut cur = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    with_dl(&size, |d| {
        d.status = DlStatus::Downloading;
        d.bytes_downloaded = cur;
        d.error = None;
    });

    let client = reqwest::blocking::Client::new();
    let urls = [
        URL_OFFICIAL.replace("{size}", &size),
        URL_MIRROR.replace("{size}", &size),
    ];
    let mut last_err: Option<String> = None;
    let mut done = false;

    for url in urls {
        if is_canceled(&size) {
            break;
        }
        match stream_download(&client, &url, &size, &part, &mut cur, &app) {
            Ok(total) => {
                if is_canceled(&size) {
                    break;
                }
                if verify_file(&part) && total > 0 && cur == total {
                    if std::fs::rename(&part, &target).is_ok() {
                        done = true;
                        break;
                    }
                    last_err = Some("重命名模型文件失败".into());
                } else {
                    last_err = Some("校验失败（文件大小或魔数头不符）".into());
                }
            }
            Err(StreamErr::Canceled) => break,
            Err(StreamErr::Network(e)) => last_err = Some(e),
        }
    }

    if done {
        let bytes = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
        with_dl(&size, |d| {
            d.status = DlStatus::Done;
            d.bytes_downloaded = bytes;
            d.total_bytes = bytes;
            d.error = None;
        });
        let _ = app.emit(EVENT_DONE, &size);
    } else if is_canceled(&size) {
        let _ = std::fs::remove_file(&part);
        clear_cancel(&size);
        with_dl(&size, |d| {
            d.status = DlStatus::Canceled;
            d.error = None;
        });
        let _ = app.emit(EVENT_CANCELED, &size);
    } else {
        with_dl(&size, |d| {
            d.status = DlStatus::Failed;
            d.error = last_err.clone();
        });
        let _ = app.emit(
            EVENT_ERROR,
            ModelError {
                size: size.clone(),
                error: last_err.unwrap_or_else(|| "下载失败".into()),
            },
        );
        // 保留 .part，下次续传
    }
    release_active(&size);
}

fn e2<E: std::fmt::Display>(e: E) -> String {
    format!("{e}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MediaDb;

    #[test]
    fn model_file_path_format() {
        let dir = std::path::PathBuf::from("C:/ml");
        assert_eq!(model_file_path(&dir, "small"), dir.join("ggml-small.bin"));
        assert_eq!(model_file_path(&dir, "base"), dir.join("ggml-base.bin"));
    }

    #[test]
    fn selected_size_defaults_small() {
        let db = MediaDb::open_in_memory().unwrap();
        assert_eq!(selected_size(&db), "small");
    }

    #[test]
    fn selected_size_from_setting() {
        let db = MediaDb::open_in_memory().unwrap();
        db.save_setting("whisper_model", "base").unwrap();
        assert_eq!(selected_size(&db), "base");
    }

    #[test]
    fn verify_detects_magic() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good");
        std::fs::write(&good, b"ggml###payload").unwrap();
        assert!(verify_file(&good));

        let bad = dir.path().join("bad");
        std::fs::write(&bad, b"XXXX###payload").unwrap();
        assert!(!verify_file(&bad));

        let empty = dir.path().join("empty");
        std::fs::write(&empty, b"").unwrap();
        assert!(!verify_file(&empty));
    }
}
