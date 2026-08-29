# 外部字幕导入（SRT/VTT）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让用户可为任一媒体导入外部 `.srt`/`.vtt` 字幕，替换该媒体已有字幕并置 `done`（解锁翻译），跳过转写。

**Architecture:** 纯函数解析（`asplayer-transcribe/subtitle_import.rs`）产出 `Vec<srt::Segment>`；DB 层新增事务方法 `replace_subtitles`（clear + upsert + 置 done + 断点清零）；`lib.rs` 加命令 `import_external_subtitle(media_id, path?)` 串联解析+写库，支持手动选文件与同名自动检测。前端加「导入字幕」入口（播放器工具栏 + 列表右键），成功后被媒体轻量刷新。

**Tech Stack:** Rust（rusqlite、tauri、anyhow、encoding_rs）+ whisper/翻译管线复用；Vue 3 + Pinia + @tauri-apps/plugin-dialog。命令用 `"$HOME/.cargo/bin/cargo"`（不在 PATH）。前端索引构建用 `npx vue-tsc --noEmit`。

---

## 文件结构

| 文件 | 职责 |
|---|---|
| `crates/asplayer-transcribe/src/subtitle_import.rs`（新增） | SRT/VTT 解析 + 字符集处理；纯函数，可单测 |
| `crates/asplayer-transcribe/src/lib.rs` | 注册 `pub mod subtitle_import;` |
| `crates/asplayer-transcribe/Cargo.toml` | 追加 `encoding_rs` 依赖 |
| `app/src-tauri/src/db.rs` | 新增 `replace_subtitles`（事务替换 + 置 done + 断点清零） |
| `app/src-tauri/src/media.rs` | 新增 `find_sibling_subtitle`（同名 .srt/.vtt 探测） |
| `app/src-tauri/src/lib.rs` | 新增命令 `import_external_subtitle` + 辅助 `import_external` + 注册 |
| `app/src/api/subtitle.ts` | 新增 `importExternalSubtitle(mediaId, path?)` |
| `app/src/App.vue` | 新增 `onImportSubtitle` + 模板接线 |
| `app/src/components/PlayerStage.vue` | 工具栏加「导入字幕」按钮 + 事件 |
| `app/src/components/PlaylistPanel.vue` | 右键菜单加「导入字幕」项 + 事件 |

---

### Task 1: crate — `parse_srt`

**Files:**
- Create: `crates/asplayer-transcribe/src/subtitle_import.rs`
- Modify: `crates/asplayer-transcribe/Cargo.toml`
- Modify: `crates/asplayer-transcribe/src/lib.rs`
- Test: `crates/asplayer-transcribe/src/subtitle_import.rs`

- [ ] **Step 1: 加依赖 + 注册模块 + 写失败测试**

`crates/asplayer-transcribe/Cargo.toml` 的 `[dependencies]` 增加一行：
```toml
encoding_rs = "0.8"
```

`crates/asplayer-transcribe/src/lib.rs` 第 3 行后加一行：
```rust
pub mod subtitle_import;
```

