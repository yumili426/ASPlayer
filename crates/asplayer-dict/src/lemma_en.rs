use std::collections::HashMap;

pub fn normalize_surface(w: &str) -> String {
    w.trim().to_lowercase()
}

/// 解析 ECDICT exchange 字段为 `kind -> 变体` 映射。kind: d=过去式, i=ing, p=过去分词, 3=单三,
/// r=比较级, t=最高级, s=复数, 0=原型, 1=原型（英式）
pub fn exchange_variants(exchange: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for pair in exchange.split('/') {
        if let Some((k, v)) = pair.split_once(':') {
            m.insert(k.to_string(), v.to_string());
        }
    }
    m
}

/// 依英文变形规则生成候选基准词（含常见不规则表）。
pub fn rule_candidates(surface: &str) -> Vec<String> {
    let w = normalize_surface(surface);
    let mut out = Vec::new();
    if w.is_empty() {
        return out;
    }
    let mut push = |s: String| {
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    // 表面词本身永远作为基准候选（确保未变形词也返回自身，如 "cat" -> ["cat"]）
    push(w.clone());
    if let Some(b) = irr(&w) {
        push(b.to_string());
    }

    // 复数：-es / -ies / -s（-ies 同时覆盖单三）
    if let Some(base) = w.strip_suffix("es") {
        push(base.to_string());
    }
    if let Some(base) = w.strip_suffix("ies") {
        push(format!("{}y", base));
    }
    if let Some(base) = w.strip_suffix("s") {
        push(base.to_string());
    }

    // 现在进行时 -ing：直接去ing；双写去尾（running→run）；补回静音e（making→make）
    if let Some(stem) = w.strip_suffix("ing") {
        push(stem.to_string());
        let chars: Vec<char> = stem.chars().collect();
        if chars.len() >= 2 && chars[chars.len() - 1] == chars[chars.len() - 2] {
            push(chars[..chars.len() - 1].iter().collect());
        }
        push(format!("{stem}e"));
    }

    // 过去式 -ied / -ed（-ied → y；-ed 去ed、补e、双写去尾）
    if let Some(stem) = w.strip_suffix("ied") {
        push(format!("{}y", stem));
    }
    if let Some(stem) = w.strip_suffix("ed") {
        push(stem.to_string());
        push(format!("{stem}e"));
        let chars: Vec<char> = stem.chars().collect();
        if chars.len() >= 2 && chars[chars.len() - 1] == chars[chars.len() - 2] {
            push(chars[..chars.len() - 1].iter().collect());
        }
    }

    // 比较级 / 最高级：-iest/-ier → y；-est/-er 去尾、双写去尾（bigger→big）、补静音e（nicer→nice）
    if let Some(stem) = w.strip_suffix("iest") {
        push(format!("{}y", stem));
    }
    if let Some(stem) = w.strip_suffix("ier") {
        push(format!("{}y", stem));
    }
    if let Some(stem) = w.strip_suffix("est") {
        push(stem.to_string());
        let chars: Vec<char> = stem.chars().collect();
        if chars.len() >= 2 && chars[chars.len() - 1] == chars[chars.len() - 2] {
            push(chars[..chars.len() - 1].iter().collect());
        }
        push(format!("{stem}e"));
    }
    if let Some(stem) = w.strip_suffix("er") {
        push(stem.to_string());
        let chars: Vec<char> = stem.chars().collect();
        if chars.len() >= 2 && chars[chars.len() - 1] == chars[chars.len() - 2] {
            push(chars[..chars.len() - 1].iter().collect());
        }
        push(format!("{stem}e"));
    }

    out
}

fn irr(w: &str) -> Option<&'static str> {
    match w {
        "better" => Some("good"), "best" => Some("good"), "worse" => Some("bad"), "worst" => Some("bad"),
        "went" => Some("go"), "gone" => Some("go"), "children" => Some("child"), "men" => Some("man"),
        "ran" => Some("run"), "ate" => Some("eat"), "is" | "are" | "was" | "were" => Some("be"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exchange_variants() {
        let ex = "d:went/i:going/p:gone/3:goes";
        let map = exchange_variants(ex);
        assert_eq!(map.get("i"), Some(&"going".to_string()));
        assert_eq!(map.get("p"), Some(&"gone".to_string()));
    }

    #[test]
    fn rule_candidates_english() {
        let c = rule_candidates("running");
        assert!(c.contains(&"run".to_string()));
        let c2 = rule_candidates("studies");
        assert!(c2.contains(&"study".to_string()));
        let c3 = rule_candidates("better");
        assert!(c3.contains(&"good".to_string())); // 不规则列表
        assert!(rule_candidates("making").contains(&"make".to_string())); // 静音e
        assert!(rule_candidates("stopped").contains(&"stop".to_string())); // 双写ed
        assert!(rule_candidates("went").contains(&"go".to_string())); // 不规则
        assert!(rule_candidates("cat").contains(&"cat".to_string())); // 未变形词返回自身
    }

    #[test]
    fn normalize_surface_lowercases() {
        assert_eq!(normalize_surface(" Running "), "running");
    }
}
