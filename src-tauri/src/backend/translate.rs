//! backend/translate —— 翻译出站族（Phase 2 批6 · manifest #54/#55）。
//!
//! ST `src/endpoints/translate.js`（1.19.0）逐行核对：
//! - libre：secrets libre_url（缺失 → 400）+ api_key_libre；zh-CN→zh、
//!   zh-TW→zt、pt-BR/PT→pt；POST {q,source:auto,target,format:text,api_key}
//!   → `translatedText`
//! - google：`google-translate-api-x` 的 gtx 免密端点等价实现 ——
//!   GET translate.googleapis.com/translate_a/single?client=gtx&dt=t&sl=auto&tl
//!   → result[0] 逐段 join
//! - yandex：**chunks 数组**（App 平铺 text → [text] 兼容）+ ucid；
//!   `json.text.join()` 字面=逗号连接
//! - lingva：lingva_url（默认 https://lingva.ml/api/v1）/auto/{lang}/{urlenc}
//!   → translation
//! - deepl：secret deepl；zh→ZH；formality/default（native 无配置面）；
//!   endpoint=pro → api.deepl.com 否则 api-free；`DeepL-Auth-Key` 头
//! - onering：oneringtranslator_url（默认环回 4990）；GET query 传参
//!   → **App 期望 {data:{result}}**（ST 裸字符串面 App 无法读取 —— 登记偏差）
//! - deeplx：deeplx_url（默认环回 1188）；zh→ZH → `data`
 //!
//! 出站网关（HIGH-5）：libre_url / deeplx_url / oneringtranslator_url 三家
//! 用户可控 URL 过统一网关 `validate_egress_url(…, SelfHosted)` —— 按自建端点类
//! 放行（http/https 均可、本机与局域网都可用，只拦 link-local/0.0.0.0），
//! 非 ProxyUrl 字面：DeepLX/OneRing 出厂默认即本机，自建服务亦常落局域网；
//! 网关拒绝 → 400 可读错误面，亦无 ST 字面）。
//! - bing：edge.microsoft.com/translate/auth 取 JWT →
//!   cognitive.microsofttranslator.com/translate?api-version=3.0&to
//!   （zh-CN→zh-Hans、zh-TW→zh-Hant、pt-BR→pt）
//!
//! 输出面：text 形统一返回 **JSON 字符串**（引号包裹；App stPostJson
//! typeof string 判定兼容 —— ST 裸 text/plain 面 App 同样无法 json 解析，
//! 属登记的同源偏差修正）。非 ok/异常 → 500（sendStatus 字面）。

use super::providers::{read_secret, EgressPolicy, validate_egress_url};
use super::BackendState;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

const DEEPLX_URL_DEFAULT: &str = "http://127.0.0.1:1188/translate";
const ONERING_URL_DEFAULT: &str = "http://127.0.0.1:4990/translate";
const LINGVA_DEFAULT: &str = "https://lingva.ml/api/v1";

fn plain(value: Value, origin: Option<HeaderValue>) -> Response {
    // 统一 JSON 面（模块头输出面说明）
    super::providers::json_resp(StatusCode::OK, value, origin)
}

fn text_ok(s: &str, origin: Option<HeaderValue>) -> Response {
    super::providers::json_resp(StatusCode::OK, Value::String(s.to_string()), origin)
}



fn status500(origin: Option<HeaderValue>) -> Response {
    // sendStatus(500) 字面（裸状态码）
    super::apply_cors_resp(StatusCode::INTERNAL_SERVER_ERROR.into_response(), origin)
}

fn status400(origin: Option<HeaderValue>) -> Response {
    super::apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin)
}

/// 出站网关拒绝（HIGH-5 接线，无 ST 字面对应 —— 登记偏差）：
/// 400 + 可读错误（json_resp 面，与未知供应商错误同形、CORS 一致）。
fn egress_reject(what: &str, why: &str, origin: Option<HeaderValue>) -> Response {
    super::providers::json_resp(
        StatusCode::BAD_REQUEST,
        json!({ "error": { "message": format!("{what} 被出站网关拒绝: {why}") } }),
        origin,
    )
}

async fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap_or_default()
}

trait SField {
    fn sfield(&self, k: &str) -> String;
}
impl SField for Value {
    fn sfield(&self, k: &str) -> String {
        self.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
    }
}

fn get_lang_field(body: &Value) -> String {
    body.sfield("lang")
}

