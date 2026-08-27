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
}