`crates/asplayer-transcribe/src/subtitle_import.rs`（仅解析 SRT，先写核心 + 时间戳助手）：
```rust
use crate::srt::Segment;
use std::path::Path;

/// MM:SS / HH:MM:SS + 毫秒（分隔符兼容 , 或 .）。返回毫秒。小时可省略。
fn parse_timestamp(ts: &str) -> Option<u64> {
    let ts = ts.trim();
    let (int_part, frac) = match ts.find([',', '.']) {
        Some(i) => (&ts[..i], &ts[i + 1..].trim()),
        None => (ts, ""),
    };
    let frac_ms: u64 = {
        if frac.is_empty() {
            0
        } else {
            let n = frac.len().min(3);
            let v: u64 = frac[..n].parse().unwrap_or(0);
            v * 10u64.pow(3 - n as u32)
        }
    };
    let parts: Vec<u64> = int_part
        .split(':')
        .map(|p| p.trim().parse::<u64>().unwrap_or(0))
        .collect();
    let (h, m, s) = match parts.as_slice() {
        [s] => (0, 0, *s),
        [m, s] => (0, *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    Some(((h * 60 + m) * 60 + s) * 1000 + frac_ms)
}

fn timeline(line: &str) -> Option<(u64, u64)> {
    let mut it = line.split("-->");
    let start = parse_timestamp(it.next()?)?;
    let end = parse_timestamp(it.next()?)?;
    Some((start, end))
}

/// 剥离行内 HTML 标签、去 BOM、多行拼接为单段文本。
fn cleanup_text(text: &str) -> String {
    let mut in_tag = false;
    let mut result = String::new();
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
        .lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join(" ")
        .replace('\u{feff}', "")
        .trim()
        .to_string()
}

fn sort_by_start(segs: &mut [Segment]) {
    segs.sort_by_key(|s| s.start_ms);
}

/// 解析 SRT 文本 → 段序列（升序、滤 `end<=start`、滤空文本）。
pub fn parse_srt(input: &str) -> Vec<Segment> {
    let normalized = input.replace("\r\n", "\n");
    let mut out = Vec::new();
    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(ti) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let Some((start, end)) = timeline(lines[ti]) else { continue };
        if end <= start {
            continue;
        }
        let text = cleanup_text(&lines[ti + 1..].join("\n"));
        if text.is_empty() {
            continue;
        }
        out.push(Segment { start_ms: start, end_ms: end, text });
    }
    sort_by_start(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_timestamp_mm_ss_and_hh_mm_ss_ok() {
        assert_eq!(parse_timestamp("00:01,500"), Some(1500));
        assert_eq!(parse_timestamp("00:01.500"), Some(1500));
        assert_eq!(parse_timestamp("01:02:03,000"), Some(3_723_000));
        assert_eq!(parse_timestamp("02.75"), Some(2750)); // "75"×10 = 750ms
        assert_eq!(parse_timestamp("not-a-time"), None);
    }

    #[test]
    fn parse_srt_basic() {
        let s = "1\n00:00:00,000 --> 00:00:01,500\nhello\n\n2\n00:00:02,000 --> 00:00:04,000\nworld\n";
        let r = parse_srt(s);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].start_ms, 0);
        assert_eq!(r[0].end_ms, 1500);
        assert_eq!(r[0].text, "hello");
        assert_eq!(r[1].start_ms, 2000);
        assert_eq!(r[1].text, "world");
    }

    #[test]
    fn parse_srt_multiline_strips_tags() {
        let s = "1\n00:00:00,000 --> 00:00:02,000\n<i>hello</i>\nworld\n";
        let r = parse_srt(s);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].text, "hello world");
    }

    #[test]
    fn parse_srt_skips_invalid_end_and_empty_text() {
        assert!(parse_srt("1\n00:00:02,000 --> 00:00:01,000\nbad\n").is_empty());
        assert!(parse_srt("1\n00:00:00,000 --> 00:00:01,000\n\n").is_empty());
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `"$HOME/.cargo/bin/cargo" test -p asplayer-transcribe subtitle_import`
Expected: 编译错误 —— `subtitle_import` 模块未找到 / 未声明（因为还没 `pub mod`）。
> 注：模块声明在 Step 1 已加，故这里已经能编译并运行；若 whisper.cpp 编译报错，先按 README 加载 m0 环境（LIBCLANG_PATH / CXXFLAGS）。

- [ ] **Step 3: 运行测试确认通过**

Run: `"$HOME/.cargo/bin/cargo" test -p asplayer-transcribe subtitle_import`
Expected: `parse_srt_basic`、`parse_srt_multiline_strips_tags`、`parse_srt_skips_invalid_end_and_empty_text`、`parse_timestamp_mm_ss_and_hh_mm_ss_ok` 全部 PASS。

- [ ] **Step 4: 提交**

```bash
git add crates/asplayer-transcribe/Cargo.toml crates/asplayer-transcribe/src/lib.rs crates/asplayer-transcribe/src/subtitle_import.rs
git commit -m "feat(transcribe): subtitle_import 解析 SRT（含时间戳助手/HTML 剥离/无效段过滤）"
```

---

### Task 2: crate — `parse_vtt`

**Files:**
- Modify: `crates/asplayer-transcribe/src/subtitle_import.rs`
- Test: `crates/asplayer-transcribe/src/subtitle_import.rs`

- [ ] **Step 1: 写失败测试**

在 `subtitle_import.rs` 的 `mod tests` 内追加：
```rust
    #[test]
    fn parse_vtt_basic_with_settings() {
        let s = "WEBVTT\n\n00:00:00.000 --> 00:00:01.500\nhello\n\n00:00:02.000 --> 00:00:03.000 align:start\nworld\n";
        let r = parse_vtt(s);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].text, "hello");
        assert_eq!(r[1].text, "world");
        assert_eq!(r[1].start_ms, 2000);
    }

    #[test]
    fn parse_vtt_ignores_note_style_region() {
        let s = "WEBVTT\n\nNOTE\nthis is a note line\n\nSTYLE\n::cue { color: red }\n\n00:00:00.000 --> 00:00:01.000\ntext here\n";
        let r = parse_vtt(s);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].text, "text here");
    }
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `"$HOME/.cargo/bin/cargo" test -p asplayer-transcribe subtitle_import::tests::parse_vtt`
Expected: 编译失败 —— `parse_vtt` 未定义。

