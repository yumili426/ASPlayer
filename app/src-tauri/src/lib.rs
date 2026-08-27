mod db;
mod media;
mod transcriber;

use db::MediaDb;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

/// 全局应用状态：SQLite 连接（互斥锁保护，Arc 便于后台线程共享）
pub struct AppState {
    pub db: Arc<Mutex<MediaDb>>,
}

type CmdResult<T> = Result<T, String>;

fn err_str<E: std::fmt::Display>(e: E) -> String {
    format!("{e}")
}

/// 定位 ffmpeg 并写入 ASPLAYER_FFMPEG，供转写管线（asplayer-transcribe）复用。
/// 若环境变量已设置则保持不变；否则按候选相对位置查找 tools/ffmpeg.exe。
fn resolve_ffmpeg() {
    use std::path::{Path, PathBuf};
    if std::env::var("ASPLAYER_FFMPEG").map(|s| !s.trim().is_empty()).unwrap_or(false) {
        return;
    }
    let candidates: Vec<PathBuf> = vec![
        PathBuf::from("tools/ffmpeg.exe"),
        PathBuf::from("../tools/ffmpeg.exe"),
        PathBuf::from("../../tools/ffmpeg.exe"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/ffmpeg.exe"),
    ];
    for c in candidates {
        if c.is_file() {
            if let Ok(abs) = std::fs::canonicalize(&c) {
                // 幂等设置环境变量
                let _ = std::env::set_var("ASPLAYER_FFMPEG", &abs);
            }
            return;
        }
    }
}

#[tauri::command]
fn import_folder(path: String, state: State<AppState>) -> CmdResult<Vec<media::MediaItem>> {
    let files = media::scan_media_files(std::path::Path::new(&path)).map_err(err_str)?;
    upsert_media_list(&files, &state)?;
    list_media(state)
}

/// 导入单个/多个媒体文件（不扫描目录，直接按路径入库）
#[tauri::command]
fn import_files(paths: Vec<String>, state: State<AppState>) -> CmdResult<Vec<media::MediaItem>> {
    // 仅保留受支持的媒体文件
    let files: Vec<PathBuf> = paths
        .iter()
        .map(PathBuf::from)
        .filter(|p| media::classify_media(p).is_some())
        .collect();
    upsert_media_list(&files, &state)?;
    list_media(state)
}

/// 把一批文件路径写入媒体库（共用）
fn upsert_media_list(files: &[PathBuf], state: &State<AppState>) -> CmdResult<()> {
    let db = state.db.lock().map_err(err_str)?;
    for f in files {
        let path_str = f.to_string_lossy();
        let title = f
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string();
        let mtype = media::classify_media(f).unwrap_or("audio");
        let file_size = std::fs::metadata(f).map(|m| m.len() as i64).unwrap_or(0);
        db.upsert_media(&path_str, &title, mtype, file_size).map_err(err_str)?;
    }
    Ok(())
}

#[tauri::command]
fn list_media(state: State<AppState>) -> CmdResult<Vec<media::MediaItem>> {
    let db = state.db.lock().map_err(err_str)?;
    db.list_media().map_err(err_str)
}

#[tauri::command]
fn save_playback_position(id: i64, position_ms: i64, state: State<AppState>) -> CmdResult<()> {
    let db = state.db.lock().map_err(err_str)?;
    db.save_playback_position(id, position_ms).map_err(err_str)
}

/// 回写前端探测到的媒体时长（导入时未探测，由前端补齐）
#[tauri::command]
fn update_media_duration(id: i64, duration_ms: i64, state: State<AppState>) -> CmdResult<()> {
    let db = state.db.lock().map_err(err_str)?;
    db.update_media_duration(id, duration_ms).map_err(err_str)
}

/// 从媒体库移除某条目（不删除本地文件）
#[tauri::command]
fn remove_media(id: i64, state: State<AppState>) -> CmdResult<()> {
    let db = state.db.lock().map_err(err_str)?;
    db.remove_media(id).map_err(err_str)
}

/// 删除本地文件并从媒体库移除（文件缺失/无权限时不中断）
#[tauri::command]
fn delete_media_file(id: i64, state: State<AppState>) -> CmdResult<()> {
    let db = state.db.lock().map_err(err_str)?;
    let path = match db.media_path(id) {
        Ok((p, _title)) => Some(p),
        Err(_) => None, // 记录已不存在，仅继续清理
    };
    if let Some(p) = path {
        let _ = std::fs::remove_file(&p);
    }
    db.remove_media(id).map_err(err_str)
}

/// 触发后台转写（立即返回，进度/结果走事件）
#[tauri::command]
fn transcribe_media(
    id: i64,
    lang: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    // 同一媒体同时只允许一个转写任务（后台线程内也会兜底校验）
    if transcriber::transcription_running(id) {
        return Err("该媒体已有转写任务在进行中".into());
    }
    let db = state.db.clone();
    std::thread::spawn(move || {
        transcriber::run_transcription(app, db, id, lang);
    });
    Ok(())
}

/// 触发后台翻译（仅转写完成后有效）
#[tauri::command]
fn translate_media(id: i64, app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let db = state.db.clone();
    std::thread::spawn(move || {
        transcriber::run_translation(app, db, id);
    });
    Ok(())
}

/// 请求取消某媒体的转写任务。
/// whisper 推理为单次整体调用不可中断：取消最迟在推理结束后生效，
/// 届时按“是否已有字幕”回退状态并广播 transcribe://canceled 事件。
/// 返回 true 表示取消请求已受理（或本就无任务、顺手修正了残留状态）。
#[tauri::command]
fn cancel_transcribe(id: i64, state: State<AppState>) -> CmdResult<bool> {
    if transcriber::transcription_running(id) {
        transcriber::request_cancel_transcription(id);
        return Ok(true);
    }
    // 不在运行中：修正可能残留的 transcribing 状态（如上次进程中途退出的恢复场景）
    let db = state.db.lock().map_err(err_str)?;
    let (status, _) = db.get_subtitle_status(id).map_err(err_str)?;
    if status == "transcribing" {
        db.rollback_after_cancel(id).map_err(err_str)?;
        return Ok(true);
    }
    Ok(false)
}

/// 保存每文件播放参数（速度/音量），前端变更后防抖调用
#[tauri::command]
fn save_playback_params(id: i64, speed: f64, volume: f64, state: State<AppState>) -> CmdResult<()> {
    let db = state.db.lock().map_err(err_str)?;
    db.save_playback_params(id, speed, volume).map_err(err_str)
}

/// 读取每文件播放参数（速度/音量），无记录时返回 (1.0, 1.0)
#[tauri::command]
fn get_playback_params(id: i64, state: State<AppState>) -> CmdResult<(f64, f64)> {
    let db = state.db.lock().map_err(err_str)?;
    db.get_playback_params(id).map_err(err_str)
}

/// 读取某媒体的字幕
#[tauri::command]
fn get_subtitles(id: i64, state: State<AppState>) -> CmdResult<Vec<transcriber::SubtitleRow>> {
    let db = state.db.lock().map_err(err_str)?;
    db.get_subtitles(id).map_err(err_str)
}

/// 查询某媒体的转写/翻译状态：返回 (status, lang)
#[tauri::command]
fn get_subtitle_status(id: i64, state: State<AppState>) -> CmdResult<(String, String)> {
    let db = state.db.lock().map_err(err_str)?;
    db.get_subtitle_status(id).map_err(err_str)
}

/// 保存设置（合并写入，不覆盖未知键）
#[tauri::command]
fn save_settings(settings: std::collections::HashMap<String, String>, state: State<AppState>) -> CmdResult<()> {
    let db = state.db.lock().map_err(err_str)?;
    for (k, v) in settings {
        db.save_setting(&k, &v).map_err(err_str)?;
    }
    Ok(())
}

/// 读取全部设置
#[tauri::command]
fn get_settings(state: State<AppState>) -> CmdResult<std::collections::HashMap<String, String>> {
    let db = state.db.lock().map_err(err_str)?;
    let rows = db.all_settings().map_err(err_str)?;
    Ok(rows.into_iter().collect())
}

/// 读取翻译 API 环境变量配置（运行时优先于设置表）
#[derive(serde::Serialize)]
struct EnvApiConfig {
    base: String,
    key: String,
    model: String,
}

#[tauri::command]
fn get_env_api_config() -> EnvApiConfig {
    fn val(name: &str) -> String {
        std::env::var(name)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_default()
    }
    EnvApiConfig {
        base: val("ASPLAYER_API_BASE"),
        key: val("ASPLAYER_API_KEY"),
        model: val("ASPLAYER_API_MODEL"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            resolve_ffmpeg();
            let dir: PathBuf = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = MediaDb::open(&dir.join("asplayer.db"))?;
            app.manage(AppState { db: Arc::new(Mutex::new(db)) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            import_folder,
            import_files,
            list_media,
            save_playback_position,
            update_media_duration,
            remove_media,
            delete_media_file,
            transcribe_media,
            translate_media,
            cancel_transcribe,
            save_playback_params,
            get_playback_params,
            get_subtitles,
            get_subtitle_status,
            save_settings,
            get_settings,
            get_env_api_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

