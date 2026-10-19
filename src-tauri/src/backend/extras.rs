//! backend/extras —— data-maid + translate + speech + vectors（Phase 1 族八 ·
//! manifest #54-#65）
//!
//! 契约核对（已逐行）：
//! - data-maid.js:653-810：/report → {report(9 组 sanitized 记录), token}（内存
//!   token 表：32 字节 hex，per-user 单 token，用后 finalize 204）；/view 是
//!   **GET+query token&hash**（浏览器 window.open）—— ⚠ S5：native 后端鉴权
//!   只走 X-Relay-Auth 头（query token 恒 403），view 端点在鉴权后面已天然
//!   封死 query 通路 —— 但路径必须**保留**（datamaid.ts:56 window.open）。
//! - translate.js：8 家供应商 POST {text, lang}。lingva/onering/google 等
//!   免密钥供应商直连上游；libre/deepl/deeplx 需服务端密钥。Phase 1 直连
//!   供应商面（密钥走 secrets）；未知 provider → 501。
//! - speech.js /synthesize（speecht5 本地 ML）→ S20 降级错误；/openai/
//!   generate-voice、/elevenlabs/synthesize 走供应商 API（xi-api-key 走 vault
//!   read_secret，上游锚点固定，缺 key 400）。E2：错误可读、不静默。
//! - vectors.js：vectra LocalIndex（`vectors/<source>/<collectionId>/<model>/
//!   index.json`：{version,metadata_config,items:[{vector,norm,metadata}]}）。
//!   嵌入来源 transformers 本地 ML / 供应商 API —— 一期前端已有三级回落；
//!   /list /insert /query /delete 均需真实向量，**无向量能力的构建返回
//!   S20 可判定错误**（评审 D5：禁止 200+空结果）。
//!   ⚠ vectra index.json 兼容读写已实现（ST 双端互通）；嵌入生成本身
//!   Phase 2 接供应商 API。

use super::{apply_cors_resp, BackendState};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

fn du(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user")
}

fn err_json(status: StatusCode, msg: &str) -> Response {
    log::warn!(target: super::logx::EXT, "extras 错误 {status}: {}", super::logx::redact(msg));
    (status, Json(json!({ "error": { "message": msg } }))).into_response()
}

fn ok_json(origin: Option<HeaderValue>, v: Value) -> Response {
    apply_cors_resp(Json(v).into_response(), origin)
}

/* ============================ data-maid（manifest #62-#65） ============================ */

/// 内存 token 表：handle → (token, paths)。ST TOKENS Map 语义。
#[derive(Default)]
pub struct MaidTokens {
    /// single user：handle 恒 default-user
    inner: Mutex<HashMap<String, MaidTokenEntry>>,
}

struct MaidTokenEntry {
    token: String,
    paths: Vec<(String, String)>, // (path, hash)
}

impl MaidTokens {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

/// 递归收集目录文件（平铺一层 + 子目录一层，覆盖 collectImages 的 user/images 子树）
fn list_files_flat(dir: &std::path::Path, recursive: bool, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_file() {
            out.push(p);
        } else if p.is_dir() && recursive {
            list_files_flat(&p, true, out);
        }
    }
}

