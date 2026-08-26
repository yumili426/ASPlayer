mod db;
mod media;

use db::MediaDb;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Manager, State};

/// 全局应用状态：SQLite 连接（互斥锁保护）
pub struct AppState {
    db: Mutex<MediaDb>,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir: PathBuf = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = MediaDb::open(&dir.join("asplayer.db"))?;
            app.manage(AppState { db: Mutex::new(db) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            import_folder,
            list_media,
            save_playback_position
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

