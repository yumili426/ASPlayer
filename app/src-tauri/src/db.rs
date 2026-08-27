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
                file_size         INTEGER NOT NULL DEFAULT 0,
                speed             REAL NOT NULL DEFAULT 1.0,
                subtitle_status   TEXT NOT NULL DEFAULT 'none',
                subtitle_lang     TEXT NOT NULL DEFAULT '',
                added_at          TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS subtitles (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                media_id   INTEGER NOT NULL,
                start_ms   INTEGER NOT NULL,
                end_ms     INTEGER NOT NULL,
                text       TEXT NOT NULL DEFAULT '',
                translation TEXT NOT NULL DEFAULT '',
                ordinal    INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (media_id) REFERENCES media_files(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_sub_media ON subtitles(media_id, start_ms);

            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;
        // 迁移已存在的旧库：为其补充 M2 新增的列（CREATE TABLE IF NOT EXISTS 不会改旧表）
        Self::migrate(conn)?;
        Self::migrate_playback_params(conn)?;
        Self::migrate_transcribe(conn)
    }

    /// 幂等迁移：给 media_files 补齐缺失的列（老版本数据库升级用）。
    fn migrate(conn: &Connection) -> rusqlite::Result<()> {
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(media_files)")?
            .query_map([], |r| r.get(1))?
            .collect::<Result<_, _>>()?;

        if !cols.iter().any(|c| c == "subtitle_status") {
            conn.execute_batch(
                "ALTER TABLE media_files ADD COLUMN subtitle_status TEXT NOT NULL DEFAULT 'none';
                 ALTER TABLE media_files ADD COLUMN subtitle_lang TEXT NOT NULL DEFAULT '';",
            )?;
        }
        if !cols.iter().any(|c| c == "file_size") {
            conn.execute_batch(
                "ALTER TABLE media_files ADD COLUMN file_size INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        if !cols.iter().any(|c| c == "transcribe_next_ms") {
            conn.execute_batch(
                "ALTER TABLE media_files ADD COLUMN transcribe_next_ms INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        Ok(())
    }

    /// 幂等迁移（v2）：补 volume 列（speed 列建表即有，旧库可能没有，一并兜底）。
    pub fn migrate_playback_params(conn: &Connection) -> rusqlite::Result<()> {
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(media_files)")?
            .query_map([], |r| r.get(1))?
            .collect::<Result<_, _>>()?;

        if !cols.iter().any(|c| c == "volume") {
            conn.execute_batch(
                "ALTER TABLE media_files ADD COLUMN volume REAL;",
            )?;
        }
        if !cols.iter().any(|c| c == "speed") {
            conn.execute_batch(
                "ALTER TABLE media_files ADD COLUMN speed REAL NOT NULL DEFAULT 1.0;",
            )?;
        }
        Ok(())
    }

    /// 按路径插入或更新，返回 id
    pub fn upsert_media(&self, path: &str, title: &str, media_type: &str, file_size: i64) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO media_files (path, title, media_type, file_size) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET title = excluded.title, file_size = excluded.file_size",
            rusqlite::params![path, title, media_type, file_size],
        )?;
        Ok(self.conn.query_row("SELECT id FROM media_files WHERE path = ?1", [path], |r| r.get(0))?)
    }

    pub fn list_media(&self) -> rusqlite::Result<Vec<MediaItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.path, m.title, m.media_type, m.duration_ms, m.playback_position,
                    m.file_size, m.subtitle_status, m.subtitle_lang,
                    (SELECT COUNT(*) FROM subtitles s WHERE s.media_id = m.id),
                    COALESCE(m.speed, 1.0), COALESCE(m.volume, 1.0),
                    m.transcribe_next_ms
             FROM media_files m ORDER BY m.added_at DESC, m.id DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(MediaItem {
                id: r.get(0)?,
                path: r.get(1)?,
                title: r.get(2)?,
                media_type: r.get(3)?,
                duration_ms: r.get(4)?,
                playback_position: r.get(5)?,
                file_size: r.get(6)?,
                subtitle_status: r.get(7)?,
                subtitle_lang: r.get(8)?,
                subtitle_count: r.get(9)?,
                transcribe_next_ms: r.get(12)?,
                speed: r.get(10)?,
                volume: r.get(11)?,
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

    /// 读取每文件播放参数（速度/音量）；无记录时返回默认值 1.0
    pub fn get_playback_params(&self, id: i64) -> rusqlite::Result<(f64, f64)> {
        self.conn.query_row(
            "SELECT COALESCE(speed, 1.0), COALESCE(volume, 1.0) FROM media_files WHERE id = ?1",
            [&id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// 保存每文件播放参数（速度/音量），由前端在变更后防抖调用
    pub fn save_playback_params(&self, id: i64, speed: f64, volume: f64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE media_files SET speed = ?1, volume = ?2 WHERE id = ?3",
            rusqlite::params![speed, volume, id],
        )?;
        Ok(())
    }

    /// 回写前端探测到的媒体时长（导入时未探测，由前端加载元数据后补齐）
    pub fn update_media_duration(&self, id: i64, duration_ms: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE media_files SET duration_ms = ?1 WHERE id = ?2",
            [duration_ms.to_string().as_str(), &id.to_string()],
        )?;
        Ok(())
    }

    /// 从库中移除某媒体（级联删除其字幕）
    pub fn remove_media(&self, id: i64) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM subtitles WHERE media_id = ?1", [&id.to_string()])?;
        self.conn
            .execute("DELETE FROM media_files WHERE id = ?1", [&id.to_string()])?;
        Ok(())
    }

    /// 取消转写后回退状态：已有字幕→恢复 done（保留旧字幕）；否则→none。
    /// 返回回退后的状态字符串（"done" | "none"）。
    pub fn rollback_after_cancel(&self, media_id: i64) -> rusqlite::Result<String> {
        let has: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM subtitles WHERE media_id = ?1",
            [&media_id.to_string()],
            |r| r.get(0),
        )?;
        if has > 0 {
            self.conn.execute(
                "UPDATE media_files SET subtitle_status = 'done' WHERE id = ?1",
                [&media_id.to_string()],
            )?;
            Ok("done".into())
        } else {
            self.conn.execute(
                "UPDATE media_files SET subtitle_status = 'none', subtitle_lang = '' WHERE id = ?1",
                [&media_id.to_string()],
            )?;
            Ok("none".into())
        }
    }

    /// 清空某媒体的所有字幕（重新转写前调用）
    pub fn clear_subtitles(&self, media_id: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM subtitles WHERE media_id = ?1",
            [&media_id.to_string()],
        )?;
        Ok(())
    }

    /// 写入单条字幕段（upsert by media_id+start_ms）
    pub fn save_subtitle(
        &self,
        media_id: i64,
        start_ms: i64,
        end_ms: i64,
        text: &str,
        translation: &str,
        ordinal: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO subtitles (media_id, start_ms, end_ms, text, translation, ordinal)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT DO UPDATE SET end_ms = excluded.end_ms,
                                       text = excluded.text,
                                       translation = CASE WHEN excluded.translation = '' THEN subtitles.translation ELSE excluded.translation END,
                                       ordinal = excluded.ordinal",
            rusqlite::params![
                media_id,
                start_ms,
                end_ms,
                text,
                translation,
                ordinal
            ],
        )?;
        Ok(())
    }

    /// 读取某媒体的全部字幕（按开始时间升序）
    pub fn get_subtitles(
        &self,
        media_id: i64,
    ) -> rusqlite::Result<Vec<crate::transcriber::SubtitleRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT start_ms, end_ms, text, translation, ordinal
             FROM subtitles WHERE media_id = ?1 ORDER BY start_ms ASC",
        )?;
        let rows = stmt.query_map([&media_id.to_string()], |r| {
            Ok(crate::transcriber::SubtitleRow {
                start_ms: r.get(0)?,
                end_ms: r.get(1)?,
                text: r.get(2)?,
                translation: r.get(3)?,
                ordinal: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// 仅读取尚未翻译的字幕段（translation 为空）
    pub fn get_untranslated_subtitles(
        &self,
        media_id: i64,
    ) -> rusqlite::Result<Vec<crate::transcriber::SubtitleRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT start_ms, end_ms, text, translation, ordinal
             FROM subtitles WHERE media_id = ?1 AND translation = '' ORDER BY start_ms ASC",
        )?;
        let rows = stmt.query_map([&media_id.to_string()], |r| {
            Ok(crate::transcriber::SubtitleRow {
                start_ms: r.get(0)?,
                end_ms: r.get(1)?,
                text: r.get(2)?,
                translation: r.get(3)?,
                ordinal: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// 更新单条字幕的译文
    pub fn update_translation(
        &self,
        media_id: i64,
        start_ms: i64,
        translation: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE subtitles SET translation = ?1 WHERE media_id = ?2 AND start_ms = ?3",
            rusqlite::params![translation, media_id, start_ms],
        )?;
        Ok(())
    }

    /// 设置某媒体的字幕状态与语言
    pub fn set_subtitle_status(
        &self,
        media_id: i64,
        status: &str,
        lang: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE media_files SET subtitle_status = ?1, subtitle_lang = ?2 WHERE id = ?3",
            rusqlite::params![status, lang, media_id],
        )?;
        Ok(())
    }

    /// 读取某媒体的状态
    pub fn get_subtitle_status(
        &self,
        media_id: i64,
    ) -> rusqlite::Result<(String, String)> {
        self.conn.query_row(
            "SELECT subtitle_status, subtitle_lang FROM media_files WHERE id = ?1",
            [&media_id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// 读取某媒体的 path 与 title
    pub fn media_path(&self, media_id: i64) -> rusqlite::Result<(String, String)> {
        self.conn.query_row(
            "SELECT path, title FROM media_files WHERE id = ?1",
            [&media_id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// 保存/覆盖一条设置
    pub fn save_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// 读取一条设置，缺省返回 None
    pub fn get_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query_map([key], |r| r.get::<_, String>(0))?;
        rows.next().transpose()
    }

    /// 读取全部设置（k/v 键值对）
    pub fn all_settings(&self) -> rusqlite::Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// 幂等迁移：给 subtitles 补 UNIQUE(media_id, start_ms)（先按该组合去重）。
    /// 这是 save_subtitle 的 ON CONFLICT DO UPDATE 真正生效、支撑续跑幂等写的前提。
    pub fn migrate_transcribe(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "DELETE FROM subtitles WHERE id NOT IN (
                SELECT MIN(id) FROM subtitles GROUP BY media_id, start_ms
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_sub_unique ON subtitles(media_id, start_ms);",
        )?;
        Ok(())
    }

    /// 读取某媒体已转写的音频毫秒断点（0 = 无断点）
    pub fn get_transcribe_next_ms(&self, id: i64) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT transcribe_next_ms FROM media_files WHERE id = ?1",
            [&id.to_string()],
            |r| r.get(0),
        )
    }

    /// 写入某媒体的转写断点（完成后置 0 清除）
    pub fn set_transcribe_next_ms(&self, id: i64, next_ms: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE media_files SET transcribe_next_ms = ?1 WHERE id = ?2",
            rusqlite::params![next_ms, id],
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
        let id1 = db.upsert_media("D:/m/a.mp4", "a", "video", 0)?;
        db.upsert_media("D:/m/b.mp3", "b", "audio", 0)?;
        // 重复插入同路径 → 更新而非新增
        let id_again = db.upsert_media("D:/m/a.mp4", "a-renamed", "video", 0)?;
        assert_eq!(id1, id_again);

        let items = db.list_media()?;
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.title == "a-renamed"));
        Ok(())
    }

    #[test]
    fn save_position_roundtrip() -> rusqlite::Result<()> {
        let db = MediaDb::open_in_memory()?;
        let id = db.upsert_media("D:/m/a.mp3", "a", "audio", 0)?;
        db.save_playback_position(id, 42_000)?;
        let items = db.list_media()?;
        assert_eq!(items[0].playback_position, 42_000);
        Ok(())
    }

    #[test]
    fn subtitles_roundtrip() -> rusqlite::Result<()> {
        let db = MediaDb::open_in_memory()?;
        let id = db.upsert_media("D:/m/a.mp3", "a", "audio", 0)?;
        db.save_subtitle(id, 0, 1500, "おやすみ", "", 0)?;
        db.save_subtitle(id, 2000, 4000, "good night", "", 1)?;
        // 更新第一条译文
        db.update_translation(id, 0, "晚安")?;

        let rows = db.get_subtitles(id)?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].translation, "晚安");
        assert_eq!(rows[1].text, "good night");

        // 未翻译段只剩第二条
        let untrans = db.get_untranslated_subtitles(id)?;
        assert_eq!(untrans.len(), 1);
        assert_eq!(untrans[0].text, "good night");
        Ok(())
    }

    #[test]
    fn subtitle_status_roundtrip() -> rusqlite::Result<()> {
        let db = MediaDb::open_in_memory()?;
        let id = db.upsert_media("D:/m/a.mp3", "a", "audio", 0)?;
        db.set_subtitle_status(id, "transcribing", "ja")?;
        let (s, l) = db.get_subtitle_status(id)?;
        assert_eq!(s, "transcribing");
        assert_eq!(l, "ja");
        db.set_subtitle_status(id, "done", "ja")?;
        let (s, _) = db.get_subtitle_status(id)?;
        assert_eq!(s, "done");
        Ok(())
    }

    #[test]
    fn settings_roundtrip() -> rusqlite::Result<()> {
        let db = MediaDb::open_in_memory()?;
        assert_eq!(db.get_setting("api_key")?, None);
        db.save_setting("api_key", "sk-test")?;
        db.save_setting("api_base", "https://api.deepseek.com/v1")?;
        assert_eq!(db.get_setting("api_key")?.as_deref(), Some("sk-test"));
        // 覆盖
        db.save_setting("api_key", "sk-new")?;
        assert_eq!(db.get_setting("api_key")?.as_deref(), Some("sk-new"));
        let all = db.all_settings()?;
        assert_eq!(all.len(), 2); // 默认设置项由前端面板首次写入，DB 层不做种子数据
        Ok(())
    }

    #[test]
    fn playback_params_roundtrip() -> rusqlite::Result<()> {
        let db = MediaDb::open_in_memory()?;
        let id = db.upsert_media("D:/m/a.mp4", "a", "video", 0)?;
        // 未保存过时返回默认值
        assert_eq!(db.get_playback_params(id)?, (1.0, 1.0));
        db.save_playback_params(id, 1.5, 0.7)?;
        assert_eq!(db.get_playback_params(id)?, (1.5, 0.7));
        // 再次覆盖
        db.save_playback_params(id, 0.75, 1.0)?;
        assert_eq!(db.get_playback_params(id)?, (0.75, 1.0));
        Ok(())
    }

    #[test]
    fn migrate_adds_volume_column() -> rusqlite::Result<()> {
        // 构造一个含 speed 但缺 volume 的库（等价于本次功能上线前的旧库）
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE media_files (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                path              TEXT NOT NULL UNIQUE,
                title             TEXT NOT NULL,
                media_type        TEXT NOT NULL,
                duration_ms       INTEGER NOT NULL DEFAULT 0,
                playback_position INTEGER NOT NULL DEFAULT 0,
                speed             REAL NOT NULL DEFAULT 1.0,
                added_at          TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;

        MediaDb::migrate_playback_params(&conn)?;

        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(media_files)")?
            .query_map([], |r| r.get(1))?
            .collect::<Result<_, _>>()?;
        assert!(cols.iter().any(|c| c == "volume"));
        assert!(cols.iter().any(|c| c == "speed"));

        // 幂等：再跑一次不报错；且旧数据读取回退到默认音量
        MediaDb::migrate_playback_params(&conn)?;
        conn.execute(
            "INSERT INTO media_files (path, title, media_type) VALUES ('D:/x.mp4', 'x', 'video')",
            [],
        )?;
        let v: f64 = conn.query_row(
            "SELECT COALESCE(volume, 1.0) FROM media_files WHERE path = 'D:/x.mp4'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(v, 1.0);
        Ok(())
    }

    #[test]
    fn list_media_includes_subtitle_fields() -> rusqlite::Result<()> {
        let db = MediaDb::open_in_memory()?;
        let id = db.upsert_media("D:/m/a.mp4", "a", "video", 0)?;
        db.save_subtitle(id, 0, 1000, "hi", "", 0)?;
        db.set_subtitle_status(id, "done", "ja")?;
        let items = db.list_media()?;
        assert_eq!(items[0].subtitle_status, "done");
        assert_eq!(items[0].subtitle_lang, "ja");
        assert_eq!(items[0].subtitle_count, 1);
        Ok(())
    }

    #[test]
    fn migrate_adds_subtitle_columns_to_old_schema() -> rusqlite::Result<()> {
        // 构造一个旧结构库：media_files 不含 subtitle_status/subtitle_lang
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE media_files (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                path              TEXT NOT NULL UNIQUE,
                title             TEXT NOT NULL,
                media_type        TEXT NOT NULL,
                duration_ms       INTEGER NOT NULL DEFAULT 0,
                playback_position INTEGER NOT NULL DEFAULT 0,
                speed             REAL NOT NULL DEFAULT 1.0,
                added_at          TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;

        // 迁移前断言无新列
        {
            let cols: Vec<String> = conn
                .prepare("PRAGMA table_info(media_files)")?
                .query_map([], |r| r.get(1))?
                .collect::<Result<_, _>>()?;
            assert!(!cols.iter().any(|c| c == "subtitle_status"));
        }

        // 执行迁移
        MediaDb::migrate(&conn)?;

        // 迁移后断言新列存在
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(media_files)")?
            .query_map([], |r| r.get(1))?
            .collect::<Result<_, _>>()?;
        assert!(cols.iter().any(|c| c == "subtitle_status"));
        assert!(cols.iter().any(|c| c == "subtitle_lang"));

        // 幂等：再跑一次不报错
        MediaDb::migrate(&conn)?;
        Ok(())
    }
}

