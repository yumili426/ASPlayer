/// 一条字幕段。start/end 用毫秒。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// 毫秒 → "HH:MM:SS,mmm"
pub fn format_timestamp(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    let msec = ms % 1000;
    format!("{h:02}:{m:02}:{s:02},{msec:03}")
}

/// 渲染整份 SRT 文本
pub fn render_srt(segments: &[Segment]) -> String {
    let mut out = String::new();
    for (i, seg) in segments.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            format_timestamp(seg.start_ms),
            format_timestamp(seg.end_ms),
            seg.text.trim()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_zero() {
        assert_eq!(format_timestamp(0), "00:00:00,000");
    }

    #[test]
    fn timestamp_with_hours_and_ms() {
        assert_eq!(format_timestamp(3_661_001), "01:01:01,001");
        assert_eq!(format_timestamp(59_999), "00:00:59,999");
    }

    #[test]
    fn render_numbered_blocks() {
        let segs = vec![
            Segment { start_ms: 0, end_ms: 1500, text: " おやすみ ".into() },
            Segment { start_ms: 2000, end_ms: 4000, text: "good night".into() },
        ];
        let s = render_srt(&segs);
        assert_eq!(
            s,
            "1\n00:00:00,000 --> 00:00:01,500\nおやすみ\n\n\
             2\n00:00:02,000 --> 00:00:04,000\ngood night\n\n"
        );
    }
}
