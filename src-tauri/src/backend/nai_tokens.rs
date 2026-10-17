//! backend/nai_tokens —— NovelAI 代表性 tokenizer 的本地编码（T16 · Phase 2 批5）。
//!
//! App 契约仅消耗两个 encode 端点（api.ts:266-280）：
//! - `/api/tokenizers/llama3/encode`（erato 模型）—— **精确**：HF tokenizers
//!   crate 直接装载 `llama3.json`（ST `src/tokenizers/llama3.json` 同文件，
//!   WebTokenizer wasm fromJSON 同格式）。
//! - `/api/tokenizers/nerdstash_v2/encode`（clio/kayra 模型）—— **词表精确 /
//!   分段近似**：SPM proto（sentencepiece）BPE 型（trainer_spec.model_type=2）。
//!   Rust 无 SPM-BPE 参考实现（@agnai/sentencepiece-js 是 wasm）→ 本模块解析
//!   proto 拿到**精确的 piece→id 词表**与 unk/bos/eos，分段用
//!   `NFKC + dummy-prefix + 空白→▁ + 贪心最长 piece` 近似。对 NAI 用途
//!   （stop_sequences/bad_words_ids）id 全部落在合法词表内、NAI 端可接受；
//!   分段粒度可能与 SPM wasm 有出入 → 逐字登记（附录 B 精度容差）。
//!
//! 其余 encode 家族（claude/qwen2/command-r 等）App 不调用 → 维持 501
//!（S20 可判定降级）。

use std::collections::HashMap;
use std::sync::OnceLock;

const LLAMA3_JSON: &[u8] = include_bytes!("../../assets/tokenizers/llama3.json");
const NERD_V2_MODEL: &[u8] = include_bytes!("../../assets/tokenizers/nerdstash_v2.model");

/* ================================================================ *
 * llama3（精确）
 * ================================================================ */

fn llama3_tokenizer() -> Result<&'static tokenizers::Tokenizer, String> {
    static TOK: OnceLock<Result<tokenizers::Tokenizer, String>> = OnceLock::new();
    let init = TOK
        .get_or_init(|| tokenizers::Tokenizer::from_bytes(LLAMA3_JSON).map_err(|e| e.to_string()));
    match init {
        Ok(t) => Ok(t),
        Err(e) => Err(e.clone()),
    }
}

/// llama3 encode → Ok(ids) / Err(可判定错误文本)
pub fn encode_llama3(text: &str) -> Result<Vec<i64>, String> {
    let tok = llama3_tokenizer()?;
    Ok(tok
        .encode(text, false)
        .map_err(|e| e.to_string())?
        .get_ids()
        .iter()
        .map(|i| *i as i64)
        .collect())
}

/* ================================================================ *
 * nerdstash_v2（SPM proto 精确词表 + 贪心分段近似）
 * ================================================================ */

struct SpmModel {
    pieces: Vec<String>,         // id → piece
    id_of: HashMap<String, i64>, // piece → id
    unk_id: i64,
}

/// SPM proto（protobuf wire format）最小解析：
/// ModelProto { 1: repeated SentencePiece {1 piece(str), 2 score(fixed32), 3 type(varint)},
///               2 trainer_spec(len), 3 normalizer_spec(len) }
fn parse_spm_proto(b: &[u8]) -> Result<SpmModel, String> {
    fn read_varint(b: &[u8], i: &mut usize) -> Result<u64, String> {
        let mut y: u64 = 0;
        let mut shift = 0u32;
        loop {
            if *i >= b.len() {
                return Err("proto truncated".into());
            }
            let x = b[*i];
            *i += 1;
            y |= ((x & 0x7F) as u64) << shift;
            if x & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        Ok(y)
    }

    let mut id_of: HashMap<String, i64> = HashMap::new();
    // id 顺序与 proto 顺序一致 —— 分段扫描使用
    let mut pieces: Vec<String> = Vec::new();
    let mut unk_id: Option<i64> = None;
    let mut i = 0usize;

    while i < b.len() {
        let tag = read_varint(b, &mut i)?;
        let field = (tag >> 3) as usize;
        let wire = tag & 7;
        if wire == 0 {
            let _v = read_varint(b, &mut i)?;
            continue;
        }
        if wire != 2 {
            return Err(format!("proto: 顶层 wire type {wire} 未支持"));
        }
        let len = read_varint(b, &mut i)? as usize;
        let end = i + len;
        if end > b.len() {
            return Err("proto: 字段长度出界".into());
        }
        let sub = &b[i..end];
        i = end;
        match field {
            1 => {
                // SentencePiece 子消息
                let mut piece = String::new();
                let mut ptype: i64 = 1;
                let mut j = 0usize;
                while j < sub.len() {
                    let t2 = read_varint(sub, &mut j)?;
                    let f2 = (t2 >> 3) as usize;
                    let w2 = t2 & 7;
                    match w2 {
                        0 => {
                            let v = read_varint(sub, &mut j)?;
                            if f2 == 3 {
                                ptype = v as i64;
                            }
                        }
                        1 => break, // fixed64
                        2 => {
                            let l = read_varint(sub, &mut j)? as usize;
                            if f2 == 1 {
                                piece = String::from_utf8_lossy(&sub[j..j + l]).to_string();
                            }
                            j += l;
                        }
                        5 => j += 4, // fixed32（score）
                        _ => return Err(format!("proto: piece wire {w2} 未支持")),
                    }
                }
                // UNKNOWN(2) 的 piece 占 id 但不可匹配；其余可匹配
                if ptype == 2 {
                    if unk_id.is_none() {
                        unk_id = Some(pieces.len() as i64);
                    }
                    pieces.push(String::new());
                } else {
                    id_of.entry(piece.clone()).or_insert(pieces.len() as i64);
                    pieces.push(piece);
                }
            }
            // trainer/normalizer/self_test —— 只占位跳过
            _ => {}
        }
    }

    Ok(SpmModel {
        pieces,
        id_of,
        unk_id: unk_id.unwrap_or(0),
    })
}

fn spm_model() -> Result<&'static SpmModel, String> {
    static M: OnceLock<Result<SpmModel, String>> = OnceLock::new();
    let init = M.get_or_init(|| parse_spm_proto(NERD_V2_MODEL));
    match init {
        Ok(m) => Ok(m),
        Err(e) => Err(e.clone()),
    }
}