- [ ] **Step 3: 实现 `parse_vtt`**

在 `sort_by_start` 定义之后追加 `parse_vtt` 与 `parse_cue_timeline`：
```rust
/// VTT 时间行：`start --> end[ settings]`（settings 丢弃）。
fn parse_cue_timeline(line: &str) -> Option<(u64, u64)> {
    let mut it = line.split("-->");
    let start = parse_timestamp(it.next()?)?;
    let end_part = it.next()?.split_whitespace().next()?;
    let end = parse_timestamp(end_part)?;
    Some((start, end))
}

/// 解析 VTT 文本 → 段序列（跳过 WEBVTT 头 / NOTE / STYLE / REGION，丢弃 cue settings）。
pub fn parse_vtt(input: &str) -> Vec<Segment> {
    let normalized = input.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("NOTE") || line.starts_with("STYLE") || line.starts_with("REGION") {
            i += 1;
            while i < lines.len() && !lines[i].trim().is_empty() {
                i += 1;
            }
            continue;
        }
        if line.contains("-->") {
            if let Some((start, end)) = parse_cue_timeline(line) {
                if end > start {
                    let mut text_lines = Vec::new();
                    i += 1;
                    while i < lines.len() && !lines[i].trim().is_empty() {
                        text_lines.push(lines[i]);
                        i += 1;
                    }
                    let text = cleanup_text(&text_lines.join("\n"));
                    if !text.is_empty() {
                        out.push(Segment { start_ms: start, end_ms: end, text });
                    }
                    continue;
                }
            }
        }
        i += 1;
    }
    sort_by_start(&mut out);
    out
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `"$HOME/.cargo/bin/cargo" test -p asplayer-transcribe subtitle_import::tests::parse_vtt`
Expected: 两个 `parse_vtt*` 测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add crates/asplayer-transcribe/src/subtitle_import.rs
git commit -m "feat(transcribe): subtitle_import 解析 VTT（跳过头/NOTE/STYLE，丢弃 cue settings）"
```

---

### Task 3: crate — `parse_subtitle_file` 与字符集处理

**Files:**
- Modify: `crates/asplayer-transcribe/src/subtitle_import.rs`
- Test: `crates/asplayer-transcribe/src/subtitle_import.rs`

- [ ] **Step 1: 写失败测试**