/// 9 组收集器（对应 collectChats/collectGroupChats/thumbnails/backups；
/// images/files 的「已引用集合」简化为已知目录形态，不解析每条消息的
/// media 引用 —— 群聊/角色会话引用扫描在 Phase 2 接 vectors/翻译面时补齐）
fn collect_report(st: &BackendState) -> Value {
    let du = du(st);
    // chats：角色没有对应卡片 → 目录 loose
    let mut chats: Vec<String> = Vec::new();
    let known_char_stems: Vec<String> = {
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(du.join("characters")) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("png") {
                    v.push(
                        p.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string(),
                    );
                }
            }
        }
        v
    };
    if let Ok(rd) = std::fs::read_dir(du.join("chats")) {
        for d in rd.flatten() {
            let p = d.path();
            if !p.is_dir() {
                continue;
            }
            let stem = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if known_char_stems.iter().any(|s| s == stem) {
                continue;
            }
            if let Ok(files) = std::fs::read_dir(&p) {
                for f in files.flatten() {
                    if f.path().extension().and_then(|x| x.to_str()) == Some("jsonl") {
                        chats.push(f.path().to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    // group chats：不被 groups/*.json 的 chats[] 引用 → loose
    let mut group_chats: Vec<String> = Vec::new();
    let known_chat_ids: Vec<String> = {
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(du.join("groups")) {
            for e in rd.flatten() {
                if let Ok(text) = std::fs::read_to_string(e.path()) {
                    if let Ok(g) = serde_json::from_str::<Value>(&text) {
                        if let Some(chats) = g.get("chats").and_then(|c| c.as_array()) {
                            for c in chats {
                                if let Some(id) = c.as_str() {
                                    v.push(id.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        v
    };
    if let Ok(rd) = std::fs::read_dir(du.join("group chats")) {
        for e in rd.flatten() {
            let stem = e
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if known_chat_ids.iter().any(|s| s == &stem) {
                continue;
            }
            group_chats.push(e.path().to_string_lossy().to_string());
        }
    }
    // 缩略图三组：原图目录无同名 → loose
    let loose_thumbs = |thumb_sub: &str, originals: &std::path::Path| -> Vec<String> {
        let mut v = Vec::new();
        let dir = du.join("thumbnails").join(thumb_sub);
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !originals.join(&name).is_file() {
                    v.push(e.path().to_string_lossy().to_string());
                }
            }
        }
        v
    };
    let avatar_thumbs = loose_thumbs("avatar", &du.join("characters"));
    let bg_thumbs = loose_thumbs("bg", &du.join("backgrounds"));
    let persona_thumbs = loose_thumbs("persona", &du.join("User Avatars"));
    // 备份两组
    let mut chat_backups = Vec::new();
    let mut settings_backups = Vec::new();
    if let Ok(rd) = std::fs::read_dir(du.join("backups")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let p = e.path().to_string_lossy().to_string();
            if name.starts_with("chat_") {
                chat_backups.push(p);
            } else if name.starts_with("settings_") {
                settings_backups.push(p);
            }
        }
    }
    // images/files：user/images 与 user/files 全部文件（万一路由形态变化不影响报告）
    let mut images = Vec::new();
    list_files_flat(&du.join("user").join("images"), true, &mut images);
    let mut files = Vec::new();
    list_files_flat(&du.join("user").join("files"), true, &mut files);

    json!({
        "images": images.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
        "files": files.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
        "chats": chats,
        "groupChats": group_chats,
        "avatarThumbnails": avatar_thumbs,
        "backgroundThumbnails": bg_thumbs,
        "personaThumbnails": persona_thumbs,
        "chatBackups": chat_backups,
        "settingsBackups": settings_backups,
    })
}

/// sanitizeRecord（data-maid.js:139-153）：name/hash(sha256 全路径)/parent/size/mtime
fn sanitize_record(st: &BackendState, path_str: &str, with_parent: bool) -> Value {
    let p = std::path::Path::new(path_str);
    let stat = std::fs::metadata(p).ok();
    let mut rec = json!({
        "name": p.file_name().and_then(|s| s.to_str()).unwrap_or(""),
        "hash": sha256_hex(path_str),
    });
    let o = rec.as_object_mut().unwrap();
    if with_parent {
        o.insert(
            "parent".into(),
            json!(p
                .parent()
                .and_then(|x| x.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("")),
        );
    }
    if let Some(s) = stat {
        o.insert("size".into(), json!(s.len()));
        o.insert(
            "mtime".into(),
            json!(s
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)),
        );
    }
    let _ = st;
    rec
}

/// POST /api/data-maid/report {…}（manifest #62）
pub fn maid_report(
    st: &BackendState,
    tokens: &MaidTokens,
    origin: Option<HeaderValue>,
) -> Response {
    let raw = collect_report(st);
    // sanitize：全部组统一记录形态（withParent：images/chats=true，其余 false ——
    // data-maid.js:163-172 的 per-组 参数）
    let mut sanitized = json!({});
    for (k, v) in raw.as_object().unwrap() {
        let with_parent = matches!(k.as_str(), "images" | "chats");
        let arr: Vec<Value> = v
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| sanitize_record(st, s.as_str().unwrap_or(""), with_parent))
                    .collect()
            })
            .unwrap_or_default();
        sanitized
            .as_object_mut()
            .unwrap()
            .insert(k.clone(), json!(arr));
    }
    // token：内存表 + 全 raw 路径注册
    let mut token_bytes = [0u8; 32];
    getrandom::getrandom(&mut token_bytes).expect("CSPRNG");
    let token: String = token_bytes.iter().map(|b| format!("{b:02x}")).collect();
    let paths: Vec<(String, String)> = raw
        .as_object()
        .unwrap()
        .values()
        .filter_map(|v| v.as_array())
        .flatten()
        .filter_map(|s| s.as_str())
        .map(|p| (p.to_string(), sha256_hex(p)))
        .collect();
    {
        let mut t = tokens.inner.lock().unwrap_or_else(|e| e.into_inner());
        t.clear(); // ST：同 handle 旧 token 移除
        t.insert(
            "default-user".into(),
            MaidTokenEntry {
                token: token.clone(),
                paths,
            },
        );
    }
    let total: usize = sanitized
        .as_object()
        .map(|o| {
            o.values()
                .filter_map(|v| v.as_array())
                .map(|a| a.len())
                .sum()
        })
        .unwrap_or(0);
    log::info!(target: super::logx::EXT, "data-maid/report 孤儿文件 {total} 项");
    ok_json(origin, json!({ "report": sanitized, "token": token }))
}

/// token 校验 helper：不存在 403 / 拿到 entry 克隆
fn check_token(
    tokens: &MaidTokens,
    token: &str,
) -> Result<(String, Vec<(String, String)>), StatusCode> {
    let t = tokens.inner.lock().unwrap_or_else(|e| e.into_inner());
    match t.get("default-user") {
        Some(e) if e.token == token => Ok((e.token.clone(), e.paths.clone())),
        _ => Err(StatusCode::FORBIDDEN),
    }
}

/// POST /api/data-maid/finalize {token}（manifest #63）—— 用后失效 → 204
pub fn maid_finalize(
    st: &BackendState,
    tokens: &MaidTokens,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let _ = st;
    let Some(token) = body.get("token").and_then(|v| v.as_str()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    match check_token(tokens, token) {
        Ok(_) => {
            tokens
                .inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove("default-user");
            apply_cors_resp(StatusCode::NO_CONTENT.into_response(), origin)
        }
        Err(code) => apply_cors_resp(code.into_response(), origin),
    }
}

/// POST /api/data-maid/delete {token, hashes[]}（manifest #65）—— 逐 hash 删除 → 204
pub fn maid_delete(
    st: &BackendState,
    tokens: &MaidTokens,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let (Some(token), Some(hashes)) = (
        body.get("token").and_then(|v| v.as_str()),
        body.get("hashes").and_then(|v| v.as_array()),
    ) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let paths = match check_token(tokens, token) {
        Ok((_, paths)) => paths,
        Err(code) => return apply_cors_resp(code.into_response(), origin),
    };
    // containment：数据根内才删（ST data-maid.js:793-797 isPathUnderParent）
    let root = du(st);
    let mut deleted = 0usize;
    for h in hashes {
        if let Some(h) = h.as_str() {
            if let Some((path, _)) = paths.iter().find(|(_, hash)| hash == h) {
                let p = std::path::Path::new(path);
                if p.starts_with(&root) && p.is_file() {
                    let _ = std::fs::remove_file(p);
                    deleted += 1;
                }
            }
        }
    }
    log::info!(target: super::logx::EXT, "data-maid/delete 请求 {} 项，实际删除 {deleted} 项", hashes.len());
    apply_cors_resp(StatusCode::NO_CONTENT.into_response(), origin)
}

/// GET /api/data-maid/view?token=&hash=（manifest #64）
/// ⚠ 鉴权已由 authorize 统一处理（S5：query token 不参与鉴权）；
/// 这里的 token 是 maid 内存表的一次性凭证。mime 按扩展名。
pub fn maid_view(
    _st: &BackendState,
    tokens: &MaidTokens,
    query: &str,
    origin: Option<HeaderValue>,
) -> Response {
    let mut token = "";
    let mut hash = "";
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        match (kv.next(), kv.next()) {
            (Some("token"), Some(v)) => token = v,
            (Some("hash"), Some(v)) => hash = v,
            _ => {}
        }
    }
    if token.is_empty() || hash.is_empty() {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    let paths = match check_token(tokens, token) {
        Ok((_, paths)) => paths,
        Err(code) => return apply_cors_resp(code.into_response(), origin),
    };
    let Some((path, _)) = paths.iter().find(|(_, h)| h == hash) else {
        return apply_cors_resp(StatusCode::NOT_FOUND.into_response(), origin);
    };
    let p = std::path::Path::new(path);
    if !p.is_file() {
        return apply_cors_resp(StatusCode::NOT_FOUND.into_response(), origin);
    }
    // mime 按扩展名（data-maid.js:761 mimetype guess → 回落 text/plain）。
    // owned String 构造 HeaderValue —— 不得 Box::leak（每次请求泄漏一段静态内存）
    let ext = p
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => format!("image/{ext}"),
        "json" | "jsonl" => "application/json".to_string(),
        _ => "text/plain".to_string(),
    };
    match std::fs::read(p) {
        Ok(bytes) => {
            let mut resp = (StatusCode::OK, bytes).into_response();
            let ct = HeaderValue::from_str(&mime)
                .unwrap_or_else(|_| HeaderValue::from_static("text/plain"));
            resp.headers_mut()
                .insert(axum::http::header::CONTENT_TYPE, ct);
            apply_cors_resp(resp, origin)
        }
        Err(_) => apply_cors_resp(StatusCode::INTERNAL_SERVER_ERROR.into_response(), origin),
    }
}

/* ====================== translate（manifest #54-#55）====================== */

// 注：翻译供应商分发在 translate.rs::dispatch（8 家全量出站，未知 provider → 400）。
// 原 translate_not_migrated 占位（仅剩测试引用）已删除 —— 死代码清理。

/* ====================== speech（manifest #56-#58）====================== */

/// POST /api/openai/generate-voice（manifest #58 · Phase 2 批6 出站）
/// ST openai.js:273-318 字面：body 采子集 {input, response_format:'mp3',
/// voice ?? 'alloy', speed ?? 1, model ?? 'tts-1'} + 可选 instructions
/// → api.openai.com/v1/audio/speech（Bearer api_key_openai）。
/// 成功 → audio/mpeg 二进制；上游失败 → 500 + 上游文本；缺 key → 400。
pub async fn speech_openai(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(key) = super::providers::read_secret(st, "api_key_openai") else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let text = body.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
    if text.is_empty() {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    let mut payload = json!({
        "input": text,
        "response_format": "mp3",
        "voice": body.get("voice").and_then(|v| v.as_str()).unwrap_or("alloy"),
        "speed": body.get("speed").and_then(|v| v.as_f64()).unwrap_or(1.0),
        "model": body.get("model").and_then(|v| v.as_str()).unwrap_or("tts-1"),
    });
    if let Some(instr) = body.get("instructions") {
        payload.as_object_mut().unwrap().insert("instructions".into(), instr.clone());
    }
    let client = reqwest::Client::builder().no_proxy().build().unwrap_or_default();
    let resp = client
        .post("https://api.openai.com/v1/audio/speech")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {key}"))
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(120))
        .json(&payload)
        .send()
        .await;
    let Ok(resp) = resp else {
        return apply_cors_resp(
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response(),
            origin,
        );
    };
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return apply_cors_resp(
            (StatusCode::INTERNAL_SERVER_ERROR, text).into_response(),
            origin,
        );
    }
    let bytes = resp.bytes().await.unwrap_or_default();
    apply_cors_resp(
        (
            [(axum::http::header::CONTENT_TYPE, HeaderValue::from_static("audio/mpeg"))],
            bytes,
        ).into_response(),
        origin,
    )
}

/// POST /api/speech/elevenlabs/synthesize {voiceId, request}（HIGH-4 · manifest #57）
/// 契约 ST speech.js:234-272（已逐行核对）+ 前端 speech.ts:141-156：
/// - key 走 vault read_secret（secrets.js:68 ELEVENLABS = api_key_elevenlabs）；缺 key → 400
/// - voiceId / request 缺失 → 400；request 必须是 JSON 对象
/// - 上游锚点固定 https://api.elevenlabs.io/v1/text-to-speech/<voiceId>，xi-api-key 头
/// - 成功 → audio/mpeg 音频流透传；上游失败 → 500 + 上游可读文本（E2：不静默）
pub async fn speech_elevenlabs(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let Some(key) = super::providers::read_secret(st, "api_key_elevenlabs") else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let Some(voice_id) = body
        .get("voiceId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let Some(request) = body.get("request") else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    if !request.is_object() {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    let url = match elevenlabs_synthesize_url(voice_id) {
        Ok(u) => u,
        Err(_) => return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin),
    };
    let client = reqwest::Client::builder().no_proxy().build().unwrap_or_default();
    let resp = client
        .post(&url)
        .header("xi-api-key", &key)
        .header("Content-Type", "application/json")
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(120))
        .json(&request)
        .send()
        .await;
    let Ok(resp) = resp else {
        return apply_cors_resp(
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response(),
            origin,
        );
    };
    if !resp.status().is_success() {
        // 上游错误可读：原文本透传（ST 是裸 500，这里按 E2 要求保留可读面）
        let text = resp.text().await.unwrap_or_default();
        return apply_cors_resp(
            (StatusCode::INTERNAL_SERVER_ERROR, text).into_response(),
            origin,
        );
    }
    let bytes = resp.bytes().await.unwrap_or_default();
    apply_cors_resp(
        (
            [(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("audio/mpeg"),
            )],
            bytes,
        )
            .into_response(),
        origin,
    )
}

/// 上游锚点构造（单一出口）：`https://api.elevenlabs.io/v1/text-to-speech/<voiceId>`。
/// voiceId 过 pathguard::component —— 含分隔符/控制字符的注入段一律拒绝。
fn elevenlabs_synthesize_url(voice_id: &str) -> Result<String, ()> {
    let safe = crate::pathguard::component(voice_id).map_err(|_| ())?;
    Ok(format!("https://api.elevenlabs.io/v1/text-to-speech/{safe}"))
}

/// POST /api/speech/synthesize（本地 ML）—— S20 降级
pub fn speech_local_unavailable(origin: Option<HeaderValue>) -> Response {
    log::warn!(target: super::logx::EXT, "speech/synthesize 本地引擎不可用 → 501（S20 降级，非伪成功）");
    apply_cors_resp(
        (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": { "message": "本地语音引擎（speecht5/transformers）尚未迁移：请选用 system/openai/elevenlabs 供应商", "code": "CAPABILITY_UNAVAILABLE" }
            })),
        )
            .into_response(),
        origin,
    )
}

/* ====================== vectors（manifest #59-#61）====================== */

fn vectors_dir(st: &BackendState) -> std::path::PathBuf {
    du(st).join("vectors")
}

/// vectra index.json 读写（ LocalIndex.js:104-109/163：{version, metadata_config, items}）
/// Item 形态：{vector, norm, metadata:{hash, text, index}}
struct VectraIndex {
    path: std::path::PathBuf,
    data: Value,
}

impl VectraIndex {
    fn open(
        vectors_root: &std::path::Path,
        source: &str,
        collection: &str,
        model_dir: &str,
    ) -> Result<Self, String> {
        let dir = vectors_root.join(source).join(collection).join(model_dir);
        let path = dir.join("index.json");
        if path.is_file() {
            let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let data = serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())?;
            Ok(Self { path, data })
        } else {
            let data = json!({
                "version": 1,
                "metadata_config": {},
                "items": [],
            });
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::write(&path, data.to_string()).map_err(|e| e.to_string())?;
            Ok(Self { path, data })
        }
    }

    fn items(&self) -> Vec<Value> {
        self.data
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    }

    fn save(&self) -> Result<(), String> {
        std::fs::write(&self.path, self.data.to_string()).map_err(|e| e.to_string())
    }
}

