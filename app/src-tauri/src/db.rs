//! SQLite 媒体库存储层

use crate::media::MediaItem;
use rusqlite::Connection;

pub struct MediaDb {
    conn: Connection,
}

impl MediaDb {
    /// 打开（或创建）指定路径的数据库并确保表结构存在。
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(&conn)?;
        Ok(Self { conn })
    }

    /// 内存库（测试用）
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(&conn)?;
        Ok(Self { conn })
    }

    fn init(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS media_files (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                path              TEXT NOT NULL UNIQUE,
                title             TEXT NOT NULL,
                media_type        TEXT NOT NULL,
                duration_ms       INTEGER NOT NULL DEFAULT 0,
                playback_position INTEGER NOT NULL DEFAULT 0,
                speed             REAL NOT NULL DEFAULT 1.0,
                added_at          TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
    }

    /// 按路径插入或更新，返回 id
    pub fn upsert_media(&self, path: &str, title: &str, media_type: &str) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO media_files (path, title, media_type) VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET title = excluded.title",
            [path, title, media_type],
        )?;
        Ok(self.conn.query_row("SELECT id FROM media_files WHERE path = ?1", [path], |r| r.get(0))?)
    }

    pub fn list_media(&self) -> rusqlite::Result<Vec<MediaItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, media_type, duration_ms, playback_position
             FROM media_files ORDER BY added_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(MediaItem {
                id: r.get(0)?,
                path: r.get(1)?,
                title: r.get(2)?,
                media_type: r.get(3)?,
                duration_ms: r.get(4)?,
                playback_position: r.get(5)?,
            })
        })?;
        rows.collect()
    }

    pub fn save_playback_position(&self, id: i64, position_ms: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE media_files SET playback_position = ?1 WHERE id = ?2",
            [position_ms.to_string().as_str(), &id.to_string()],
        )?;
        Ok(())
    }
}

use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_and_list() -> rusqlite::Result<()> {
        let db = MediaDb::open_in_memory()?;
        let id1 = db.upsert_media("D:/m/a.mp4", "a", "video")?;
        db.upsert_media("D:/m/b.mp3", "b", "audio")?;
        // 重复插入同路径 → 更新而非新增
        let id_again = db.upsert_media("D:/m/a.mp4", "a-renamed", "video")?;
        assert_eq!(id1, id_again);

        let items = db.list_media()?;
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.title == "a-renamed"));
        Ok(())
    }

    #[test]
    fn save_position_roundtrip() -> rusqlite::Result<()> {
        let db = MediaDb::open_in_memory()?;
        let id = db.upsert_media("D:/m/a.mp3", "a", "audio")?;
        db.save_playback_position(id, 42_000)?;
        let items = db.list_media()?;
        assert_eq!(items[0].playback_position, 42_000);
        Ok(())
    }
}