在 `mod tests` 内追加（需 `use std::path::Path;` 已在文件顶部）：
```rust
    #[test]
    fn parse_subtitle_file_gbk() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("t.srt");
        let mut bytes = b"1\n00:00:00,000 --> 00:00:01,000\n".to_vec();
        bytes.extend_from_slice(&[0xC4, 0xE3, 0xBA, 0xC3]); // "你好" 的 GBK
        std::fs::write(&p, bytes)?;
        let segs = parse_subtitle_file(&p)?;
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "你好");
        Ok(())
    }

    #[test]
    fn parse_subtitle_file_utf16le_bom() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("t.srt");
        let text = "1\n00:00:00,000 --> 00:00:01,000\nhi\n";
        let mut bytes = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        std::fs::write(&p, bytes)?;
        let segs = parse_subtitle_file(&p)?;
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "hi");
        Ok(())
    }

    #[test]
    fn parse_subtitle_file_unknown_ext_rejected() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("t.txt");
        std::fs::write(&p, "1\n00:00:00,000 --> 00:00:01,000\nhi\n")?;
        assert!(parse_subtitle_file(&p).is_err());
        Ok(())
    }

    #[test]
    fn parse_subtitle_file_missing_file_errors() {
        assert!(parse_subtitle_file(Path::new("Z:/nope/nothing.srt")).is_err());
    }
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `"$HOME/.cargo/bin/cargo" test -p asplayer-transcribe subtitle_import::tests::parse_subtitle_file`
Expected: 编译失败 —— `parse_subtitle_file` 未定义。

- [ ] **Step 3: 实现解析分派 + 字符集解码**

在文件顶部把 `use std::path::Path;` 改为 `use std::path::Path;`（保留）并新增：`use anyhow::{bail, Context, Result};`。在 `parse_vtt` 定义后追加：
```rust
/// 按扩展名分派解析文件；含字符集处理（BOM → UTF-8 → GBK 回退）。
pub fn parse_subtitle_file(path: &Path) -> Result<Vec<Segment>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let bytes = std::fs::read(path).with_context(|| format!("读取字幕文件失败: {}", path.display()))?;
    let text = decode_subtitle_bytes(&bytes)?;
    match ext.as_str() {
        "srt" => Ok(parse_srt(&text)),
        "vtt" => Ok(parse_vtt(&text)),
        other => bail!("不支持的字幕格式: .{other}（支持 srt / vtt）"),
    }
}

