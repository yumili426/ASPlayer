/// 一条词典产出的查询结果（对齐 UI 卡片字段）
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LookupResult {
    pub term: String,
    pub lang: &'static str, // "en" | "ja"
    pub phonetic: Option<String>,   // en: 音标；ja: None（读音走 reading）
    pub reading: Option<String>,    // ja: 假名读音
    pub pos: Option<String>,        // 词性
    pub definitions: Vec<String>,   // en: 中文释义；ja: 英文 gloss
    pub suggestions: Vec<String>,   // 未命中时的相似词
}

#[derive(Debug, Clone, Default)]
pub struct EnEntry {
    pub word: String,
    pub phonetic: String,
    pub definition: String,   // 中文解释
    pub translation: String,  // 中文翻译（较简）
    pub pos: String,
    pub exchange: String,     // 词形变化，如 "d:went/i:going/p:gone/3:goes"
}

#[derive(Debug, Clone, Default)]
pub struct JaEntry {
    pub surface: String,   // 词典形（汉字/表记）
    pub reading: String,   // 假名读音
    pub pos: String,
    pub gloss: String,     // 英文释义
}

#[derive(Debug, Clone, Default)]
pub struct Suggestion {
    pub term: String,
    pub score: u32,
}
