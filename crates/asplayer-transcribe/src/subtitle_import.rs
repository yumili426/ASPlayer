use crate::srt::Segment;
use anyhow::{bail, Context, Result};
use std::path::Path;

/// MM:SS / HH:MM:SS + 毫秒（分隔符兼容 , 或 .）。返回毫秒。小时可省略。
fn parse_timestamp(ts: &str) -> Option<u64> {
    let ts = ts.trim();
    let (int_part, frac) = match ts.find([',', '.']) {
        Some(i) => (&ts[..i], ts[i + 1..].trim()),
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
        .map(|p| p.trim().parse::<u64>())
        .collect::<Result<_, _>>()
        .ok()?;
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
}