fn decode_subtitle_bytes(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Ok(String::from_utf8_lossy(&bytes[3..]).into_owned());
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let conv: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return Ok(String::from_utf16_lossy(&conv));
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let conv: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return Ok(String::from_utf16_lossy(&conv));
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => {
            let (decoded, _, _) = encoding_rs::GBK.decode(bytes);
            Ok(decoded.into_owned())
        }
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `"$HOME/.cargo/bin/cargo" test -p asplayer-transcribe subtitle_import`
Expected: 全部 PASS（含 `parse_subtitle_file_gbk` / `_utf16le_bom` / `_unknown_ext_rejected` / `_missing_file_errors`）。

- [ ] **Step 5: 提交**

```bash
git add crates/asplayer-transcribe/src/subtitle_import.rs
git commit -m "feat(transcribe): parse_subtitle_file 按扩展名分派 + BOM/UTF-8/GBK 字符集处理"
```

---

### Task 4: db — `replace_subtitles`

**Files:**
- Modify: `app/src-tauri/src/db.rs`
- Test: `app/src-tauri/src/db.rs`

- [ ] **Step 1: 写失败测试**

在 `db.rs` 的 `#[cfg(test)] mod tests` 内追加（放在 `subtitle_status_roundtrip` 这类测试之后）：
```rust
    #[test]
    fn replace_subtitles_replaces_and_marks_done() -> rusqlite::Result<()> {
        use asplayer_transcribe::srt::Segment;
        let db = MediaDb::open_in_memory()?;
        let id = db.upsert_media("D:/m/a.mp4", "a", "video", 0)?;
        db.save_subtitle(id, 0, 1000, "old", "", 0)?;
        db.set_transcribe_next_ms(id, 2500)?;
        let segs = vec![
            Segment { start_ms: 0, end_ms: 1500, text: "hello".into() },
            Segment { start_ms: 2000, end_ms: 4000, text: "world".into() },
        ];
        db.replace_subtitles(id, &segs)?;
        let rows = db.get_subtitles(id)?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].text, "hello");
        assert_eq!(rows[1].text, "world");
        let (status, lang) = db.get_subtitle_status(id)?;
        assert_eq!(status, "done");
        assert_eq!(lang, "");
        assert_eq!(db.get_transcribe_next_ms(id)?, 0);
        // 幂等重复调用不叠加
        db.replace_subtitles(id, &segs)?;
        assert_eq!(db.get_subtitles(id)?.len(), 2);
        Ok(())
    }
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `"$HOME/.cargo/bin/cargo" test -p app replace_subtitles`
Expected: 编译失败 —— `replace_subtitles` 未定义。

- [ ] **Step 3: 实现 `replace_subtitles`**

在 `db.rs` 的 `set_subtitle_status` 之前（`get_subtitles` 之后附近）插入方法体：
```rust
    /// 事务内替换某媒体的全部字幕：clear + upsert + 置 done + 转写断点清零。
    /// 导入的外部字幕无译文（translation 置空）；断点清零防「从断点继续」污染。
    pub fn replace_subtitles(
        &self,
        media_id: i64,
        segs: &[asplayer_transcribe::srt::Segment],
    ) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM subtitles WHERE media_id = ?1", rusqlite::params![media_id])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO subtitles (media_id, start_ms, end_ms, text, translation, ordinal)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT DO UPDATE SET end_ms = excluded.end_ms,
                                           text = excluded.text,
                                           translation = CASE WHEN excluded.translation = '' THEN subtitles.translation ELSE excluded.translation END,
                                           ordinal = excluded.ordinal",
            )?;
            for (i, seg) in segs.iter().enumerate() {
                stmt.execute(rusqlite::params![media_id, seg.start_ms as i64, seg.end_ms as i64, seg.text, "", i as i64])?;
            }
        }
        tx.execute(
            "UPDATE media_files SET subtitle_status = 'done', subtitle_lang = '' WHERE id = ?1",
            rusqlite::params![media_id],
        )?;
        tx.execute(
            "UPDATE media_files SET transcribe_next_ms = 0 WHERE id = ?1",
            rusqlite::params![media_id],
        )?;
        tx.commit()
    }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `"$HOME/.cargo/bin/cargo" test -p app replace_subtitles`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add app/src-tauri/src/db.rs
