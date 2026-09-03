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
    // 要求模型把原文原样回显在 `src`（内容锚定）：批内行号 i 偶发漂移时，
    // 后端可以靠 `src` 与真实行文的匹配把译文挂回正确的全局序号，从根上矫正错位。
    // 顶层必须是 JSON 对象 —— 与 call_api 的 response_format:json_object 兼容（数组会触发解析矛盾）。
    let system = concat!(
        "You are a professional subtitle translator. Translate each numbered subtitle line ",
        "into the target language naturally and conversationally, as native spoken content. ",
        "Preserve tone (including soft/whispered ASMR style). Use surrounding lines only as context. ",
        "Reply with STRICT JSON only: a JSON object mapping each input line number (as a string key) ",
        "to an object with two fields: {\"src\": <copy the source line text VERBATIM, without any number prefix>, ",
        "\"t\": <your translation>}. No extra keys, no commentary."
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

/// 去掉行文开头的数字前缀（`12. ` / `12: `）。防御模型回显 src 时把编号一并带回。
fn strip_index_prefix(text: &str) -> &str {
    let t = text.trim_start();
    let bytes = t.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b':') {
        let s = &t[i + 1..];
        if !s.trim().is_empty() {
            return s.trim();
        }
    }
    t
}

/// 归一化行文用于内容匹配：去编号前缀 → 只保留字母数字（含中日韩）→ 小写。
fn normalize_kw(text: &str) -> String {
    strip_index_prefix(text)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn json_usize(v: &Value) -> Option<usize> {
    match v {
        Value::Number(n) => n.as_u64().map(|u| u as usize),
        Value::String(s) => s.trim().parse::<usize>().ok(),
        _ => None,
    }
}

/// 解析模型返回的 JSON，逐行以「回显的 src」内容锚定，矫正批内行号漂移。
///
/// 模型在批内偶发把行号 i 后移/前移（错位根因），但通常会把原文原样回显在 `src`。
/// 本函数以归一化后的 src 与预期行文匹配，命中唯一行时把译文挂到「该行真实全局序号」
/// （而非模型自报的 i），从而矫正漂移；src 缺失/匹配不上/重复时回退到模型自报 i。
/// 顶层既接受数组也接受对象（值为字符串的旧格式按旧行为用 i 当序号），
/// 容忍代码围栏，缺失与空译文的条目被跳过。
pub fn parse_aligned_mapping(
    raw: &str,
    expected: &[(usize, &str)],
) -> Result<HashMap<usize, String>> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let v: Value = serde_json::from_str(cleaned).context("模型返回的不是合法 JSON")?;

    // 归一化的预期行文 → 命中该行的 expected 位置列表（可能有多行同文）
    let mut norm_map: HashMap<String, Vec<usize>> = HashMap::new();
    for (pos, (_, t)) in expected.iter().enumerate() {
        norm_map.entry(normalize_kw(t)).or_default().push(pos);
    }

    struct Item {
        idx: Option<usize>,
        src: Option<String>,
        text: String,
    }
    let mut items: Vec<Item> = Vec::new();

    match &v {
        Value::Array(arr) => {
            for elem in arr {
                let obj = match elem.as_object() {
                    Some(o) => o,
                    None => continue,
                };
                items.push(Item {
                    idx: obj.get("i").and_then(json_usize),
                    src: obj.get("src").and_then(Value::as_str).map(str::to_string),
                    text: obj.get("t").and_then(Value::as_str).unwrap_or("").to_string(),
                });
            }
        }
        Value::Object(obj) => {
            for (k, val) in obj {
                let idx = k.parse::<usize>().ok();
                match val {
                    Value::String(s) => items.push(Item { idx, src: None, text: s.clone() }),
                    Value::Object(inner) => items.push(Item {
                        idx,
                        src: inner.get("src").and_then(Value::as_str).map(str::to_string),
                        text: inner.get("t").and_then(Value::as_str).unwrap_or("").to_string(),
                    }),
                    _ => {}
                }
            }
        }
        _ => bail!("JSON 顶层既不是对象也不是数组"),
    }

    let mut map: HashMap<usize, String> = HashMap::new();
    for item in items {
        let text = item.text.trim();
        if text.is_empty() {
            continue;
        }
        let mut chosen: Option<usize> = None;
        // 首选：src 内容锚定到未被占用的唯一行
        if let Some(src) = &item.src {
            if let Some(pos_list) = norm_map.get(&normalize_kw(src)) {
                let free: Vec<usize> = pos_list
                    .iter()
                    .copied()
                    .filter(|&p| !map.contains_key(&expected[p].0))
                    .collect();
                if free.len() == 1 {
                    chosen = Some(expected[free[0]].0);
                }
            }
        }
        // 回退：模型自报的行号（仅当它对应一条真实预期行且未被占用）
        if chosen.is_none() {
            if let Some(i) = item.idx {
                if expected.iter().any(|(g, _)| *g == i) && !map.contains_key(&i) {
                    chosen = Some(i);
                }
            }
        }
        if let Some(g) = chosen {
            map.insert(g, text.to_string());
        }
    }
    Ok(map)
}

