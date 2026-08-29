use crate::srt::Segment;
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
