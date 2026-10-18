//! backend/classify —— POST /api/extra/classify 本地实装（manifest #66 · R4）
//!
//! ── 定性（先读这段再改）──
//! 本模块是**无 ML 运行时的词典近似**：中英文关键词表 → 标签确定性打分，
//! 不含模型文件、不出网、不引新依赖。它刻意把**接口形状与 ST extra
//! classify 对齐**（请求 {text, top_k?} → 响应 [{label, score}]；ST classify.js
//! 与前端 src/services/st/expressions.ts:34 只读数组首条的 label）——因此未来
//! 换成真正的模型推理时，**前端与路由零改动**：只需重写 classify_text
//! 这一个语义入口，响应形状不变。
//!
//! ── 契约 ──
//! - 请求：{text: string, top_k?: number}；top_k 默认 5，至少返回 1 条；
//! - 响应：JSON 数组 [{label, score}]，按 score 降序，同分按标签字典序
//!   （同输入同输出，便于快照/回归比对）；
//! - 标签集：GoEmotions 28 类（GOEMOTIONS_LABELS，与 ST emotion labels 同名）；
//! - score ∈ [0,1]：本实现按命中次数归一化，最高分恒为 1.0；
//! - 空文本 / 无命中 → [{"label":"neutral","score":1.0}]；
//! - text 截断到 MAX_TEXT_CHARS 个字符（非字节），超出部分不参与打分。
//!
//! ── 已知边界 ──
//! 词典法不理解否定与反讽（如「不开心」仍按「开心」命中）：这是近似法的
//! 显式代价；本模块只承诺响应形状与确定性，不承诺语义精度。

use axum::http::HeaderValue;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// GoEmotions 28 类（ST classify 管线的标签集；字母序）
pub const GOEMOTIONS_LABELS: [&str; 28] = [
    "admiration",
    "amusement",
    "anger",
    "annoyance",
    "approval",
    "caring",
    "confusion",
    "curiosity",
    "desire",
    "disappointment",
    "disapproval",
    "disgust",
    "embarrassment",
    "excitement",
    "fear",
    "gratitude",
    "grief",
    "joy",
    "love",
    "nervousness",
    "optimism",
    "pride",
    "realization",
    "relief",
    "remorse",
    "sadness",
    "surprise",
    "neutral",
];

/// top_k 默认值（至少返回 1 条）
pub const DEFAULT_TOP_K: usize = 5;

/// text 截断上限（字符数）
pub const MAX_TEXT_CHARS: usize = 4096;

/// 单条分类结果（即响应数组元素）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClassifyHit {
    pub label: String,
    pub score: f64,
}

