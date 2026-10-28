use std::path::Path;

use crate::transcriber::SubtitleRow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Srt,
    Vtt,
    Txt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentMode {
    Original,
    Translation,
    Bilingual,
}

impl ExportFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "srt" => Some(Self::Srt),
            "vtt" => Some(Self::Vtt),
            "txt" => Some(Self::Txt),
            _ => None,
        }
    }
}

impl ContentMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "original" => Some(Self::Original),
            "translation" => Some(Self::Translation),
            "bilingual" => Some(Self::Bilingual),
            _ => None,
        }
    }
}

fn content_lines<'a>(row: &'a SubtitleRow, content: ContentMode) -> Vec<&'a str> {
    let text = row.text.trim();
    let translation = row.translation.trim();
    match content {
        ContentMode::Original => {
            if text.is_empty() {
                vec![]
            } else {
                vec![text]
            }
        }
        ContentMode::Translation => {
            if translation.is_empty() {
                vec![]
            } else {
                vec![translation]
            }
        }
        ContentMode::Bilingual => {
            let mut lines = Vec::with_capacity(2);
            if !text.is_empty() {
                lines.push(text);
            }
            if !translation.is_empty() {
                lines.push(translation);
            }
            lines
        }
    }
}

fn fmt_time(ms: i64, comma: bool) -> String {
    let ms = ms.max(0);
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!(
        "{h:02}:{m:02}:{s:02}{}{millis:03}",
        if comma { ',' } else { '.' }
    )
}

pub fn count(rows: &[SubtitleRow], content: ContentMode) -> usize {
    rows.iter()
        .filter(|row| !content_lines(row, content).is_empty())
        .count()
}

pub fn render(rows: &[SubtitleRow], format: ExportFormat, content: ContentMode) -> String {
    if format == ExportFormat::Txt {
        let blocks: Vec<String> = rows
            .iter()
            .map(|row| content_lines(row, content).join("\n"))
            .filter(|block| !block.is_empty())
            .collect();
        return if blocks.is_empty() {
            String::new()
        } else {
            format!("{}\n", blocks.join("\n\n"))
        };
    }

    let mut out = String::new();
    if format == ExportFormat::Vtt {
        out.push_str("WEBVTT\n\n");
    }
    let mut index = 0;
    for row in rows {
        let lines = content_lines(row, content);
        if lines.is_empty() {
            continue;
        }
        index += 1;
        if format == ExportFormat::Srt {
            out.push_str(&format!("{index}\n"));
        }
        out.push_str(&format!(
            "{} --> {}\n{}\n\n",
            fmt_time(row.start_ms, format == ExportFormat::Srt),
            fmt_time(row.end_ms, format == ExportFormat::Srt),
            lines.join("\n")
        ));
    }
    out
}

pub fn write(
    rows: &[SubtitleRow],
    path: &Path,
    format: ExportFormat,
    content: ContentMode,
) -> Result<usize, String> {
    if rows.is_empty() {
        return Err("没有可导出的字幕".into());
    }
    let exported = count(rows, content);
    if exported == 0 {
        return Err(if content == ContentMode::Translation {
            "没有译文可导出".into()
        } else {
            "没有可导出的字幕".into()
        });
    }
    std::fs::write(path, render(rows, format, content))
        .map_err(|e| format!("写入文件失败: {e}"))?;
    Ok(exported)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(start: i64, end: i64, text: &str, translation: &str) -> SubtitleRow {
        SubtitleRow {
            start_ms: start,
            end_ms: end,
            text: text.into(),
            translation: translation.into(),
            ordinal: 0,
        }
    }

    fn sample() -> Vec<SubtitleRow> {
        vec![
            row(0, 1500, "おやすみ", "晚安"),
            row(2000, 4000, "good night", ""),
        ]
    }

    #[test]
    fn parse_accepts_known_values_only() {
        assert_eq!(ExportFormat::parse("srt"), Some(ExportFormat::Srt));
        assert_eq!(ExportFormat::parse("vtt"), Some(ExportFormat::Vtt));
        assert_eq!(ExportFormat::parse("txt"), Some(ExportFormat::Txt));
        assert_eq!(ExportFormat::parse("ass"), None);
        assert_eq!(ContentMode::parse("original"), Some(ContentMode::Original));
        assert_eq!(ContentMode::parse("translation"), Some(ContentMode::Translation));
        assert_eq!(ContentMode::parse("bilingual"), Some(ContentMode::Bilingual));
        assert_eq!(ContentMode::parse("bilingual2"), None);
    }

    #[test]
    fn srt_numbers_blocks_and_renders_bilingual() {
        assert_eq!(
            render(&sample(), ExportFormat::Srt, ContentMode::Bilingual),
            "1\n00:00:00,000 --> 00:00:01,500\nおやすみ\n晚安\n\n\
             2\n00:00:02,000 --> 00:00:04,000\ngood night\n\n"
        );
    }

    #[test]
    fn vtt_has_header_and_dot_timestamps() {
        assert_eq!(
            render(&sample(), ExportFormat::Vtt, ContentMode::Original),
            "WEBVTT\n\n00:00:00.000 --> 00:00:01.500\nおやすみ\n\n\
             00:00:02.000 --> 00:00:04.000\ngood night\n\n"
        );
    }

    #[test]
    fn txt_separates_segments_with_blank_line() {
        assert_eq!(
            render(&sample(), ExportFormat::Txt, ContentMode::Bilingual),
            "おやすみ\n晚安\n\ngood night\n"
        );
    }

    #[test]
    fn translation_mode_skips_missing_translation() {
        assert_eq!(count(&sample(), ContentMode::Translation), 1);
        assert_eq!(
            render(&sample(), ExportFormat::Txt, ContentMode::Translation),
            "晚安\n"
        );
    }

    #[test]
    fn empty_rows_render_minimal_output() {
        assert_eq!(render(&[], ExportFormat::Srt, ContentMode::Bilingual), "");
        assert_eq!(render(&[], ExportFormat::Vtt, ContentMode::Bilingual), "WEBVTT\n\n");
        assert_eq!(render(&[], ExportFormat::Txt, ContentMode::Bilingual), "");
    }

    #[test]
    fn write_reports_count_and_creates_file() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("out.txt");
        let n = write(&sample(), &path, ExportFormat::Txt, ContentMode::Bilingual).unwrap();
        assert_eq!(n, 2);
        assert_eq!(
            std::fs::read_to_string(&path)?,
            "おやすみ\n晚安\n\ngood night\n"
        );
        Ok(())
    }

    #[test]
    fn write_rejects_empty_rows_and_empty_translation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.srt");
        assert_eq!(
            write(&[], &path, ExportFormat::Srt, ContentMode::Bilingual).unwrap_err(),
            "没有可导出的字幕"
        );
        let rows = vec![row(0, 1000, "hi", "")];
        assert_eq!(
            write(&rows, &path, ExportFormat::Srt, ContentMode::Translation).unwrap_err(),
            "没有译文可导出"
        );
    }
}