/// SRM 规格化（SPM 默认：add dummy prefix + 空白→▁ + NFKC）
fn nerd_normalize(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    // NFKC 近似 precompiled charsmap（SPM 默认 NormalizerSpec = nmt_nfkc）
    let nfkc: String = text.nfkc().collect();
    let mut out = String::with_capacity(nfkc.len() + 1);
    out.push('\u{2581}'); // dummy prefix
    for ch in nfkc.chars() {
        if ch == ' ' {
            out.push('\u{2581}');
        } else {
            out.push(ch);
        }
    }
    out
}

/// nerdstash_v2 encode → 词表可靠、分段为贪心最长 piece 近似
pub fn encode_nerdstash_v2(text: &str) -> Result<Vec<i64>, String> {
    let m = spm_model()?;
    let normalized = nerd_normalize(text);
    let bytes = normalized.as_bytes();
    let mut ids: Vec<i64> = Vec::new();
    let mut pos = 0usize;

    while pos < bytes.len() {
        let remaining = bytes.len() - pos;
        let mut found: Option<(i64, usize)> = None;
        let mut l = remaining.min(64);
        while l >= 1 {
            if let Ok(s) = std::str::from_utf8(&bytes[pos..pos + l]) {
                if let Some(id) = m.id_of.get(s) {
                    found = Some((*id, l));
                    break;
                }
            }
            l -= 1;
        }
        match found {
            Some((id, byte_len)) => {
                ids.push(id);
                pos += byte_len.max(1);
            }
            None => {
                // 不可匹配：unk 单步推进
                ids.push(m.unk_id);
                pos += 1;
            }
        }
    }
    Ok(ids)
}

/* ============================================================ *
 * 入口：App 两个 encode 端点（#52/#53）—— handlers 在 tokenizers_secrets.rs
 * ============================================================ */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llama3_ids_valid_and_deterministic() {
        let a = encode_llama3("Hello world").unwrap();
        assert!(!a.is_empty());
        assert!(
            a.iter().all(|i| (0..128256).contains(i)),
            "llama3 词表 id 上界 128256"
        );
        let b = encode_llama3("Hello world").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn nerdstash_ids_in_vocab_range() {
        let ids = encode_nerdstash_v2("The quick brown fox").unwrap();
        assert!(!ids.is_empty());
        assert!(
            ids.iter().all(|i| (0..44351).contains(i)),
            "nerdstash_v2 词表 ~44351"
        );
        // known unk: 随机高档 unicode 全部给合法（unk id 同样有效 id 域）
        let weird = encode_nerdstash_v2("🦊").unwrap();
        assert!(!weird.is_empty());
    }

    #[test]
    fn proto_pieces_loaded() {
        let m = spm_model().unwrap();
        assert!(!m.pieces.is_empty());
        // 首批已知特殊 piece 出现在词表（nerdstash 前缀 |edn...）
        assert!(
            m.pieces.iter().any(|p| p.contains("<")),
            "词表应包含 SPM 片形"
        );
    }
}