/// POST /api/vector/list {collectionId, source} → hash 数组（manifest #59）
/// ⚠ 嵌入生成（insert 时的向量计算）是 Phase 2；已有 index（ST 双写遗留）
/// 可直接读 —— 空集合返回 []（诚实空，insert 走不通见下）。
pub fn vector_list(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(collection) = body.get("collectionId").and_then(|v| v.as_str()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let source = body
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("transformers");
    let Ok(col) = crate::pathguard::component(collection) else {
        return apply_cors_resp(StatusCode::FORBIDDEN.into_response(), origin);
    };
    let Ok(src) = crate::pathguard::component(source) else {
        return apply_cors_resp(StatusCode::FORBIDDEN.into_response(), origin);
    };
    let index = match VectraIndex::open(&vectors_dir(st), &src, &col, "default") {
        Ok(i) => i,
        Err(e) => {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("vector index 打开失败：{e}"),
            )
        }
    };
    let hashes: Vec<Value> = index
        .items()
        .iter()
        .filter_map(|it| it.get("metadata").and_then(|m| m.get("hash")).cloned())
        .collect();
    ok_json(origin, json!(hashes))
}

/// POST /api/vector/insert {collectionId, source, items[]}（manifest #60）
/// items 需要嵌入向量 → Phase 2（供应商嵌入 API / 本地 ML）。
/// **S20/D5：返回可判定错误而非 200+空成功。**
pub fn vector_insert(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let _ = st;
    let has_items = body
        .get("items")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if !has_items || body.get("collectionId").is_none() {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    let resp = (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": "向量嵌入生成尚未接入（随 Phase 2 供应商嵌入 API 接通）",
                "code": "NOT_MIGRATED",
            }
        })),
    );
    apply_cors_resp(resp.into_response(), origin)
}

