//! 转写/翻译后台任务：封装 ffmpeg→whisper→翻译 的调用，负责与数据库交互。
//! 耗时操作在独立线程执行，通过 `tauri::AppHandle` 事件向前端推送进度。

use crate::db::MediaDb;
use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
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

/// 转写进度事件负载
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressPayload {
    media_id: i64,
    stage: String,  // "extract" | "transcribe" | "translate" | "done"
    progress: u8,   // 0-100
    message: String,
}

const EVENT_PROGRESS: &str = "transcribe://progress";
const EVENT_DONE: &str = "transcribe://done";
const EVENT_ERROR: &str = "transcribe://error";

/// 解析 whisper 模型路径：环境变量 ASPLAYER_MODEL > 默认 ~/.asplayer/models/ggml-small.bin
pub fn model_path() -> PathBuf {
    if let Ok(m) = std::env::var("ASPLAYER_MODEL") {
        if !m.trim().is_empty() {
            return PathBuf::from(m);
        }
    }
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".asplayer").join("models").join("ggml-small.bin")
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
/// 转写任务（后台线程调用）：抽音轨 → whisper → 逐段写库 → 状态置 done
pub fn run_transcription(
    app: AppHandle,
    db: Arc<Mutex<MediaDb>>,
    media_id: i64,
    lang: Option<String>,
) {
    // 此处的耗时步骤（抽音轨、whisper）都在锁外，仅数据库读写短锁。
    let path = match with_db(&db, |d| d.media_path(media_id)).map(|v| v.0) {
        Ok(p) => p,
        Err(e) => {
            let _ = app.emit(EVENT_ERROR, format!("找不到媒体: {e}"));
            return;
        }
    };

    let lang_str = lang.unwrap_or_default();
    let _ = with_db(&db, |d| d.set_subtitle_status(media_id, "transcribing", &lang_str));
    emit_progress(&app, media_id, "extract", 5, "抽取音轨…");

    // 抽音轨到临时目录
    let tmp = std::env::temp_dir().join(format!("asplayer-{media_id}"));
    let _ = std::fs::create_dir_all(&tmp);
    let wav = match asplayer_transcribe::audio::extract_wav(&PathBuf::from(&path), &tmp) {
        Ok(w) => w,
        Err(e) => {
            let _ = with_db(&db, |d| d.set_subtitle_status(media_id, "error", ""));
            let _ = app.emit(EVENT_ERROR, format!("抽音轨失败: {e}"));
            return;
        }
    };

    emit_progress(&app, media_id, "transcribe", 15, "Whisper 转写中…");
    let samples = match asplayer_transcribe::audio::read_samples_f32(&wav) {
        Ok(s) => s,
        Err(e) => {
            let _ = with_db(&db, |d| d.set_subtitle_status(media_id, "error", ""));
            let _ = app.emit(EVENT_ERROR, format!("读取音频失败: {e}"));
            return;
        }
    };

    let model = model_path().to_string_lossy().into_owned();
    let lang_opt = if lang_str.is_empty() { None } else { Some(lang_str.as_str()) };
    let segments = match asplayer_transcribe::whisper::transcribe(&model, lang_opt, &samples) {
        Ok(segs) => segs,
        Err(e) => {
            let _ = with_db(&db, |d| d.set_subtitle_status(media_id, "error", ""));
            let _ = app.emit(EVENT_ERROR, format!("转写失败: {e}"));
            return;
        }
    };

    emit_progress(&app, media_id, "transcribe", 80, &format!("写入 {} 段字幕…", segments.len()));
    let _ = with_db(&db, |d| {
        d.clear_subtitles(media_id)?;
        for (i, seg) in segments.iter().enumerate() {
            d.save_subtitle(media_id, seg.start_ms as i64, seg.end_ms as i64, &seg.text, "", i as i64)?;
        }
        d.set_subtitle_status(media_id, "done", &lang_str)
    });

    let _ = std::fs::remove_dir_all(&tmp);
    emit_progress(&app, media_id, "done", 100, "转写完成");
    let _ = app.emit(EVENT_DONE, media_id);
}


/// 翻译任务（后台线程调用）：读取未翻译段 → 批量翻译 → 回写
pub fn run_translation(app: AppHandle, db: Arc<Mutex<MediaDb>>, media_id: i64) {
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

    let cfg = with_db(&db, |d| Ok(resolve_api_config(d))).unwrap_or_else(|_| {
        ApiConfig { api_base: String::new(), api_key: String::new(), model: "deepseek-chat".into() }
    });
    if cfg.api_key.is_empty() {
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

