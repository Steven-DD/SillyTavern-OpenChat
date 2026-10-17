//! backend/tokenizers_secrets —— Token 计数 + 密钥写入（Phase 1 族七 · manifest #50-#53）
//!
//! ST 契约核对：
//! - tokenizers.js:916-1036 `/openai/count`：真 tokenizer 失败时回落
//!   **guesstimate**（`ceil(utf8 bytes / 3.35)`，tokenizers.js:68-71，
//!   BYTES_PER_TOKEN=3.35）。Phase 1 无重型 tokenizer crate → 直接走
//!   guesstimate（ST 自家的回落公式；前端本就有三级兜底，清单 T16
//!   分层标准即「一期近似、逐模型精确化」）。body 是**消息数组**，
//!   ST 失败回落对 `JSON.stringify(req.body)` 计数 —— 同口径。
//! - tokenizers.js /encode 端点族（llama3/nerdstash_v2/claude 等）：
//!   返回 `{ids}` token 序列，精度要求 0 误差（NAI stop_sequences 语义），
//!   **不可近似伪造 → 501 NOT_MIGRATED**（NAI 属 Phase 2，连续性清单 §1.2）。
//! - secrets.js:511-525 `/write {key, value, label?}`：多值结构 ——
//!   push `{id: uuid, value, label: 'Unlabeled', active: true}` 且先
//!   `_deactivateAllSecrets`；落 `<root>/secrets.json`（明文 JSON，
//!   secrets.js:150 —— T12 迁移为 vault 前 fallback 形态）。
//!   返回 `{id}`（前端 api.ts:524 只透传）。

use super::{apply_cors_resp, BackendState};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

/// ST BYTES_PER_TOKEN（tokenizers.js:59）
const BYTES_PER_TOKEN: f64 = 3.35;

fn err_json(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": { "message": msg } }))).into_response()
}

fn ok_json(origin: Option<HeaderValue>, v: Value) -> Response {
    apply_cors_resp(Json(v).into_response(), origin)
}

/// guesstimate（tokenizers.js:68-71）：
/// ceil(UTF-8 字节数 / 3.35)。
pub fn guesstimate(text: &str) -> u64 {
    (text.as_bytes().len() as f64 / BYTES_PER_TOKEN).ceil() as u64
}

/// POST /api/tokenizers/openai/count?model=（manifest #51）
/// body：消息数组（前端 tokenizer.ts:28-31）—— ST 回落按 JSON.stringify(body) 计数。
/// 返回 `{token_count}`。
pub fn count(st: &BackendState, body_bytes: &[u8], origin: Option<HeaderValue>) -> Response {
    if body_bytes.is_empty() {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    // Phase 1：gueestimate 口径 —— 与 ST 失败回落字面一致（对整个 body JSON 计数）。
    // 真 tokenizer（tiktoken/SentencePiece/Web tokenizer）随 Phase 3 接入 crate。
    let n = guesstimate(&String::from_utf8_lossy(body_bytes));
    let _ = st;
    ok_json(origin, json!({ "token_count": n }))
}

/// POST /api/tokenizers/<model>/encode 的非 App 契约家族 → 501（S20）
fn encode_unsupported(model: &str, origin: Option<HeaderValue>) -> Response {
    let resp = (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": format!("tokenizer {model} encode 尚未开放（App 契约仅 llama3/nerdstash_v2）"),
                "code": "NOT_MIGRATED",
            }
        })),
    );
    apply_cors_resp(resp.into_response(), origin)
}

/// POST /api/tokenizers/<model>/encode（manifest #52/#53 · T16 批5）
/// App 契约只消耗 llama3 / nerdstash_v2（api.ts:266-280 NAI 停止词/禁词编码）：
/// - llama3：HF tokenizers 精确（见 nai_tokens.rs 模块头）
/// - nerdstash_v2：词表精确 + 贪心分段近似（SPM BPE 无 Rust 参考实现）
pub fn encode_with_body(model: &str, body_bytes: &[u8], origin: Option<HeaderValue>) -> Response {
    if model != "llama3" && model != "nerdstash_v2" {
        return encode_unsupported(model, origin);
    }
    let text = match serde_json::from_slice::<serde_json::Value>(body_bytes) {
        Ok(v) => v
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string(),
        Err(_) => {
            return apply_cors_resp(
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": { "message": "请求体不是合法 JSON" } })),
                )
                    .into_response(),
                origin,
            )
        }
    };
    let result = match model {
        "llama3" => super::nai_tokens::encode_llama3(&text),
        _ => super::nai_tokens::encode_nerdstash_v2(&text),
    };
    match result {
        Ok(ids) => apply_cors_resp(
            (
                StatusCode::OK,
                Json(json!({ "ids": ids, "length": ids.len() })),
            )
                .into_response(),
            origin,
        ),
        Err(e) => apply_cors_resp(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "message": e } })),
            )
                .into_response(),
            origin,
        ),
    }
}

