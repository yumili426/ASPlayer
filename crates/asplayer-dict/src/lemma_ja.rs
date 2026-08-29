/// 把常用活用形还原为辞书形。基于替换规则表，覆盖常见口语/书面形；未匹配则原样返回。
/// 注意：日文活用还原较复杂，本版为规则近似（ます/て/た 形等），后续按真实数据迭代。
pub fn deinflect(surface: &str) -> Option<String> {
    let s = surface.trim();
    if s.is_empty() {
        return None;
    }
    deinflect_rules(s).or_else(|| Some(s.to_string()))
}

fn deinflect_rules(s: &str) -> Option<String> {
    // 更 specific（更长）的先匹配
    if let Some(stem) = s.strip_suffix("ています") {
        return Some(append_dict(stem));
    }
    if let Some(stem) = s.strip_suffix("ていました") {
        return Some(append_dict(stem));
    }
    if let Some(stem) = s.strip_suffix("ます") {
        return Some(renyoukei_to_dictionary(stem));
    }
    if let Some(stem) = s.strip_suffix("ました") {
        return Some(renyoukei_to_dictionary(stem));
    }
    // 音便·促音便（best-effort）：行って -> 行く / 行った -> 行く
    if let Some(stem) = s.strip_suffix("って") {
        return Some(format!("{}く", stem));
    }
    if let Some(stem) = s.strip_suffix("った") {
        return Some(format!("{}く", stem));
    }
    // 一般 て/た -> る（食べて/食べた -> 食べる）
    if let Some(stem) = s.strip_suffix("て") {
        return Some(append_dict(stem));
    }
    if let Some(stem) = s.strip_suffix("た") {
        return Some(append_dict(stem));
    }
    if let Some(stem) = s.strip_suffix("ない") {
        return Some(append_dict(stem));
    }
    None
}

fn append_dict(stem: &str) -> String {
    format!("{stem}る")
}

/// 連用形末假名映射到辞书形结尾：五段（い段→う段）；否则视为一段 +る。
fn renyoukei_to_dictionary(stem: &str) -> String {
    let mut chars: Vec<char> = stem.chars().collect();
    match chars.pop() {
        Some('き') => chars.push('く'),
        Some('み') => chars.push('む'),
        Some('し') => chars.push('す'),
        Some('ち') => chars.push('つ'),
        Some('に') => chars.push('ぬ'),
        Some('び') => chars.push('ぶ'),
        Some('り') => chars.push('る'),
        Some('ぎ') => chars.push('ぐ'),
        Some(c) => { chars.push(c); chars.push('る'); }
        None => {}
    }
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deinflect_common_ja() {
        assert_eq!(deinflect("食べています"), Some("食べる".to_string()));
        assert_eq!(deinflect("食べた"), Some("食べる".to_string()));
        assert_eq!(deinflect("行って"), Some("行く".to_string()));
        assert_eq!(deinflect("飲みます"), Some("飲む".to_string()));
        assert_eq!(deinflect("かわいい"), Some("かわいい".to_string())); // 词干/形容，原样
    }
}