/// POST /api/vector/query {collectionId, searchText, topK, source, threshold}
/// （manifest #61）—— S20 对齐整改（与 vector_insert 同错误面）：
/// 查询需要把文本转向量，嵌入能力未接入前**没有可判定的查询语义**——
/// 原「按 metadata hash 序返回 topK + degraded 标记」是无效排序（相似度不可判），
/// 伪装成降级成功，违反 D5「禁止 200+空/伪结果」。现返回 501 + 可判定错误。
pub fn vector_query(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let _ = st;
    let Some(collection) = body.get("collectionId").and_then(|v| v.as_str()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    if body
        .get("searchText")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .is_empty()
    {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    }
    let source = body
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("transformers");
    if crate::pathguard::component(collection).is_err()
        || crate::pathguard::component(source).is_err()
    {
        return apply_cors_resp(StatusCode::FORBIDDEN.into_response(), origin);
    }
    let resp = (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": "向量嵌入生成尚未接入（随 Phase 2 供应商嵌入 API 接通），查询暂不可用",
                "code": "CAPABILITY_UNAVAILABLE",
            }
        })),
    );
    apply_cors_resp(resp.into_response(), origin)
}

/// POST /api/vector/delete {collectionId, hashes} —— 直接按 hash 删（无需嵌入）
pub fn vector_delete(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(collection) = body.get("collectionId").and_then(|v| v.as_str()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let Some(hashes) = body.get("hashes").and_then(|v| v.as_array()) else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let source = body
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("transformers");
    let Ok(col) = crate::pathguard::component(collection) else {
        return apply_cors_resp(StatusCode::FORBIDDEN.into_response(), origin);
    };
    let Ok(src) = crate::pathguard::component(source) else {
        return apply_cors_resp(StatusCode::FORBIDDEN.into_response(), origin);
    };
    let mut index = match VectraIndex::open(&vectors_dir(st), &src, &col, "default") {
        Ok(i) => i,
        Err(e) => {
            return err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("vector index 打开失败：{e}"),
            )
        }
    };
    let items = index.data.get_mut("items").and_then(|v| v.as_array_mut());
    if let Some(items) = items {
        items.retain(|it| {
            let h = it.get("metadata").and_then(|m| m.get("hash")).cloned();
            !hashes.contains(&h.unwrap_or(Value::Null))
        });
        if index.save().is_err() {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, "vector index 保存失败");
        }
    }
    apply_cors_resp(StatusCode::NO_CONTENT.into_response(), origin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_support::TestBackend;
    fn new_chats() -> std::sync::Arc<super::super::chats::ChatsService> {
        std::sync::Arc::new(super::super::chats::ChatsService::new())
    }

    fn new_maid() -> std::sync::Arc<super::super::extras::MaidTokens> {
        std::sync::Arc::new(super::super::extras::MaidTokens::new())
    }
    use std::sync::Arc;

    fn st_with(tag: &str) -> TestBackend {
        let tmp = TestBackend::temp(&format!("stoc-extras-{tag}-"));
        let root = tmp.path().to_path_buf();
        TestBackend::new(tmp, BackendState {
            token: Arc::new("t".into()),
            data_root: root,
            max_body_bytes: 1024,
            settings_svc: Arc::new(super::super::settings::SettingsService::new()),
            chats_svc: new_chats(),
            maid_tokens: new_maid(),
          vault: std::sync::Arc::new(super::super::vault::Vault::new_memory()),
        })
    }

    #[test]
    fn maid_report_and_delete_flow() {
        let st = st_with("maid");
        let du = du(&st);
        // 造 loose 文件：无对应卡片的 chats 目录 + 备份
        std::fs::create_dir_all(du.join("chats").join("loose-char")).unwrap();
        std::fs::write(du.join("chats").join("loose-char").join("c.jsonl"), "{}\n").unwrap();
        std::fs::create_dir_all(du.join("backups")).unwrap();
        std::fs::write(du.join("backups").join("chat_x_1.jsonl"), "{}").unwrap();
        // 有卡片的目录（不 loose）
        std::fs::create_dir_all(du.join("characters")).unwrap();
        std::fs::write(
            du.join("characters").join("known.png"),
            [0x89u8, b'P', b'N', b'G'],
        )
        .unwrap();
        std::fs::create_dir_all(du.join("chats").join("known")).unwrap();
        std::fs::write(du.join("chats").join("known").join("k.jsonl"), "{}\n").unwrap();

        let tokens = MaidTokens::new();
        let r = maid_report(&st, &tokens, None);
        assert_eq!(r.status().as_u16(), 200);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let report = &v["report"];
        assert_eq!(
            report["chats"].as_array().unwrap().len(),
            1,
            "loose 会话目录检出"
        );
        assert_eq!(report["chatBackups"].as_array().unwrap().len(), 1);
        let token = v["token"].as_str().unwrap().to_string();

        // 按 hash 删除 loose 文件
        let hash = report["chats"][0]["hash"].as_str().unwrap().to_string();
        let r = maid_delete(
            &st,
            &tokens,
            &json!({ "token": token, "hashes": [hash] }),
            None,
        );
        assert_eq!(r.status().as_u16(), 204);
        assert!(!du.join("chats").join("loose-char").join("c.jsonl").exists());

        // 错 token → 403
        let r = maid_finalize(&st, &tokens, &json!({ "token": "bogus" }), None);
        assert_eq!(r.status().as_u16(), 403);

        // list_urls/data URI 不可行：known 会话不检
        let r = maid_report(&st, &tokens, None);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            v["report"]["chats"].as_array().unwrap().len(),
            0,
            "known 会话不是 loose"
        );
    }

    #[test]
    fn maid_view_serves_by_hash() {
        let st = st_with("maidview");
        let du = du(&st);
        std::fs::create_dir_all(du.join("backups")).unwrap();
        std::fs::write(du.join("backups").join("chat_l_1.jsonl"), "{\"mes\":\"x\"}").unwrap();
        let tokens = MaidTokens::new();
        let r = maid_report(&st, &tokens, None);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let token = v["token"].as_str().unwrap().to_string();
        let hash = v["report"]["chatBackups"][0]["hash"]
            .as_str()
            .unwrap()
            .to_string();

        let r = maid_view(&st, &tokens, &format!("token={token}&hash={hash}"), None);
        assert_eq!(r.status().as_u16(), 200);
        assert_eq!(
            r.headers()
                .get("content-type")
                .and_then(|c| c.to_str().ok()),
            Some("application/json")
        );
        // 错 hash → 404；错 token → 403
        let r = maid_view(&st, &tokens, &format!("token={token}&hash=dead"), None);
        assert_eq!(r.status().as_u16(), 404);
        let r = maid_view(&st, &tokens, "token=zz&hash=1", None);
        assert_eq!(r.status().as_u16(), 403);
    }

    #[test]
    fn vector_list_and_delete_work_with_st_index() {
        let st = st_with("vec");
        // 手工构造 ST vectra 双写遗留 index
        let dir = du(&st)
            .join("vectors")
            .join("openai")
            .join("coll-1")
            .join("default");
        std::fs::create_dir_all(&dir).unwrap();
        let index = json!({
            "version": 1,
            "metadata_config": {},
            "items": [
                { "vector": [1.0], "norm": 1.0, "metadata": { "hash": 111, "text": "a", "index": 0 } },
                { "vector": [2.0], "norm": 2.0, "metadata": { "hash": 222, "text": "b", "index": 1 } },
            ],
        });
        std::fs::write(dir.join("index.json"), index.to_string()).unwrap();

        let r = vector_list(
            &st,
            &json!({ "collectionId": "coll-1", "source": "openai" }),
            None,
        );
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v, json!([111, 222]));

        // insert → 501（S20）
        let r = vector_insert(
            &st,
            &json!({ "collectionId": "c", "items": [{ "hash": 1 }] }),
            None,
        );
        assert_eq!(r.status().as_u16(), 501);

        // delete by hash
        let r = vector_delete(
            &st,
            &json!({ "collectionId": "coll-1", "source": "openai", "hashes": [111] }),
            None,
        );
        assert_eq!(r.status().as_u16(), 204);
        let r = vector_list(
            &st,
            &json!({ "collectionId": "coll-1", "source": "openai" }),
            None,
        );
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v, json!([222]));
    }

    #[test]
    fn speech_degrade_honestly() {
        let r = speech_local_unavailable(None);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            v["error"]["code"], "CAPABILITY_UNAVAILABLE",
            "S20 降级可判定"
        );
    }

    #[test]
    fn elevenlabs_url_anchor_is_fixed() {
        // HIGH-4 锚点：上游 URL 单一出口，host/path 固定，voiceId 只能是干净单段
        let u = elevenlabs_synthesize_url("21m00Tcm4TlvDq8ikWAM").unwrap();
        assert_eq!(
            u,
            "https://api.elevenlabs.io/v1/text-to-speech/21m00Tcm4TlvDq8ikWAM"
        );
        // voiceId 注入段拒绝（URL/路径注入向量）
        assert!(elevenlabs_synthesize_url("../v1/voices").is_err());
        assert!(elevenlabs_synthesize_url("a/b").is_err());
        assert!(elevenlabs_synthesize_url("a\\b").is_err());
        assert!(elevenlabs_synthesize_url("").is_err());
        // body 里塞整 URL 也不影响锚点 —— URL 只由 voiceId 构造
        assert_eq!(
            elevenlabs_synthesize_url("voice-1").unwrap(),
            "https://api.elevenlabs.io/v1/text-to-speech/voice-1"
        );
    }

    #[tokio::test]
    async fn elevenlabs_faces_without_key_and_params() {
        let st = st_with("elxlabs");
        // 缺 key → 400（ST speech.js:237-240）
        let r = speech_elevenlabs(
            &st,
            &json!({ "voiceId": "v1", "request": { "text": "hi" } }),
            None,
        )
        .await;
        assert_eq!(r.status().as_u16(), 400, "缺 key 必须 400");

        // 注入 key（镜像 secrets.json active 条目 + memory keychain 各一：
        // read_active 以镜像定位 id）后：缺参仍 400（都在出站前判定，不发网络）
        std::fs::create_dir_all(du(&st)).unwrap();
        std::fs::write(
            du(&st).join("secrets.json"),
            json!({ "api_key_elevenlabs": [
                { "id": "default", "value": "sk-test", "label": "t", "active": true }
            ] })
            .to_string(),
        )
        .unwrap();
        st.vault
            .store_sync(&st, "api_key_elevenlabs", "default", "sk-test");
        let r = speech_elevenlabs(&st, &json!({ "request": { "text": "hi" } }), None).await;
        assert_eq!(r.status().as_u16(), 400, "缺 voiceId → 400");
        let r = speech_elevenlabs(&st, &json!({ "voiceId": "v1" }), None).await;
        assert_eq!(r.status().as_u16(), 400, "缺 request → 400");
        let r = speech_elevenlabs(
            &st,
            &json!({ "voiceId": "v1", "request": "not-an-object" }),
            None,
        )
        .await;
        assert_eq!(r.status().as_u16(), 400, "request 非对象 → 400");
        // voiceId 注入段 → 400（不出站）
        let r = speech_elevenlabs(
            &st,
            &json!({ "voiceId": "../../admin", "request": { "text": "hi" } }),
            None,
        )
        .await;
        assert_eq!(r.status().as_u16(), 400, "voiceId 注入 → 400");
    }

    #[test]
    fn vector_query_is_501_capability_unavailable() {
        // S20 对齐：查询无嵌入能力 = 无可判定语义，禁止 200+degraded 伪成功
        let st = st_with("vecq");
        let r = vector_query(
            &st,
            &json!({ "collectionId": "c1", "searchText": "hello", "topK": 5 }),
            None,
        );
        assert_eq!(r.status().as_u16(), 501);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], "CAPABILITY_UNAVAILABLE");
        // 缺参仍 400
        let r = vector_query(&st, &json!({ "collectionId": "c1" }), None);
        assert_eq!(r.status().as_u16(), 400);
        // 注入 collectionId → 403
        let r = vector_query(
            &st,
            &json!({ "collectionId": "../x", "searchText": "hi" }),
            None,
        );
        assert_eq!(r.status().as_u16(), 403);
    }
}