/* ---------------- secrets（manifest #50） ---------------- */

fn secrets_path(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user").join("secrets.json")
}

/// 生成 uuid v4（无 uuid crate —— getrandom 拼标准格式 8-4-4-4-12）
fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("OS CSPRNG 失败");
    b[6] = (b[6] & 0x0F) | 0x40; // version 4
    b[8] = (b[8] & 0x3F) | 0x80; // variant 10
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// _deactivateAllSecrets（secrets.js:153-164）：把该 key 下全部置 active=false
fn deactivate_all(arr: &mut Vec<Value>) {
    for s in arr.iter_mut() {
        if let Some(o) = s.as_object_mut() {
            o.insert("active".into(), json!(false));
        }
    }
}

/// POST /api/secrets/write {key, value, label?}（manifest #50）
/// 多值结构：active �ertos 标志 + 新条目激活、旧条目全部失活。
/// 返回 `{id}`。⚠ T12（keychain vault）落地前仍为明文 JSON 北斗形态 ——
/// 这是与 ST 当前一致的已知基线（清单 T1 已纠正文档口径）。
pub fn secrets_write(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(key) = body
        .get("key")
        .and_then(|v| v.as_str())
        .filter(|k| !k.is_empty())
    else {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, "Invalid key or value").into_response(),
            origin,
        );
    };
    let Some(value) = body.get("value").and_then(|v| v.as_str()) else {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, "Invalid key or value").into_response(),
            origin,
        );
    };
    let label = body
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or("Unlabeled")
        .to_string();

    let p = secrets_path(st);
    let mut secrets: Value = match std::fs::read_to_string(&p) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    };
    let obj = secrets.as_object_mut().unwrap();
    let arr = obj.entry(key.to_string()).or_insert_with(|| json!([]));
    if !arr.is_array() {
        *arr = json!([]);
    }
    let list = arr.as_array_mut().unwrap();
    deactivate_all(list);
    let id = uuid_v4();
    list.push(json!({
        "id": id,
        "value": value,
        "label": label,
        "active": true,
    }));
    // 原子写（0600 权限在 Windows 上由 NTFS 继承；POSIX 场景由 launched 环境决定）
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let pretty = serde_json::to_string_pretty(&secrets).unwrap_or_default();
    let tmp = p.with_extension("json.tmp");
    match std::fs::write(&tmp, &pretty).and_then(|_| std::fs::rename(&tmp, &p)) {
        Ok(()) => ok_json(origin, json!({ "id": id })),
        Err(e) => err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("secrets 写入失败：{e}"),
        ),
    }
}

/// POST /api/secrets/read —— secret_state 布尔表（api.ts 用它判断密钥已配）
/// （manifest 未列前端直接消费；ST /read 是 state 形态。native 无明文外泄面）
pub fn secrets_read(st: &BackendState, origin: Option<HeaderValue>) -> Response {
    let p = secrets_path(st);
    let Ok(text) = std::fs::read_to_string(&p) else {
        return ok_json(origin, json!({}));
    };
    let Ok(secrets) = serde_json::from_str::<Value>(&text) else {
        return ok_json(origin, json!({}));
    };
    let mut out = json!({});
    if let Some(obj) = secrets.as_object() {
        for (k, v) in obj {
            let has_active = v
                .as_array()
                .map(|a| {
                    a.iter()
                        .any(|s| s.get("active").and_then(|b| b.as_bool()).unwrap_or(false))
                })
                .unwrap_or(false);
            out.as_object_mut()
                .unwrap()
                .insert(k.clone(), json!(has_active));
        }
    }
    ok_json(origin, out)
}

/* ---------------- /api/secrets/view —— S8：后端不提供读回全量密钥 ---------------- */

