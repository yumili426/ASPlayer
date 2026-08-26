use crate::srt::Segment;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const BATCH_SIZE: usize = 25;

/// 构建一个批次的翻译 prompt（纯函数，可单测）。
/// batch 元素为 (全局序号, 原文)。
pub fn build_prompts(
    batch: &[(usize, &str)],
    context_before: &str,
    target_lang: &str,
) -> (String, String) {
    let system = concat!(
        "You are a professional subtitle translator. Translate each numbered subtitle line ",
        "into the target language naturally and conversationally, as native spoken content. ",
        "Preserve tone (including soft/whispered ASMR style). Use surrounding lines only as context. ",
        "Reply with STRICT JSON only: an object mapping each input index (as string) to the translated string. No extra keys, no commentary."
    );
    let lines: Vec<String> = batch.iter().map(|(i, t)| format!("{i}. {t}")).collect();
    let ctx = if context_before.trim().is_empty() {
        String::new()
    } else {
        format!("[Context before, do not translate]:\n{context_before}\n\n")
    };
    let user = format!(
        "{ctx}[Target language]: {target_lang}\n[Lines]:\n{}\n\nReply JSON now.",
        lines.join("\n")
    );
    (system.to_string(), user)
}

fn call_api(api_base: &str, api_key: &str, model: &str, system: &str, user: &str) -> Result<String> {
    let url = format!("{}/chat/completions", api_base.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "temperature": 0.3,
        "response_format": {"type": "json_object"}
    });
    let resp = reqwest::blocking::Client::new()
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .context("翻译 API 请求失败")?;
    let status = resp.status();
    let v: Value = resp.json().context("翻译 API 返回非 JSON")?;
    if !status.is_success() {
        bail!("翻译 API 错误 {status}: {v}");
    }
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .context("翻译 API 响应缺少 choices[0].message.content")?;
    Ok(content.to_string())
}

/// 解析模型返回的 JSON 映射，容忍代码围栏包裹；缺失的 idx 被容忍。
pub fn parse_mapping(raw: &str, expected_idx: &[usize]) -> Result<HashMap<usize, String>> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let v: Value = serde_json::from_str(cleaned).context("模型返回的不是合法 JSON")?;
    let obj = v.as_object().context("JSON 顶层不是对象")?;
    let mut map = HashMap::new();
    for idx in expected_idx {
        let key = idx.to_string();
        if let Some(s) = obj.get(key.as_str()).and_then(Value::as_str) {
            map.insert(*idx, s.to_string());
        }
    }
    Ok(map)
}

/// 整体翻译：每批 BATCH_SIZE 句、带前 5 句上下文、解析失败自动重试至多 3 次。
pub fn translate_segments(
    segments: &[Segment],
    api_base: &str,
    api_key: &str,
    model: &str,
    target_lang: &str,
) -> Result<HashMap<usize, String>> {
    let mut result = HashMap::new();
    for chunk in segments.chunks(BATCH_SIZE) {
        let start_global = result.len();
        let batch: Vec<(usize, &str)> =
            chunk.iter().enumerate().map(|(i, s)| (start_global + i, s.text.as_str())).collect();
        let from = start_global.saturating_sub(5);
        let before = segments[from..start_global]
            .iter()
            .map(|s| s.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let (sys, usr) = build_prompts(&batch, &before, target_lang);

        let expected: Vec<usize> = batch.iter().map(|(i, _)| *i).collect();
        let mut attempt = 0;
        loop {
            attempt += 1;
            let raw = call_api(api_base, api_key, model, &sys, &usr)?;
            match parse_mapping(&raw, &expected) {
                Ok(m) => {
                    result.extend(m);
                    break;
                }
                Err(e) if attempt < 3 => eprintln!("批次解析失败（第{attempt}次），重试: {e}"),
                Err(e) => return Err(e),
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_contain_indices_and_context() {
        let batch = vec![(7, "おやすみ"), (8, "またね")];
        let (sys, usr) = build_prompts(&batch, "前の文", "Simplified Chinese");
        assert!(sys.contains("STRICT JSON"));
        assert!(usr.contains("[Context before"));
        assert!(usr.contains("7. おやすみ"));
        assert!(usr.contains("8. またね"));
        assert!(usr.contains("Simplified Chinese"));
    }

    #[test]
    fn prompts_empty_context_has_no_marker() {
        let (sys, usr) = build_prompts(&[(0, "hi")], "", "English");
        assert!(sys.contains("STRICT JSON"));
        assert!(!usr.contains("[Context before"));
    }

    #[test]
    fn parse_plain_and_fenced_json() {
        let m = parse_mapping(r#"{"0": "晚安", "1": "再见"}"#, &[0, 1]).unwrap();
        assert_eq!(m[&0], "晚安");
        let fenced = "```json\n{\"5\": \"好的\"}\n```";
        let m2 = parse_mapping(fenced, &[5]).unwrap();
        assert_eq!(m2[&5], "好的");
    }

    #[test]
    fn parse_missing_index_is_tolerated() {
        let m = parse_mapping("{\"0\":\"x\"}", &[0, 1]).unwrap();
        assert!(!m.contains_key(&1));
    }
}