/// (关键词, 标签) 词典：关键词全小写；ASCII 词按词边界匹配，CJK 词按子串匹配
const LEXICON: &[(&str, &str)] = &[
    // admiration
    ("敬佩", "admiration"),
    ("钦佩", "admiration"),
    ("佩服", "admiration"),
    ("崇拜", "admiration"),
    ("赞叹", "admiration"),
    ("admire", "admiration"),
    ("admired", "admiration"),
    ("admiration", "admiration"),
    ("impressed", "admiration"),
    // amusement
    ("哈哈", "amusement"),
    ("搞笑", "amusement"),
    ("好笑", "amusement"),
    ("有趣", "amusement"),
    ("逗笑", "amusement"),
    ("funny", "amusement"),
    ("amused", "amusement"),
    ("hilarious", "amusement"),
    ("lol", "amusement"),
    ("haha", "amusement"),
    // anger
    ("生气", "anger"),
    ("愤怒", "anger"),
    ("恼火", "anger"),
    ("火大", "anger"),
    ("气死", "anger"),
    ("angry", "anger"),
    ("anger", "anger"),
    ("furious", "anger"),
    ("rage", "anger"),
    ("mad", "anger"),
    // annoyance
    ("烦人", "annoyance"),
    ("烦躁", "annoyance"),
    ("烦死", "annoyance"),
    ("好烦", "annoyance"),
    ("讨厌", "annoyance"),
    ("annoyed", "annoyance"),
    ("annoying", "annoyance"),
    ("irritating", "annoyance"),
    // approval
    ("赞同", "approval"),
    ("同意", "approval"),
    ("认可", "approval"),
    ("赞成", "approval"),
    ("支持", "approval"),
    ("approve", "approval"),
    ("approval", "approval"),
    ("agreed", "approval"),
    ("agree", "approval"),
    // caring
    ("关心", "caring"),
    ("照顾", "caring"),
    ("在乎", "caring"),
    ("体贴", "caring"),
    ("caring", "caring"),
    ("cared", "caring"),
    // confusion
    ("困惑", "confusion"),
    ("疑惑", "confusion"),
    ("不解", "confusion"),
    ("迷惑", "confusion"),
    ("搞不懂", "confusion"),
    ("confused", "confusion"),
    ("confusion", "confusion"),
    ("puzzled", "confusion"),
    // curiosity
    ("好奇", "curiosity"),
    ("想知道", "curiosity"),
    ("感兴趣", "curiosity"),
    ("curious", "curiosity"),
    ("curiosity", "curiosity"),
    ("interested", "curiosity"),
    // desire
    ("想要", "desire"),
    ("想要得到", "desire"),
    ("渴望", "desire"),
    ("希望得到", "desire"),
    ("好想", "desire"),
    ("desire", "desire"),
    ("craving", "desire"),
    ("wish", "desire"),
    // disappointment
    ("失望", "disappointment"),
    ("遗憾", "disappointment"),
    ("disappointed", "disappointment"),
    ("disappointment", "disappointment"),
    // disapproval
    ("反对", "disapproval"),
    ("不赞同", "disapproval"),
    ("不认可", "disapproval"),
    ("不应该", "disapproval"),
    ("disapprove", "disapproval"),
    ("disagree", "disapproval"),
    ("disapproval", "disapproval"),
    // disgust
    ("恶心", "disgust"),
    ("厌恶", "disgust"),
    ("反感", "disgust"),
    ("作呕", "disgust"),
    ("disgust", "disgust"),
    ("disgusted", "disgust"),
    ("gross", "disgust"),
    ("yuck", "disgust"),
    // embarrassment
    ("尴尬", "embarrassment"),
    ("害羞", "embarrassment"),
    ("难为情", "embarrassment"),
    ("不好意思", "embarrassment"),
    ("embarrassed", "embarrassment"),
    ("embarrassment", "embarrassment"),
    ("awkward", "embarrassment"),
    // excitement
    ("兴奋", "excitement"),
    ("激动", "excitement"),
    ("期待", "excitement"),
    ("excited", "excitement"),
    ("excitement", "excitement"),
    ("thrilled", "excitement"),
    // fear
    ("害怕", "fear"),
    ("恐惧", "fear"),
    ("惊恐", "fear"),
    ("吓人", "fear"),
    ("可怕", "fear"),
    ("afraid", "fear"),
    ("fear", "fear"),
    ("scared", "fear"),
    ("frightened", "fear"),
    ("terrified", "fear"),
    // gratitude
    ("谢谢", "gratitude"),
    ("感谢", "gratitude"),
    ("多谢", "gratitude"),
    ("感恩", "gratitude"),
    ("辛苦了", "gratitude"),
    ("thank", "gratitude"),
    ("thanks", "gratitude"),
    ("grateful", "gratitude"),
    ("gratitude", "gratitude"),
    // grief
    ("悲痛", "grief"),
    ("哀悼", "grief"),
    ("哀伤", "grief"),
    ("逝去", "grief"),
    ("grief", "grief"),
    ("mourning", "grief"),
    ("mourn", "grief"),
    // joy
    ("开心", "joy"),
    ("高兴", "joy"),
    ("快乐", "joy"),
    ("愉快", "joy"),
    ("幸福", "joy"),
    ("欢喜", "joy"),
    ("happy", "joy"),
    ("joy", "joy"),
    ("joyful", "joy"),
    ("glad", "joy"),
    ("delighted", "joy"),
    ("happiness", "joy"),
    // love
    ("爱你", "love"),
    ("我爱", "love"),
    ("喜爱", "love"),
    ("深爱", "love"),
    ("挚爱", "love"),
    ("恋人", "love"),
    ("love", "love"),
    ("adore", "love"),
    ("beloved", "love"),
    // nervousness
    ("紧张", "nervousness"),
    ("焦虑", "nervousness"),
    ("不安", "nervousness"),
    ("担心", "nervousness"),
    ("忐忑", "nervousness"),
    ("nervous", "nervousness"),
    ("anxious", "nervousness"),
    ("anxiety", "nervousness"),
    ("worried", "nervousness"),
    // optimism
    ("乐观", "optimism"),
    ("会好起来", "optimism"),
    ("充满希望", "optimism"),
    ("有希望", "optimism"),
    ("optimistic", "optimism"),
    ("optimism", "optimism"),
    ("hopeful", "optimism"),
    ("hopefully", "optimism"),
    // pride
    ("骄傲", "pride"),
    ("自豪", "pride"),
    ("得意", "pride"),
    ("引以为傲", "pride"),
    ("proud", "pride"),
    ("pride", "pride"),
    // realization
    ("原来", "realization"),
    ("明白了", "realization"),
    ("意识到", "realization"),
    ("恍然大悟", "realization"),
    ("realized", "realization"),
    ("realise", "realization"),
    ("realization", "realization"),
    ("i see", "realization"),
    // relief
    ("松了口气", "relief"),
    ("松一口气", "relief"),
    ("放心", "relief"),
    ("释然", "relief"),
    ("好险", "relief"),
    ("relief", "relief"),
    ("relieved", "relief"),
    ("phew", "relief"),
    // remorse
    ("对不起", "remorse"),
    ("抱歉", "remorse"),
    ("后悔", "remorse"),
    ("内疚", "remorse"),
    ("愧疚", "remorse"),
    ("懊悔", "remorse"),
    ("sorry", "remorse"),
    ("remorse", "remorse"),
    ("regret", "remorse"),
    ("apologize", "remorse"),
    // sadness
    ("难过", "sadness"),
    ("悲伤", "sadness"),
    ("伤心", "sadness"),
    ("沮丧", "sadness"),
    ("失落", "sadness"),
    ("痛哭", "sadness"),
    ("哭泣", "sadness"),
    ("想哭", "sadness"),
    ("泪目", "sadness"),
    ("sad", "sadness"),
    ("sadness", "sadness"),
    ("sorrow", "sadness"),
    ("cry", "sadness"),
    ("unhappy", "sadness"),
    // surprise
    ("惊讶", "surprise"),
    ("震惊", "surprise"),
    ("意外", "surprise"),
    ("吃惊", "surprise"),
    ("没想到", "surprise"),
    ("surprised", "surprise"),
    ("surprise", "surprise"),
    ("shocked", "surprise"),
    ("wow", "surprise"),
    ("unexpected", "surprise"),
];