/// POST /api/secrets/view：ST 默认 403（allowKeysExposure=false）。
/// native 后端**永不提供**该端点（评审 P0-3B/S8 硬性安全需求）。
pub fn secrets_view(origin: Option<HeaderValue>) -> Response {
    apply_cors_resp(
        (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": { "message": "native 后端不提供密钥明文读取（S8）" } })),
        )
            .into_response(),
        origin,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn new_chats() -> std::sync::Arc<super::super::chats::ChatsService> {
        std::sync::Arc::new(super::super::chats::ChatsService::new())
    }

    fn new_maid() -> std::sync::Arc<super::super::extras::MaidTokens> {
        std::sync::Arc::new(super::super::extras::MaidTokens::new())
    }
    use std::sync::Arc;

    fn st_with(tag: &str) -> BackendState {
        let root = std::env::temp_dir().join(format!("stchat-toksec-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        BackendState {
            token: Arc::new("t".into()),
            data_root: root,
            max_body_bytes: 1024,
            settings_svc: Arc::new(super::super::settings::SettingsService::new()),
            chats_svc: new_chats(),
            maid_tokens: new_maid(),
        }
    }

    #[test]
    fn guesstimate_matches_st_formula() {
        // ceil(bytes / 3.35)：中文 3 字节/字，ASCII 1 字节
        assert_eq!(guesstimate(""), 0);
        assert_eq!(guesstimate("abc"), 1, "3 bytes / 3.35 → ceil(0.895) = 1");
        assert_eq!(guesstimate("abcdefg"), 3, "7/3.35 → ceil(2.09) = 3");
        let cjk = "林晚棠"; // 9 字节
        assert_eq!(guesstimate(cjk), 3, "9/3.35 → ceil(2.69) = 3");
    }

    #[test]
    fn count_returns_token_count() {
        let st = st_with("count");
        let body = json!([{ "role": "system", "content": "hello world" }]).to_string();
        let r = count(&st, body.as_bytes(), None);
        assert_eq!(r.status().as_u16(), 200);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let n = v["token_count"].as_u64().unwrap();
        assert!(n > 0);
        // 空体 → 400
        let r = count(&st, b"", None);
        assert_eq!(r.status().as_u16(), 400);
    }

    #[test]
    fn encode_unregistered_family_is_501() {
        let r = encode_with_body("claude", b"{}", None);
        assert_eq!(r.status().as_u16(), 501);
    }

    #[test]
    fn encode_llama3_family_delegates() {
        // 有 body：llama3 走真 tokenizer → 200 + ids
        let r = encode_with_body("llama3", br#"{"text":"Hello"}"#, None);
        assert_eq!(r.status().as_u16(), 200);
    }

    #[test]
    fn secrets_write_multistate_and_rotation() {
        let st = st_with("sec");
        // 第一次写入
        let r = secrets_write(
            &st,
            &json!({ "key": "api_key_openai", "value": "sk-1" }),
            None,
        );
        assert_eq!(r.status().as_u16(), 200);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let id1 = v["id"].as_str().unwrap().to_string();
        assert_eq!(id1.len(), 36, "uuid 形态");

        // 落盘结构：数组 + active 标志
        let raw = std::fs::read_to_string(secrets_path(&st)).unwrap();
        let disk: Value = serde_json::from_str(&raw).unwrap();
        let arr = disk["api_key_openai"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["active"], true);
        assert_eq!(arr[0]["label"], "Unlabeled");

        // 轮换：新条目激活、旧条目失活（_deactivateAllSecrets）
        let r = secrets_write(
            &st,
            &json!({ "key": "api_key_openai", "value": "sk-2" }),
            None,
        );
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let id2 = v["id"].as_str().unwrap().to_string();
        assert_ne!(id1, id2);
        let raw = std::fs::read_to_string(secrets_path(&st)).unwrap();
        let disk: Value = serde_json::from_str(&raw).unwrap();
        let arr = disk["api_key_openai"].as_array().unwrap();
        assert_eq!(arr.len(), 2, "多值结构保留历史");
        assert_eq!(arr[0]["active"], false, "旧条目失活");
        assert_eq!(arr[1]["active"], true, "新条目激活");
        assert_ne!(arr[0]["id"], arr[1]["id"]);

        // 空值 → 400
        let r = secrets_write(&st, &json!({ "key": "k" }), None);
        assert_eq!(r.status().as_u16(), 400);
    }

    #[test]
    fn secrets_read_state_and_view_forbidden() {
        let st = st_with("secstate");
        secrets_write(
            &st,
            &json!({ "key": "api_key_claude", "value": "sk" }),
            None,
        );
        let r = secrets_read(&st, None);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["api_key_claude"], true, "布尔形态（不回明文）");
        assert!(!v.to_string().contains("sk"), "读回面无明文");

        // /view 恒 403（S8）
        let r = secrets_view(None);
        assert_eq!(r.status().as_u16(), 403);
    }

    #[test]
    fn uuid_v4_format() {
        let u = uuid_v4();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4", "version 位");
        assert!(matches!(&u[19..20], "8" | "9" | "a" | "b"), "variant 位");
    }
}