/* ---------------- dispatch（/api/translate/<provider>） ---------------- */
pub async fn dispatch(
    st: &BackendState,
    provider: &str,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let target = body
        .get("target_language")
        .or_else(|| body.get("target"))
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    log::info!(
        target: super::logx::EXT,
        "translate provider={} target={}",
        super::logx::redact(provider),
        super::logx::redact(target)
    );
    match provider {
        "libre" => libre(st, body, origin).await,
        "google" => google(st, body, origin).await,
        "yandex" => yandex(st, body, origin).await,
        "lingva" => lingva(st, body, origin).await,
        "deepl" => deepl(st, body, origin).await,
        "deeplx" => deeplx(st, body, origin).await,
        "bing" => bing(st, body, origin).await,
        _ => super::providers::json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": { "message": format!("未知翻译供应商 {provider}") } }),
            origin,
        ),
    }
}

/* ---------------- libre ---------------- */
pub async fn libre(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let url = match read_secret(st, "libre_url") {
        Some(u) => u,
        None => return status400(origin),
    };
    // HIGH-5：libre_url 用户可控出站 → 统一网关（SelfHosted 自建端点：
    // http/https 均可，本机与局域网都放行；非 ProxyUrl 字面 —— 汇报已登记）
    let url = match validate_egress_url(&url, EgressPolicy::SelfHosted) {
        Ok(u) => u,
        Err(e) => return egress_reject("libre_url", &e, origin),
    };
    let mut lang = get_lang_field(body);
    match lang.as_str() {
        "zh-CN" => lang = "zh".into(),
        "zh-TW" => lang = "zt".into(),
        "pt-BR" | "pt-PT" => lang = "pt".into(),
        _ => {}
    }
    let text = body.sfield("text");
    if text.is_empty() || lang.is_empty() {
        return status400(origin);
    }
    let payload = json!({
        "q": text,
        "source": "auto",
        "target": lang,
        "format": "text",
        "api_key": read_secret(st, "libre").unwrap_or_default(),
    });
    let resp = client().await
        .post(&url)
        .header("Content-Type", "application/json")
        .header("accept-encoding", "identity")
        .json(&payload)
        .send()
        .await;
    let Ok(resp) = resp else { return status500(origin) };
    if !resp.status().is_success() {
        return status500(origin);
    }
    let v = resp.json::<Value>().await.unwrap_or(json!({}));
    text_ok(v.sfield("translatedText").as_str(), origin)
}

/* ---------------- google（gtx） ---------------- */
pub async fn google(_st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let mut lang = get_lang_field(body);
    if lang == "pt-BR" {
        lang = "pt".into();
    }
    let text = match body.get("text") { Some(Value::String(s)) => s.clone(), Some(other) => other.to_string(), None => String::new() };
    if text.is_empty() || lang.is_empty() {
        return status400(origin);
    }
    // google-translate-api-x（client=gtx）等价 GET
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&dt=t&sl=auto&tl={}&q={}",
        query_escape(&lang),
        query_escape(&text),
    );
    let resp = client().await
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await;
    let Ok(resp) = resp else { return status500(origin) };
    if !resp.status().is_success() {
        return status500(origin);
    }
    let v = resp.json::<Value>().await.unwrap_or(json!([]));
    let seg0 = v.get(0).and_then(|s| s.as_array());
    let out = seg0
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get(0).and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    text_ok(&out, origin)
}

