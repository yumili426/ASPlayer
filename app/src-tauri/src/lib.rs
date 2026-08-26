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
        db.upsert_media(&path_str, &title, mtype).map_err(err_str)?;
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

/// 触发后台转写（立即返回，进度/结果走事件）
#[tauri::command]
fn transcribe_media(
    id: i64,
    lang: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<()> {
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
            transcribe_media,
            translate_media,
            get_subtitles,
            get_subtitle_status,
            save_settings,
            get_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