/// 关键词出现次数：ASCII 词要求词边界（防 mad 命中 made），CJK 词按子串计数
fn keyword_hits(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() || !haystack.contains(needle) {
        return 0;
    }
    if !needle.is_ascii() {
        return haystack.matches(needle).count();
    }
    let bytes = haystack.as_bytes();
    let mut hits = 0usize;
    let mut from = 0usize;
    while from < haystack.len() {
        let Some(rel) = haystack[from..].find(needle) else {
            break;
        };
        let start = from + rel;
        let end = start + needle.len();
        let before_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let after_ok = end == haystack.len()
            || !haystack[end..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric());
        if before_ok && after_ok {
            hits += 1;
        }
        from = end;
    }
    hits
}

/// 词典打分入口（未来换模型推理只改这里）：返回降序命中，至少 1 条
pub fn classify_text(text: &str, top_k: usize) -> Vec<ClassifyHit> {
    let truncated: String = text.chars().take(MAX_TEXT_CHARS).collect();
    let lower = truncated.to_lowercase();

    let mut raw: HashMap<&'static str, usize> = HashMap::new();
    for &(keyword, label) in LEXICON {
        let n = keyword_hits(&lower, keyword);
        if n > 0 {
            *raw.entry(label).or_insert(0) += n;
        }
    }

    if raw.is_empty() {
        return vec![ClassifyHit {
            label: "neutral".to_string(),
            score: 1.0,
        }];
    }

    let mut ranked: Vec<(&'static str, usize)> = raw.into_iter().collect();
    // 同分按标签字典序 —— 打分确定性与快照稳定
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let max = ranked[0].1.max(1) as f64;
    let k = top_k.clamp(1, GOEMOTIONS_LABELS.len()).min(ranked.len());

    ranked
        .into_iter()
        .take(k)
        .map(|(label, n)| ClassifyHit {
            label: label.to_string(),
            score: (n as f64 / max).clamp(0.0, 1.0),
        })
        .collect()
}

/// POST /api/extra/classify（api.rs route() 分发接入；鉴权/CORS 走既有统一入口）
pub fn handle(body: &Value, origin: Option<HeaderValue>) -> Response {
    let text = body.get("text").and_then(Value::as_str).unwrap_or("");
    let top_k = body
        .get("top_k")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_TOP_K);
    let hits = classify_text(text, top_k);
    super::apply_cors_resp(Json(hits).into_response(), origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels_of(hits: &[ClassifyHit]) -> Vec<&str> {
        hits.iter().map(|h| h.label.as_str()).collect()
    }

    fn top_label(text: &str) -> String {
        classify_text(text, DEFAULT_TOP_K)[0].label.clone()
    }

    #[test]
    fn chinese_gratitude_maps_to_gratitude() {
        assert_eq!(top_label("谢谢你帮了我"), "gratitude");
    }

    #[test]
    fn chinese_sadness_maps_to_sadness() {
        assert_eq!(top_label("我好难过"), "sadness");
    }

    #[test]
    fn english_joy_detected() {
        assert_eq!(top_label("I am so happy today"), "joy");
        // ASCII 关键词要求词边界：mad 不得命中 made
        assert_eq!(top_label("he made a table"), "neutral");
    }

    #[test]
    fn empty_or_unmatched_text_falls_back_to_neutral() {
        let neutral = vec![ClassifyHit {
            label: "neutral".to_string(),
            score: 1.0,
        }];
        assert_eq!(classify_text("", DEFAULT_TOP_K), neutral);
        assert_eq!(classify_text("   ", DEFAULT_TOP_K), neutral);
        assert_eq!(classify_text("zxqv 量子泡沫", DEFAULT_TOP_K), neutral);
    }

    #[test]
    fn top_k_takes_effect_and_never_returns_empty() {
        let text = "谢谢你，我好难过，但是我很开心，也很生气";
        assert!(classify_text(text, 28).len() >= 3, "应命中多个标签");
        assert_eq!(classify_text(text, 1).len(), 1);
        assert_eq!(classify_text(text, 2).len(), 2);
        assert_eq!(classify_text(text, 0).len(), 1, "top_k=0 也必须至少 1 条");
        assert!(classify_text(text, 100).len() <= 28);
    }

    #[test]
    fn scores_are_descending_and_within_unit_range() {
        let hits = classify_text("谢谢你，我好难过，但是我很开心，也很生气", 28);
        assert!(hits.len() >= 2, "应命中多个标签：{hits:?}");
        assert!(
            (hits[0].score - 1.0).abs() < f64::EPSILON,
            "最高分应为 1.0：{hits:?}"
        );
        for w in hits.windows(2) {
            assert!(w[0].score >= w[1].score, "score 必须降序：{hits:?}");
        }
        for h in &hits {
            assert!((0.0..=1.0).contains(&h.score), "score 越界：{h:?}");
        }
    }

    #[test]
    fn ties_are_broken_by_label_order_and_result_is_deterministic() {
        let text = "我很开心，也很生气"; // joy 与 anger 各命中 1 次 → 同分
        let a = classify_text(text, 28);
        let b = classify_text(text, 28);
        assert_eq!(labels_of(&a), vec!["anger", "joy"], "同分必须按标签字典序");
        assert_eq!(labels_of(&a), labels_of(&b), "同输入必须同输出");
    }

    #[test]
    fn every_label_stays_within_goemotions_set() {
        assert_eq!(GOEMOTIONS_LABELS.len(), 28);
        for &(keyword, label) in LEXICON {
            assert!(
                GOEMOTIONS_LABELS.contains(&label),
                "词表标签越界：{keyword} → {label}"
            );
        }
        let hits = classify_text("谢谢 难过 开心 生气 害怕 惊讶 讨厌 好奇", 28);
        for h in &hits {
            assert!(
                GOEMOTIONS_LABELS.contains(&h.label.as_str()),
                "越界标签：{h:?}"
            );
        }
    }

    #[test]
    fn text_is_truncated_to_max_chars() {
        let tail = "a".repeat(MAX_TEXT_CHARS) + "谢谢你";
        assert_eq!(top_label(&tail), "neutral", "截断后不应命中尾部关键词");
        let head = "谢谢你".to_string() + &"a".repeat(MAX_TEXT_CHARS);
        assert_eq!(top_label(&head), "gratitude", "截断前段的关键词仍应命中");
    }

    #[tokio::test]
    async fn response_shape_is_json_array_of_label_and_score() {
        let resp = handle(&serde_json::json!({ "text": "谢谢你" }), None);
        assert_eq!(resp.status().as_u16(), 200);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = v.as_array().expect("响应必须是 JSON 数组");
        assert!(!arr.is_empty(), "至少返回 1 条");
        for item in arr {
            assert!(
                item.get("label").and_then(Value::as_str).is_some(),
                "{item}"
            );
            let score = item
                .get("score")
                .and_then(Value::as_f64)
                .expect("score 应为数字");
            assert!((0.0..=1.0).contains(&score), "{item}");
        }
        assert_eq!(arr[0]["label"], "gratitude");
    }
}