/// 翻译主体：注入 API 调用以便单测。每批 BATCH_SIZE 句、带前 5 句上下文、解析失败自动重试至多 3 次。
fn translate_with<F>(
    segments: &[Segment],
    target_lang: &str,
    mut call: F,
) -> Result<HashMap<usize, String>>
where
    F: FnMut(&str, &str) -> Result<String>,
{
    let mut result = HashMap::new();
    for (chunk_pos, chunk) in segments.chunks(BATCH_SIZE).enumerate() {
        // 批的全局起点必须是该批在完整列表中的绝对偏移，而不能用 result.len()。
        // 一旦前批有 idx 未被模型返回（parse_aligned_mapping 容忍缺失），result.len() 会偏小，
        // 使后续所有批的全局序号整体左移，导致译文与原文错位（每句译文变成下一句的译文）。
        let start_global = chunk_pos * BATCH_SIZE;
        let batch: Vec<(usize, &str)> =
            chunk.iter().enumerate().map(|(i, s)| (start_global + i, s.text.as_str())).collect();
        let from = start_global.saturating_sub(5);
        let before = segments[from..start_global]
            .iter()
            .map(|s| s.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let (sys, usr) = build_prompts(&batch, &before, target_lang);

        let mut attempt = 0;
        loop {
            attempt += 1;
            let raw = call(&sys, &usr)?;
            match parse_aligned_mapping(&raw, &batch) {
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

/// 整体翻译：每批 BATCH_SIZE 句、带前 5 句上下文、解析失败自动重试至多 3 次。
pub fn translate_segments(
    segments: &[Segment],
    api_base: &str,
    api_key: &str,
    model: &str,
    target_lang: &str,
) -> Result<HashMap<usize, String>> {
    translate_with(segments, target_lang, |sys, usr| {
        call_api(api_base, api_key, model, sys, usr)
    })
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
        let m = parse_aligned_mapping(r#"{"0": "晚安", "1": "再见"}"#, &[(0, "hello"), (1, "world")]).unwrap();
        assert_eq!(m[&0], "晚安");
        assert_eq!(m[&1], "再见");
        let fenced = "```json\n{\"5\": \"好的\"}\n```";
        let m2 = parse_aligned_mapping(fenced, &[(5, "ok")]).unwrap();
        assert_eq!(m2[&5], "好的");
    }

    #[test]
    fn parse_missing_index_is_tolerated() {
        let m = parse_aligned_mapping("{\"0\":\"x\"}", &[(0, "a"), (1, "b")]).unwrap();
        assert!(m.contains_key(&0));
        assert!(!m.contains_key(&1));
    }

    /// 核心回归：模型在批内把行号 i 漂移（此处整体后移 1），但原样回显了 src。
    /// parse_aligned_mapping 必须靠 src 内容锚定，把译文挂回真实行，而非模型自报的 i。
    /// 顶层对象键为模型自报的（漂移后的）行号 —— 正是 response_format:json_object 的实际形态。
    #[test]
    fn content_anchor_corrects_model_index_drift_within_batch() {
        let expected: Vec<(usize, &str)> = vec![(10, "apple"), (11, "banana"), (12, "cherry")];
        let raw = r#"{
            "11": {"src": "banana", "t": "香蕉"},
            "13": {"src": "cherry", "t": "樱桃"},
            "12": {"src": "apple", "t": "苹果"}
        }"#;
        let m = parse_aligned_mapping(raw, &expected).unwrap();
        // 翻译必须落在正确语义行，而不是模型自报的 i
        assert_eq!(m.get(&10), Some(&"苹果".to_string()));
        assert_eq!(m.get(&11), Some(&"香蕉".to_string()));
        assert_eq!(m.get(&12), Some(&"樱桃".to_string()));
        assert!(!m.contains_key(&13));
    }

    #[test]
    fn anchor_falls_back_to_model_index_when_no_src() {
        // 旧格式：值就是译文、无 src → 用模型自报 i
        let m = parse_aligned_mapping(r#"{"3":"你好","4":"世界"}"#, &[(3, "hello"), (4, "world")]).unwrap();
        assert_eq!(m.get(&3), Some(&"你好".to_string()));
        assert_eq!(m.get(&4), Some(&"世界".to_string()));
    }

    #[test]
    fn anchor_skips_empty_translation() {
        let m = parse_aligned_mapping(
            r#"{"0":{"src":"hi","t":""}}"#,
            &[(0, "hi"), (1, "there")],
        )
        .unwrap();
        assert!(!m.contains_key(&0));
        assert!(!m.contains_key(&1));
    }

    /// 回归：某批漏译一行时，后续批的全局序号不得左移（否则译文整体错位）。
    #[test]
    fn batch_indices_do_not_shift_when_a_batch_loses_a_line() -> anyhow::Result<()> {
        // 28 段：批0 = 段0..24（标签0..24），批1 = 段25..27（标签25..27）。
        let segments: Vec<Segment> = (0..28)
            .map(|i| Segment { start_ms: i * 1000, end_ms: i * 1000 + 900, text: format!("s{i}") })
            .collect();
        // 忠实模型：对 prompt 里每行返回 {i, src, t:"T<原文>"}；但每批舍去该批最大标签（模拟漏译一行）。
        let fake = |_sys: &str, usr: &str| -> anyhow::Result<String> {
            let block = usr
                .split("[Lines]:")
                .nth(1)
                .unwrap_or("")
                .split("Reply JSON now")
                .next()
                .unwrap_or("");
            let mut entries: Vec<(usize, String)> = Vec::new();
            for line in block.lines() {
                let l = line.trim();
                if l.is_empty() {
                    continue;
                }
                if let Some((idx, text)) = l.split_once(". ") {
                    if let Ok(n) = idx.trim().parse::<usize>() {
                        entries.push((n, text.trim().to_string()));
                    }
                }
            }
            if entries.is_empty() {
                return Err(anyhow::anyhow!("未解析到行"));
            }
            let max = entries.iter().map(|(i, _)| *i).max().unwrap();
            let obj: Vec<(usize, String)> = entries
                .into_iter()
                .filter(|(i, _)| *i != max)
                .collect();
            // 文本无非特殊字符，直接手拼合法 JSON 数组。
            let inner = obj
                .iter()
                .map(|(k, v)| format!("{{\"i\":{k},\"src\":\"{v}\",\"t\":\"T{v}\"}}"))
                .collect::<Vec<_>>()
                .join(",");
            Ok(format!("[{inner}]"))
        };

        let m = translate_with(&segments, "Simplified Chinese", fake)?;

        // 段24在批0被漏译 → 不应出现于结果（旧实现会误把「段25的译文」塞到键24）。
        assert!(!m.contains_key(&24));
        assert_eq!(m.get(&25).map(String::as_str), Some("Ts25"));
        assert_eq!(m.get(&26), Some(&"Ts26".to_string()));
        assert_eq!(m.get(&27), None); // 批1最大标签27被漏译
        Ok(())
    }
}
