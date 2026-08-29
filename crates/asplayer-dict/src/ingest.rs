use crate::types::{EnEntry, JaEntry};
use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::Reader;

/// ECDICT 有效行所需的最小列数：word, phonetic, definition, translation, pos。
/// 列数不足的残缺行视为垃圾数据，宽容跳过。
const MIN_COLS: usize = 5;

/// 解析 ECDICT CSV。
/// - 第一行若为表头（首个字段为 "word"）则跳过。
/// - 列数不足（< MIN_COLS）或 word 为空的行宽容跳过。
///
/// CSV 列序（0-based）：0 word, 1 phonetic, 2 definition, 3 translation, 4 pos,
/// 5 collins, 6 oxford, 7 tag, 8 bnc, 9 frq, 10 exchange, 11 detail, 12 audio
pub fn parse_en_csv(header: &str, data: &str) -> Result<Vec<EnEntry>> {
    let _ = header; // 为真实调用保留签名；本实现靠首记录内容判定表头
    let mut out = Vec::new();
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(data.as_bytes());

    let is_header = |rec: &csv::StringRecord| rec.get(0).map_or(false, |c| c == "word");

    for (i, rec) in rdr.records().enumerate() {
        let rec = rec.context("csv record")?;
        if i == 0 && is_header(&rec) {
            continue;
        }
        // 残缺行（字段数不足）宽容跳过
        if rec.len() < MIN_COLS {
            continue;
        }
        let word = rec.get(0).unwrap_or("").trim().to_string();
        if word.is_empty() {
            continue;
        }
        out.push(EnEntry {
            word,
            phonetic: rec.get(1).unwrap_or("").to_string(),
            definition: rec.get(2).unwrap_or("").to_string(),
            translation: rec.get(3).unwrap_or("").to_string(),
            pos: rec.get(4).unwrap_or("").to_string(),
            exchange: rec.get(10).unwrap_or("").to_string(),
        });
    }
    Ok(out)
}

/// 解析 JMdict XML。每个 <entry> 取首个 <keb>、<reb>，聚合 <sense> 下所有 <gloss>（以 " / " 连接）。
pub fn parse_jm_dict(xml: &str) -> Result<Vec<JaEntry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut out = Vec::new();

    let mut cur: JaEntry = JaEntry::default();
    let mut in_entry = false;
    let mut in_gloss = false;
    let mut gloss_parts: Vec<String> = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"entry" => {
                    cur = JaEntry::default();
                    gloss_parts.clear();
                    in_entry = true;
                }
                b"keb" if cur.surface.is_empty() => cur.surface = read_text(&mut reader, e.name().as_ref())?,
                b"reb" if cur.reading.is_empty() => cur.reading = read_text(&mut reader, e.name().as_ref())?,
                b"gloss" if in_entry => in_gloss = true,
                _ => {}
            },
            Ok(Event::Text(t)) if in_gloss => {
                gloss_parts.push(t.unescape().map(|s| s.into_owned()).unwrap_or_default());
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"gloss" => in_gloss = false,
                b"entry" => {
                    if !cur.surface.is_empty() {
                        cur.gloss = gloss_parts.join(" / ");
                        out.push(cur.clone());
                    }
                    in_entry = false;
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn read_text(reader: &mut Reader<&[u8]>, name: &[u8]) -> Result<String> {
    let mut text = String::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Text(t)) => text.push_str(&t.unescape().map(|s| s.into_owned()).unwrap_or_default()),
            Ok(Event::End(e)) if e.name().as_ref() == name => break,
            _ => break,
        }
        buf.clear();
    }
    Ok(text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ecdict_csv_header_and_row() {
        let header = "word,phonetic,definition,translation,pos,collins,oxford,tag,bnc,frq,exchange,detail,audio\n";
        // 单行完整行：exchange 位于第 11 列（0-based index 10）
        let row = "run,rʌn,跑；奔跑,赛跑,vi,2,1,3,500,1000,p:ran/i:running/3:runs,,\n";
        let rows = parse_en_csv(header, row).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].word, "run");
        assert_eq!(rows[0].exchange, "p:ran/i:running/3:runs");
        assert!(rows[0].definition.contains("跑"));
    }

    #[test]
    fn skips_bad_rows() {
        let header = "word,phonetic,definition,translation,pos,collins,oxford,tag,bnc,frq,exchange,detail,audio\n";
        // 短行（列数不足）或空 word 的行应被跳过（宽容解析）
        let data = "too,few\nrun,rʌn\n,\n";
        let rows = parse_en_csv(header, data).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn parse_jmdict_sample() -> anyhow::Result<()> {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<JMdict>
  <entry>
    <k_ele><keb>食べる</keb></k_ele>
    <r_ele><reb>たべる</reb></r_ele>
    <sense>
      <pos>v1</pos>
      <gloss>to eat</gloss>
      <gloss>to have a meal</gloss>
    </sense>
  </entry>
</JMdict>"#;
        let entries = parse_jm_dict(xml)?;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].surface, "食べる");
        assert_eq!(entries[0].reading, "たべる");
        assert_eq!(entries[0].gloss, "to eat / to have a meal");
        Ok(())
    }

    #[test]
    fn parse_jmdict_first_wins_for_keb_reb() -> anyhow::Result<()> {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<JMdict>
  <entry>
    <k_ele><keb>食べる</keb></k_ele>
    <k_ele><keb>食う</keb></k_ele>
    <r_ele><reb>たべる</reb></r_ele>
    <r_ele><reb>くう</reb></r_ele>
    <sense>
      <gloss>to eat</gloss>
    </sense>
  </entry>
</JMdict>"#;
        let entries = parse_jm_dict(xml)?;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].surface, "食べる");
        assert_eq!(entries[0].reading, "たべる");
        assert_eq!(entries[0].gloss, "to eat");
        Ok(())
    }
}