git commit -m "feat(db): 新增 replace_subtitles 事务替换字幕并置 done + 断点清零"
```

---

### Task 5: media — 同名字幕探测

**Files:**
- Modify: `app/src-tauri/src/media.rs`
- Test: `app/src-tauri/src/media.rs`

- [ ] **Step 1: 写失败测试**

在 `media.rs` 的 `#[cfg(test)] mod tests` 内追加：
```rust
    #[test]
    fn find_sibling_prefers_srt_over_vtt() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join("a.mp4"), b"x")?;
        fs::write(dir.path().join("a.srt"), b"1")?;
        fs::write(dir.path().join("a.vtt"), b"WEBVTT")?;
        let got = find_sibling_subtitle(&dir.path().join("a.mp4")).expect("should find sibling");
        assert_eq!(got, dir.path().join("a.srt"));
        Ok(())
    }

    #[test]
    fn find_sibling_falls_back_to_vtt_and_none() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join("b.mp4"), b"x")?;
        fs::write(dir.path().join("b.vtt"), b"WEBVTT")?;
        let got = find_sibling_subtitle(&dir.path().join("b.mp4")).expect("vtt sibling");
        assert_eq!(got, dir.path().join("b.vtt"));
        assert_eq!(find_sibling_subtitle(&dir.path().join("b.mp4")), Some(dir.path().join("b.vtt")));
        assert_eq!(find_sibling_subtitle(&dir.path().join("no-suchfile.mp4")), None);
        Ok(())
    }
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `"$HOME/.cargo/bin/cargo" test -p app find_sibling`
Expected: 编译失败 —— `find_sibling_subtitle` 未定义。

- [ ] **Step 3: 实现 `find_sibling_subtitle`**

在 `scan_media_files` 定义之后追加：
```rust
/// 在媒体同目录找同名 `.srt`/`.vtt` 字幕（存在 `.srt` 则优先）。找不到返回 None。
pub fn find_sibling_subtitle(media_path: &Path) -> Option<PathBuf> {
    let stem = media_path.file_stem()?.to_str()?;
    let dir = media_path.parent()?;
    for ext in ["srt", "vtt"] {
        let cand = dir.join(format!("{stem}.{ext}"));
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `"$HOME/.cargo/bin/cargo" test -p app find_sibling`
Expected: 两个测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add app/src-tauri/src/media.rs
git commit -m "feat(media): find_sibling_subtitle 同名 srt/vtt 探测（srt 优先）"
```

---

### Task 6: lib — `import_external_subtitle` 命令

**Files:**
- Modify: `app/src-tauri/src/lib.rs`
- Test: `app/src-tauri/src/lib.rs`

- [ ] **Step 1: 写失败测试**

在 `lib.rs` 末尾（`run()` 之后）追加：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn import_external_parses_and_replaces() -> anyhow::Result<()> {
        let db = MediaDb::open_in_memory()?;
        let id = db.upsert_media("D:/m/a.mp4", "a", "video", 0)?;
        db.save_subtitle(id, 0, 1000, "old", "", 0)?;
        db.set_transcribe_next_ms(id, 2500)?;

        let dir = tempfile::tempdir()?;
        let srt = dir.path().join("a.srt");
        let mut f = std::fs::File::create(&srt)?;
        writeln!(f, "1")?;
        writeln!(f, "00:00:00,000 --> 00:00:01,500")?;
        writeln!(f, "hello")?;
        writeln!(f)?;
        writeln!(f, "2")?;
        writeln!(f, "00:00:02,000 --> 00:00:03,000")?;
        writeln!(f, "world")?;

        let n = import_external(&db, id, &srt)?;
        assert_eq!(n, 2);
        let rows = db.get_subtitles(id)?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].text, "hello");
        assert_eq!(rows[1].text, "world");
        let (status, _) = db.get_subtitle_status(id)?;
        assert_eq!(status, "done");
        assert_eq!(db.get_transcribe_next_ms(id)?, 0);
        Ok(())
    }

    #[test]
    fn import_external_empty_file_rejected() -> anyhow::Result<()> {
        let db = MediaDb::open_in_memory()?;
        let id = db.upsert_media("D:/m/a.mp4", "a", "video", 0)?;
        let dir = tempfile::tempdir()?;
        let srt = dir.path().join("a.srt");
        std::fs::write(&srt, "")?;
        assert!(import_external(&db, id, &srt).is_err());
        Ok(())
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `"$HOME/.cargo/bin/cargo" test -p app import_external`
Expected: 编译失败 —— `import_external` 未定义，且 `Path` 未导入。

- [ ] **Step 3: 实现 `import_external` + 命令 + 注册**

在 `lib.rs` 顶部把 `use std::path::PathBuf;` 改为：
```rust
use std::path::{Path, PathBuf};
```

在 `cancel_transcribe` 命令之后追加辅助函数与命令：
```rust
/// 执行外部字幕导入：解析 path → 替换 media 字幕（置 done、断点清零）→ 返回段数。
fn import_external(db: &MediaDb, media_id: i64, path: &Path) -> anyhow::Result<usize> {
    let segs = asplayer_transcribe::subtitle_import::parse_subtitle_file(path)?;
    if segs.is_empty() {
        anyhow::bail!("未解析到任何字幕");
    }
    db.replace_subtitles(media_id, &segs)
        .map_err(|e| anyhow::anyhow!("写入字幕失败: {e}"))?;
    Ok(segs.len())
}

/// 导入外部字幕：path=None 时按同名自动检测；成功返回段数。
#[tauri::command]
fn import_external_subtitle(
    media_id: i64,
    path: Option<String>,
    state: State<AppState>,
) -> CmdResult<usize> {
    let db = state.db.lock().map_err(err_str)?;
    let (media_path, _title) = db.media_path(media_id).map_err(err_str)?;
    let resolved = match path {
        Some(p) => PathBuf::from(p),
        None => media::find_sibling_subtitle(Path::new(&media_path))
            .ok_or_else(|| "未找到同名字幕文件，请手动选择".to_string())?,
    };
    import_external(&db, media_id, &resolved).map_err(err_str)
}
```

在 `.invoke_handler(tauri::generate_handler![ ... ])` 的列表里（`get_subtitle_status` 之后）加一行：
```rust
            import_external_subtitle,
```

- [ ] **Step 4: 运行测试确认通过**

Run: `"$HOME/.cargo/bin/cargo" test -p app import_external`
Expected: `import_external_parses_and_replaces` 与 `import_external_empty_file_rejected` 均 PASS。

- [ ] **Step 5: 提交**

```bash
git add app/src-tauri/src/lib.rs
git commit -m "feat(tauri): import_external_subtitle 命令（手动选文件/同名自动）+ import_external 辅助"
```

---

### Task 7: 前端 API + 入口 + 接线

**Files:**
- Modify: `app/src/api/subtitle.ts`
- Modify: `app/src/App.vue`
- Modify: `app/src/components/PlayerStage.vue`
- Modify: `app/src/components/PlaylistPanel.vue`

- [ ] **Step 1: 加 API**

`app/src/api/subtitle.ts` 的 `cancelTranscribe` 之后追加：
```ts
export function importExternalSubtitle(mediaId: number, path?: string): Promise<number> {
  return invoke<number>("import_external_subtitle", {
    mediaId,
    ...(path ? { path } : {}),
  });
}
```

- [ ] **Step 2: App.vue 加 `onImportSubtitle`**

`App.vue` 的现有导入块（第 18-25 行）把 `cancelTranscribe, translateMedia` 等一起引入，追加 `getSubtitleStatus` 与 `importExternalSubtitle`：
```ts
import {
  onTranscribeProgress,
  onTranscribeDone,
  onTranscribeError,
  onTranscribeCanceled,
  cancelTranscribe,
  translateMedia,
  getSubtitleStatus,
  importExternalSubtitle,
} from "./api/subtitle";
```

在 `importFiles` 函数之后追加：
```ts
// 导入外部字幕：选文件 → 有旧字幕先确认 → 替换 → 刷新当前媒体
async function onImportSubtitle(mediaId: number) {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const sel = await open({
    multiple: false,
    filters: [{ name: "字幕文件", extensions: ["srt", "vtt"] }],
  });
  if (!sel) return;
  const path = Array.isArray(sel) ? sel[0] : sel;
  const [st] = await getSubtitleStatus(mediaId).catch(() => ["none" as string, "" as string]);
  if (st !== "none") {
    const ok = window.confirm("导入将替换该媒体现有的字幕，继续？");
    if (!ok) return;
  }
  try {
    const count = await importExternalSubtitle(mediaId, path);
    // eslint-disable-next-line no-console
    console.log("[ASPlayer] 导入字幕:", count, "段");
    if (sub.currentId.value === mediaId) {
      sub.setStatus("done", "done", 100, "");
      sub.load(mediaId);
    }
    refresh().catch(() => {});
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[ASPlayer] 导入字幕失败:", e);
    sub.setStatus("error", "", 0, String(e));
  }
}
```

模板接线：PlayerStage 加 `@import-subtitle="onImportSubtitle"`；PlaylistPanel 加 `@import-subtitle="(item) => onImportSubtitle(item.id)"`：
```html
    <PlayerStage
      ...
      @import-subtitle="onImportSubtitle"
      ...
    />
    ...
    <PlaylistPanel
      ...
      @import-subtitle="(item) => onImportSubtitle(item.id)"
      ...
    />
```

- [ ] **Step 3: PlayerStage 加按钮**

`defineEmits` 里追加 `importSubtitle: [id: number];`，并在 `doTranslate` 函数之后加：
```ts
function doImportSubtitle() {
  if (props.item) emit("importSubtitle", props.item.id);
}
```
工具栏 `delete` 的按钮组（第 390 行「转写」按钮之后）插入：
```html
        <button class="iconbtn" title="导入字幕" @click="doImportSubtitle" :disabled="!item"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="stroke:var(--fg-2)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5h5l1 1h10v13H4z"/><path d="M12 9v5m0 0-2-2m2 2 2-2"/></svg></button>
```

- [ ] **Step 4: PlaylistPanel 加右键项**

`defineEmits` 里追加 `importSubtitle: [item: MediaItem];`，并加：
```ts
function importSubtitle(item: MediaItem) {
  closeCtx();
  emit("importSubtitle", item);
}
```
右键菜单（第 238-244 行「播放」之后）插入：
```html
        <button class="pl-ctx-item" @click="importSubtitle(ctxMenu!.item)">
          导入字幕
        </button>
```

- [ ] **Step 5: 类型检查 + 构建**

Run: `cd app && export PATH="/c/Program Files/nodejs:$PATH" && npx vue-tsc --noEmit`
Expected: 无输出（0 错误）。

Run: `cd app && export PATH="/c/Program Files/nodejs:$PATH" && npm run build`
Expected: `✓ built in ...`（仅既有动态导入警告）。

- [ ] **Step 6: 提交**

```bash
git add app/src/api/subtitle.ts app/src/App.vue app/src/components/PlayerStage.vue app/src/components/PlaylistPanel.vue
git commit -m "feat(ui): 外部字幕导入入口（工具栏/右键）+ onImportSubtitle 接线与确认替换"
```

---

## 自检清单

**1. Spec 覆盖**
- §4.1 解析（SRT/VTT/字符集）→ Task 1-3 ✔
- §4.2 `replace_subtitles` → Task 4 ✔
- §4.3 `import_external_subtitle` 命令 + 同名检测 + 注册 → Task 5-6 ✔
- §5.1 API → Task 7 Step 1 ✔
- §5.2 App.vue `onImportSubtitle`（确认+load+refresh）→ Task 7 Step 2 ✔
- §5.3 工具栏按钮 → Task 7 Step 3 ✔
- §5.4 右键菜单 → Task 7 Step 4 ✔
- §5.5 同名自动（命令层 `path=None`）→ Task 6 ✔
- §6 边界（0 段报错、断点清零、冲突确认）→ Task 6 测试 / 前端确认 ✔

**2. 占位符扫描**：无 TBD/TODO；每个代码步都有完整实现或测试代码。

**3. 类型一致性**
- `Segment{start_ms:u64,end_ms:u64,text}`（crate）→ `replace_subtitles` 收 `&[asplayer_transcribe::srt::Segment]`；写库 `seg.start_ms as i64` ✔
- 命令参数 JS 侧 `mediaId`/`path` ↦ Rust `media_id`/`path`（Tauri v2 自动 camelCase→snake_case，参照既有 `save_playback_position`/`positionMs`）✔
- 前端 `importExternalSubtitle` 返回 `Promise<number>`（段数），App 用 `count` ✔
- 事件名两端一致：`importSubtitle`（PlayerStage 传 `number id`）/ PlaylistPanel 传 `MediaItem`，App 分别用 `onImportSubtitle(id)` 与 `(item)=>onImportSubtitle(item.id)` ✔

**注**：命令 `import_external_subtitle` 直接经 `MediaDb`（`State<AppState>`）取路径，未在单测中直接调用（需要 `State`，不可在纯 Rust 测试构造）；其核心逻辑 `import_external` + `replace_subtitles` + `find_sibling_subtitle` 均已单测覆盖，命令层做的是薄包装，靠手动验收确认。
