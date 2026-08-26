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

#[tauri::command]
fn import_folder(path: String, state: State<AppState>) -> CmdResult<Vec<media::MediaItem>> {
    let files = media::scan_media_files(std::path::Path::new(&path)).map_err(err_str)?;
    {
        let db = state.db.lock().map_err(err_str)?;
        for f in &files {
            let path_str = f.to_string_lossy();
            let title = f
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("untitled")
                .to_string();
            let mtype = media::classify_media(f).unwrap_or("audio");
            db.upsert_media(&path_str, &title, mtype).map_err(err_str)?;
        }
    }
    list_media(state)
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
            let dir: PathBuf = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = MediaDb::open(&dir.join("asplayer.db"))?;
            app.manage(AppState { db: Arc::new(Mutex::new(db)) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            import_folder,
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

