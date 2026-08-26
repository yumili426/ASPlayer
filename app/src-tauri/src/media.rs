//! 媒体库：目录扫描、格式过滤（纯函数，可单测）

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 支持的媒体扩展名（小写）
const AUDIO_EXTS: &[&str] = &["mp3", "m4a", "wav", "flac", "ogg", "aac"];
const VIDEO_EXTS: &[&str] = &["mp4", "webm", "mkv", "mov"];

#[derive(Debug, Clone, Serialize)]
pub struct MediaItem {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub media_type: String, // "video" | "audio"
    pub duration_ms: i64,
    pub playback_position: i64,
}

/// 判断文件是否为受支持的媒体，返回 Some("audio"/"video")
pub fn classify_media(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if AUDIO_EXTS.contains(&ext.as_str()) {
        Some("audio")
    } else if VIDEO_EXTS.contains(&ext.as_str()) {
        Some("video")
    } else {
        None
    }
}

/// 递归扫描目录，返回其中的媒体文件路径列表（不做数据库操作）。
/// 跳过以 . 开头的隐藏文件/目录。
pub fn scan_media_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if classify_media(&path).is_some() {
                out.push(path);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn classify_by_extension() {
        assert_eq!(classify_media(Path::new("a/b.MP4")), Some("video"));
        assert_eq!(classify_media(Path::new("a/b.mp3")), Some("audio"));
        assert_eq!(classify_media(Path::new("a/b.txt")), None);
        assert_eq!(classify_media(Path::new("a/b")), None);
    }

    #[test]
    fn scan_finds_media_and_skips_hidden() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join("song.mp3"), b"x")?;
        fs::write(dir.path().join("movie.mp4"), b"x")?;
        fs::write(dir.path().join("notes.txt"), b"x")?;
        fs::create_dir(dir.path().join(".hidden"))?;
        fs::write(dir.path().join(".hidden/secret.wav"), b"x")?;

        let found = scan_media_files(dir.path())?;
        assert_eq!(found.len(), 2);
        Ok(())
    }
}
