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
const EVENT_CANCELED: &str = "transcribe://canceled";

/// 正在运行的转写任务集合（media_id）。
/// 1) 防止同一媒体并发触发多个转写任务；
/// 2) 支持取消：请求取消 = 从集合移除，任务在下一个检查点自行退出。
/// （whisper 推理为单次整体调用不可中断，取消最迟在其结束后生效）
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

/// 取消检查点：未取消则 Ok(())；已请求取消则按“是否已有字幕”回退状态并返回 Canceled。
fn check_canceled(db: &Arc<Mutex<MediaDb>>, media_id: i64) -> Result<(), TranscribeStop> {
    if transcription_running(media_id) {
        return Ok(());
    }
    // 已请求取消：done→保留旧字幕并恢复 done；否则回退 none
    let _ = with_db(db, |d| d.rollback_after_cancel(media_id));
    Err(TranscribeStop::Canceled)
}

/// 转写步骤主体（锁外耗时操作 + 数据库短锁）
fn transcribe_inner(
    app: &AppHandle,
    db: &Arc<Mutex<MediaDb>>,
    media_id: i64,
    lang_str: &str,
    path: &str,
    tmp: &std::path::Path,
) -> Result<(), TranscribeStop> {
    let _ = with_db(db, |d| d.set_subtitle_status(media_id, "transcribing", lang_str));
    emit_progress(app, media_id, "extract", 5, "抽取音轨…");

    // 抽音轨到临时目录
    let _ = std::fs::create_dir_all(tmp);
    let wav = match asplayer_transcribe::audio::extract_wav(&PathBuf::from(path), tmp) {
        Ok(w) => w,
        Err(e) => return Err(fail(db, media_id, format!("抽音轨失败: {e}"))),
    };
    check_canceled(db, media_id)?;

    emit_progress(app, media_id, "transcribe", 15, "Whisper 转写中…");
    let samples = match asplayer_transcribe::audio::read_samples_f32(&wav) {
        Ok(s) => s,
        Err(e) => return Err(fail(db, media_id, format!("读取音频失败: {e}"))),
    };
    check_canceled(db, media_id)?;

    let model = {
        let g = db.lock().map_err(|e| fail(db, media_id, format!("数据库锁异常: {e}")))?;
        crate::models::resolve_model_path(&g).to_string_lossy().into_owned()
    };
    let lang_opt = if lang_str.is_empty() { None } else { Some(lang_str) };
    let segments = match asplayer_transcribe::whisper::transcribe(&model, lang_opt, &samples) {
        Ok(segs) => segs,
        Err(e) => return Err(fail(db, media_id, format!("转写失败: {e}"))),
    };

    // whisper 推理为单次整体调用不可中断：结束后立即落盘“推理期间到达的取消请求”
    if !transcription_running(media_id) {
        let _ = with_db(db, |d| d.rollback_after_cancel(media_id));
        return Err(TranscribeStop::Canceled);
    }

    emit_progress(app, media_id, "transcribe", 80, &format!("写入 {} 段字幕…", segments.len()));
    let _ = with_db(db, |d| {
        d.clear_subtitles(media_id)?;
        for (i, seg) in segments.iter().enumerate() {
            d.save_subtitle(media_id, seg.start_ms as i64, seg.end_ms as i64, &seg.text, "", i as i64)?;
        }
        d.set_subtitle_status(media_id, "done", lang_str)
    });
    Ok(())
}

/// 转写任务（后台线程调用）：抽音轨 → whisper → 逐段写库 → 状态置 done。
/// 任何退出路径统一：清理临时目录、注销运行标记。
pub fn run_transcription(
    app: AppHandle,
    db: Arc<Mutex<MediaDb>>,
    media_id: i64,
    lang: Option<String>,
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

    let tmp = std::env::temp_dir().join(format!("asplayer-{media_id}"));
    let result = transcribe_inner(&app, &db, media_id, &lang.unwrap_or_default(), &path, &tmp);

    let _ = std::fs::remove_dir_all(&tmp);
    unregister_transcription(media_id);

    match result {
        Ok(()) => {
            emit_progress(&app, media_id, "done", 100, "转写完成");
            let _ = app.emit(EVENT_DONE, media_id);
        }
        Err(TranscribeStop::Canceled) => {
            let _ = app.emit(EVENT_CANCELED, media_id);
        }
        Err(TranscribeStop::Error(msg)) => {
            let _ = app.emit(EVENT_ERROR, msg);
        }
    }
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