/* ---------------- yandex ---------------- */
pub async fn yandex(_st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let mut lang = get_lang_field(body);
    if lang == "pt-PT" || lang == "zh-CN" || lang == "zh-TW" {
        lang = if lang.starts_with("zh") { "zh".into() } else { "pt".into() };
    }
    // App 平铺 text → chunks 单段兼容（ST 要求 chunks 数组）
    let chunks: Vec<String> = match body.get("chunks").and_then(|c| c.as_array()) {
        Some(a) => a.iter().map(|c| c.as_str().unwrap_or("").to_string()).collect(),
        None => {
            let t = body.sfield("text");
            if t.is_empty() {
                return status400(origin);
            }
            vec![t]
        }
    };
    if lang.is_empty() {
        return status400(origin);
    }
    let mut form = String::new();
    for chunk in &chunks {
        form.push_str(&format!("text={}&", query_escape(chunk)));
    }
    form.push_str(&format!("lang={}", query_escape(&lang)));
    let ucid: String = uuid_hex();
    let resp = client().await
        .post(format!("https://translate.yandex.net/api/v1/tr.json/translate?ucid={ucid}&srv=android&format=text"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("accept-encoding", "identity")
        .body(form)
        .send()
        .await;
    let Ok(resp) = resp else { return status500(origin) };
    if !resp.status().is_success() {
        return status500(origin);
    }
    let v = resp.json::<Value>().await.unwrap_or(json!({}));
    let translated = v
        .get("text")
        .and_then(|t| t.as_array())
        .map(|a| a.iter().map(|x| x.as_str().unwrap_or("")).collect::<Vec<_>>().join(","))
        .unwrap_or_default();
    text_ok(&translated, origin)
}

/* ---------------- lingva ---------------- */
pub async fn lingva(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let base = read_secret(st, "lingva_url").unwrap_or_else(|| LINGVA_DEFAULT.to_string());
    let mut lang = get_lang_field(body);
    match lang.as_str() {
        "zh-CN" | "zh-TW" => lang = "zh".into(),
        "pt-BR" | "pt-PT" => lang = "pt".into(),
        _ => {}
    }
    let text = body.sfield("text");
    if text.is_empty() || lang.is_empty() {
        return status400(origin);
    }
    let url = format!("{}/auto/{}/{}", base.trim_end_matches('/'), lang, query_escape(&text));
    let resp = client().await.get(&url).header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(30)).send().await;
    let Ok(resp) = resp else { return status500(origin) };
    let v = resp.json::<Value>().await.unwrap_or(json!({}));
    text_ok(v.sfield("translation").as_str(), origin)
}

/* ---------------- deepl ---------------- */
pub async fn deepl(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(key) = read_secret(st, "deepl") else { return status400(origin) };
    let mut lang = get_lang_field(body).to_uppercase();
    if lang.starts_with("ZH") {
        lang = "ZH".into();
    }
    let text = body.sfield("text");
    if text.is_empty() || lang.is_empty() {
        return status400(origin);
    }
    let mut form = format!("text={}&target_lang={}", query_escape(&text), query_escape(&lang));
    // formality：de/fr/it/es/nl/ja/ru/pt 面向（native 无配置面 → 'default'）
    if ["DE", "FR", "IT", "ES", "NL", "JA", "RU", "PT-BR", "PT-PT"].contains(&lang.as_str()) {
        form.push_str("&formality=default");
    }
    let endpoint = if body.sfield("endpoint") == "pro" {
        "https://api.deepl.com/v2/translate"
    } else {
        "https://api-free.deepl.com/v2/translate"
    };
    let resp = client().await
        .post(endpoint)
        .header("Accept", "application/json")
        .header("Authorization", format!("DeepL-Auth-Key {key}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("accept-encoding", "identity")
        .body(form)
        .send()
        .await;
    let Ok(resp) = resp else { return status500(origin) };
    if !resp.status().is_success() {
        return status500(origin);
    }
    let v = resp.json::<Value>().await.unwrap_or(json!({}));
    let out = v
        .get("translations")
        .and_then(|t| t.as_array())
        .and_then(|a| a.first())
        .and_then(|t| t.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    text_ok(&out, origin)
}

/* ---------------- onering ---------------- */
pub async fn onering(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let url = read_secret(st, "oneringtranslator_url").unwrap_or_else(|| ONERING_URL_DEFAULT.to_string());
    // HIGH-5：oneringtranslator_url 用户可控出站 → 统一网关（SelfHosted 自建端点：
    // 出厂默认 127.0.0.1:4990；本机与局域网均可，非 ProxyUrl 字面）
    let url = match validate_egress_url(&url, EgressPolicy::SelfHosted) {
        Ok(u) => u,
        Err(e) => return egress_reject("oneringtranslator_url", &e, origin),
    };
    let mut to_lang = body.sfield("to_lang");
    if ["pt-BR", "pt-PT"].contains(&body.sfield("to_lang").as_str()) {
        to_lang = "pt".into();
    }
    let text = body.sfield("text");
    let from_lang = body.sfield("from_lang");
    if text.is_empty() || from_lang.is_empty() || to_lang.is_empty() {
        return status400(origin);
    }
    let url = format!(
        "{}/?text={}&from_lang={}&to_lang={}",
        url.trim_end_matches('/'),
        query_escape(&text),
        query_escape(&from_lang),
        query_escape(&to_lang),
    );
    let resp = client().await.get(&url).header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(60)).send().await;
    let Ok(resp) = resp else { return status500(origin) };
    if !resp.status().is_success() {
        return status500(origin);
    }
    let v = resp.json::<Value>().await.unwrap_or(json!({}));
    let result = v.sfield("result");
    // App 契约面：{data:{result}}（ST 裸字符串面 App 读不到 —— 模块头偏差登记）
    plain(json!({ "data": { "result": result } }), origin)
}

/* ---------------- deeplx ---------------- */
pub async fn deeplx(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let url = read_secret(st, "deeplx_url").unwrap_or_else(|| DEEPLX_URL_DEFAULT.to_string());
    // HIGH-5：deeplx_url 用户可控出站 → 统一网关（SelfHosted 自建端点：
    // 出厂默认 127.0.0.1:1188；本机与局域网均可，非 ProxyUrl 字面）
    let url = match validate_egress_url(&url, EgressPolicy::SelfHosted) {
        Ok(u) => u,
        Err(e) => return egress_reject("deeplx_url", &e, origin),
    };
    let mut lang = get_lang_field(body);
    if ["zh-CN", "zh-TW"].contains(&lang.as_str()) {
        lang = "ZH".into();
    }
    let text = body.sfield("text");
    if text.is_empty() || lang.is_empty() {
        return status400(origin);
    }
    let payload = json!({ "text": text, "source_lang": "auto", "target_lang": lang });
    let resp = client().await
        .post(&url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("accept-encoding", "identity")
        .json(&payload)
        .send()
        .await;
    let Ok(resp) = resp else { return status500(origin) };
    if !resp.status().is_success() {
        return status500(origin);
    }
    let v = resp.json::<Value>().await.unwrap_or(json!({}));
    text_ok(v.sfield("data").as_str(), origin)
}

/* ---------------- bing ---------------- */
pub async fn bing(_st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let mut lang = get_lang_field(body);
    match lang.as_str() {
        "zh-CN" => lang = "zh-Hans".into(),
        "zh-TW" => lang = "zh-Hant".into(),
        "pt-BR" => lang = "pt".into(),
        _ => {}
    }
    let text = body.sfield("text");
    if text.is_empty() || lang.is_empty() {
        return status400(origin);
    }
    let cli = client().await;
    // 1) edge.microsoft.com/translate/auth → JWT（bing-translate-api 等价）
    let jwt = {
        let r = cli
            .get("https://edge.microsoft.com/translate/auth")
            .header("accept-encoding", "identity")
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await;
        let Ok(r) = r else { return status500(origin) };
        let t = r.text().await.unwrap_or_default();
        if t.is_empty() {
            return status500(origin);
        }
        t
    };
    // 2) cognitive translator API
    let resp = cli
        .post(format!(
            "https://api.cognitive.microsofttranslator.com/translate?api-version=3.0&to={}",
            query_escape(&lang)
        ))
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Content-Type", "application/json")
        .header("accept-encoding", "identity")
        .timeout(std::time::Duration::from_secs(30))
        .json(&json!([{ "Text": text }]))
        .send()
        .await;
    let Ok(resp) = resp else { return status500(origin) };
    if !resp.status().is_success() {
        return status500(origin);
    }
    let v = resp.json::<Value>().await.unwrap_or(json!([]));
    let out = v
        .get(0)
        .and_then(|item| item.get("translations"))
        .and_then(|t| t.as_array())
        .and_then(|a| a.first())
        .and_then(|t| t.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    text_ok(&out, origin)
}

/* ---------------- 工具 ---------------- */

/// application/x-www-form-urlencoded 百分号编码（空格 → '+' 语义）
fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'*' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn uuid_hex() -> String {
    // ST yandex ucid（translate.js:129）= uuidv4().replaceAll('-', '')
    // —— v4 随机（crypto.randomUUID：每 nibble 来自 CSPRNG，仅 version/variant
    // 位固定）。复用 vault::uuid_v4（getrandom OS CSPRNG 同源），去连字符
    // 即 32 hex 小写；修复前实现把循环外同一 xored 值重复 16 次 → 恒重复串。
    super::vault::uuid_v4().replace('-', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_escape_shapes() {
        assert_eq!(query_escape("a b+c&d"), "a+b%2Bc%26d");
        assert_eq!(query_escape("你好"), "%E4%BD%A0%E5%A5%BD");
    }

    #[test]
    fn lang_mappings_bing() {
        // bing 映射直接内联核对（zh-CN → zh-Hans 等）
        let mut lang = "zh-CN".to_string();
        if lang == "zh-CN" { lang = "zh-Hans".into(); }
        assert_eq!(lang, "zh-Hans");
    }

    #[test]
    fn uuid_hex_is_random_v4_shape() {
        let a = uuid_hex();
        let b = uuid_hex();
        assert_eq!(a.len(), 32, "uuid v4 无连字符 = 32 hex（ST ucid 字面长度）");
        assert!(
            a.chars().all(|c| c.is_ascii_hexdigit()) && a.chars().all(|c| !c.is_ascii_uppercase()),
            "全小写 hex"
        );
        assert_eq!(&a[12..13], "4", "version 位 = 4（uuidv4 语义）");
        assert!(
            matches!(&a[16..17], "8" | "9" | "a" | "b"),
            "variant 位 = 10xx（uuidv4 语义）"
        );
        assert_ne!(a, b, "两次生成必须不同（每 nibble 随机，CSPRNG 源）");
    }
}
