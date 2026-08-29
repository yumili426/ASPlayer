use crate::lemma_en;
use crate::lemma_ja;

/// 依字符集判定语言。含假名（平/片假名）→ ja；否则视为拉丁 → en。
pub fn detect_lang(term: &str) -> &'static str {
    let has_kana = term.chars().any(|c| matches!(c,
        // 平假名区（含浊音、小写、长音等）
        'ぁ'..='ゖ' | 'ゝ' | 'ゞ' | 'ー'
        // 片假名区（含浊音、小写、长音等）
        | 'ァ'..='ヺ'
        // 半角片假名
        | '\u{FF66}'..='\u{FF9F}'));
    if has_kana { "ja" } else { "en" }
}

/// 词形还原：产出候选基准词集合（英文走 exchange+规则；日文走活用还原）。
pub fn lemma_candidates(term: &str, lang: &'static str) -> Vec<String> {
    match lang {
        "ja" => lemma_ja::deinflect(term).into_iter().collect(),
        _ => lemma_en::rule_candidates(term),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_detect() {
        assert_eq!(detect_lang("running"), "en");
        assert_eq!(detect_lang("食べる"), "ja");
        assert_eq!(detect_lang("たべる"), "ja");
        assert_eq!(detect_lang("good 世界"), "en"); // 拉丁+混合默认为 en（世界是汉字，非假名）
        assert_eq!(detect_lang("コーヒー"), "ja"); // 全角片假名 + 长音
    }

    #[test]
    fn lemma_candidates_path() {
        assert!(lemma_candidates("running", "en").contains(&"run".to_string()));
        assert_eq!(lemma_candidates("食べています", "ja"), vec!["食べる".to_string()]);
    }
}
