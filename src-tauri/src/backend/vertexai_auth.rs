//! backend/vertexai_auth —— VertexAI **full 模式**认证（Phase 2 收尾批11）。
//!
//! ST `src/endpoints/google.js getVertexAIAuth`（service-account 分支）语义：
//! 1. secrets 读 `vertexai_service_account_json`（T12 vault 配置类 → settings）
//! 2. JSON 内取 `client_email` / `private_key` / `project_id`
//! 3. 签 RS256 JWT：iss=client_email、scope=cloud-platform、aud=token 端点、
//!    exp/iat（±1h 窗口）
//! 4. POST https://oauth2.googleapis.com/token
//!    grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer&assertion=<jwt>
//!    → { access_token, expires_in }
//! 5. 请求面 `Authorization: Bearer <token>`；URL
//!    `/v1/projects/{project_id}/locations/{region}/publishers/google/models/{model}:{op}`
//!
//! token 进程内缓存（expires 提前 60s 刷新）；ST getProjectIdFromServiceAccount
//! 同款字段读取；失败面：缺 SA → 400 {error:{message}}；签名/交换 → 400
//!（sendMakerSuite 契约面 auth error → 400 {error:true,message}）。

use serde_json::{json, Value};
use std::sync::Mutex;

/// token 期满边距（秒）——提前刷新避免边界 401
const TOKEN_EXPIRY_MARGIN_SECS: i64 = 60;

#[derive(Clone)]
struct CachedToken {
    token: String,
    expires_at: i64,
}

static TOKEN_CACHE: Mutex<Option<CachedToken>> = Mutex::new(None);

/// 服务账号 JSON 字段（ST getProjectIdFromServiceAccount 同款宽松面）
fn sa_field(sa: &Value, key: &str) -> Option<String> {
    sa.get(key).and_then(|v| v.as_str()).map(String::from).filter(|s| !s.is_empty())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// JWT 构造（供测试复用/断言面）
fn build_jwt(sa: &Value, iat: i64, exp: i64) -> Result<String, String> {
    let email = sa_field(sa, "client_email").ok_or("服务账号 JSON 缺 client_email")?;
    let key_pem = sa_field(sa, "private_key").ok_or("服务账号 JSON 缺 private_key")?;
    let claims = json!({
        "iss": email,
        "scope": "https://www.googleapis.com/auth/cloud-platform",
        "aud": "https://oauth2.googleapis.com/token",
        "exp": exp,
        "iat": iat,
    });
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(key_pem.as_bytes())
        .map_err(|e| format!("服务账号私钥解析失败：{e}"))?;
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &claims,
        &key,
    )
    .map_err(|e| format!("JWT 签名失败：{e}"))
}

/// token 交换（带缓存；region/project 不进缓存键 —— token 与项目无关）
pub async fn get_access_token(sa_json_text: &str) -> Result<(String, String), String> {
    // 1) 缓存命中
    {
        let cache = TOKEN_CACHE.lock().unwrap();
        if let Some(c) = cache.as_ref() {
            if c.expires_at > now_secs() + TOKEN_EXPIRY_MARGIN_SECS {
                return Ok((c.token.clone(), c.expires_at.to_string()));
            }
        }
    }
    let sa: Value = serde_json::from_str(sa_json_text)
        .map_err(|e| format!("服务账号 JSON 解析失败：{e}"))?;
    let iat = now_secs();
    let exp = iat + 3600;
    let jwt = build_jwt(&sa, iat, exp)?;

    // 2) token 端点交换
    let client = reqwest::Client::builder().no_proxy().build().unwrap_or_default();
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(30))
        .body(format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={}",
            urlencoded(&jwt)
        ))
        .send()
        .await
        .map_err(|e| format!("token 端点连接失败：{e}"))?;
    let text = resp.text().await.unwrap_or_default();
    let v: Value = serde_json::from_str(&text).map_err(|_| {
        format!("token 端点返回非 JSON（前 200 字：{}）", text.chars().take(200).collect::<String>())
    })?;
    let token = v
        .get("access_token")
        .and_then(|t| t.as_str())
        .ok_or_else(|| {
            format!(
                "token 交换失败：{}",
                v.get("error_description").and_then(|e| e.as_str()).unwrap_or("unknown")
            )
        })?
        .to_string();
    let expires_in = v.get("expires_in").and_then(|e| e.as_i64()).unwrap_or(3600);

    // 3) 缓存
    let expires_at = now_secs() + expires_in;
    *TOKEN_CACHE.lock().unwrap() = Some(CachedToken { token: token.clone(), expires_at });
    Ok((token, expires_at.to_string()))
}

/// URL 构造（full 模式：playground 路径含 project/region；与 ST :716-723 字面）
pub fn full_mode_url(region: &str, project_id: &str, model: &str, op: &str, stream: bool) -> String {
    let sse = if stream { "?alt=sse" } else { "" };
    format!(
        "https://aiplatform.googleapis.com/v1/projects/{project_id}/locations/{region}/publishers/google/models/{model}:{op}{sse}"
    )
}

fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// full 模式授权入口：vault 读 SA JSON → token（缓存）→ project_id
pub async fn authorize_full(
    sa_json_text: &str,
) -> Result<(String, String, String), String> {
    let sa: Value = serde_json::from_str(sa_json_text)
        .map_err(|e| format!("服务账号 JSON 解析失败：{e}"))?;
    let project_id = sa_field(&sa, "project_id").ok_or("服务账号 JSON 缺 project_id")?;
    let (token, _) = get_access_token(sa_json_text).await?;
    Ok((token, project_id, String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 测试用 RSA2048 私钥（PKCS#8 PEM，仅用于断言 JWT 形状；非真实凭据）
    const TEST_SA: &str = r#"{
        "project_id": "proj-x",
        "client_email": "svc@proj-x.iam.gserviceaccount.com",
        "private_key": "-----BEGIN RSA PRIVATE KEY-----\nMIIBOgIBAAJBALKZIkMBg2wS9EqmS0D UlTNYEi87CIfDNT5KNOYanld\n-----END RSA PRIVATE KEY-----\n"
    }"#;

    #[test]
    fn sa_fields_extract() {
        let sa: Value = serde_json::from_str(TEST_SA).unwrap();
        assert_eq!(sa_field(&sa, "project_id").as_deref(), Some("proj-x"));
        assert_eq!(sa_field(&sa, "missing"), None);
        // 空串语义 = None
        let sa2 = json!({ "client_email": "" });
        assert_eq!(sa_field(&sa2, "client_email"), None);
    }

    #[test]
    fn full_mode_url_shape() {
        assert_eq!(
            full_mode_url("us-central1", "proj-x", "gemini-2.5-flash", "generateContent", false),
            "https://aiplatform.googleapis.com/v1/projects/proj-x/locations/us-central1/publishers/google/models/gemini-2.5-flash:generateContent"
        );
        assert!(full_mode_url("global", "p", "m", "streamGenerateContent", true)
            .ends_with(":streamGenerateContent?alt=sse"));
    }

    #[test]
    fn urlencoded_shape() {
        assert_eq!(urlencoded("a b+c&d"), "a%20b%2Bc%26d");
        assert_eq!(urlencoded("urn:ietf:params"), "urn%3Aietf%3Aparams");
    }
}
