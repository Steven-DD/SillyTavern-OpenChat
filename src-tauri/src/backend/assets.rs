//! backend/assets —— 资产与静态资源服务（Phase 1 族六 · manifest #30-#40）
//!
//! ST 契约核对：
//! - /thumbnail?type=bg|avatar|persona&file=（thumbnails.js:249-303）：
//!   file !== sanitize(file) → 403；原图不存在 → 404。
//!   **Phase 1 裁剪**：直接 serve 原图（等效 ST `thumbnails.enabled=false` 的
//!   受支持配置路径，前端 <img> 行为不变）；真缩略图生成（Jimp 等价）延后，
//!   届时按 mtime>ctime 失效重生成（thumbnails.js:118-143 的缓存语义）。
//! - 静态前缀 /characters|backgrounds|user-images（users.js 静态路由）：
//!   等价文件的环回读取，鉴权豁免（S3）。
//! - /api/avatars/get|upload|delete（avatars.js:17-60）：getImages 按 name
//!   排序返回图片文件名数组；delete 恶意名 403 + {result:'ok'}/404；
//!   upload multipart → S16 魔数校验 → overwrite_name 或 时间戳.png → {path}。
//! - /api/backgrounds/all（backgrounds.js:14-38）：{images:[{filename,isAnimated}],
//!   config:{width,height}}；isAnimated 默认 false（metadata 索引是 ST 增强件）。
//! - /api/images/upload {image(base64), format, ch_name, filename}（images.js）：
//!   群头像用；S16+S17（服务端命名）。
//! - /api/sprites/get?name=（sprites.js:118-150）：characters/<name>/ 下图片
//!   扫描，label 从文件名正则取（joy.png/joy-1.png/joy.expressive.png），
//!   path=/characters/<name>/<file>?t=<mtime yyyymmddhhmmss>。

use super::{apply_cors_resp, BackendState};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::time::UNIX_EPOCH;

fn du(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user")
}

fn err_resp(status: StatusCode) -> Response {
    status.into_response()
}

/// S18：静态图片响应 —— Content-Type 按扩展名映射 + nosniff
fn image_response(bytes: Vec<u8>, file_name: &str, origin: Option<HeaderValue>) -> Response {
    let ext = file_name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" | "apng" => "image/png",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    };
    let mut resp = (StatusCode::OK, bytes).into_response();
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(mime) {
        h.insert(header::CONTENT_TYPE, v);
    }
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    apply_cors_resp(resp, origin)
}

/// thumbnail type → (thumbnails 子目录, 原图目录)
fn thumb_dirs(st: &BackendState, t: &str) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let du = du(st);
    match t {
        "bg" => Some((du.join("thumbnails").join("bg"), du.join("backgrounds"))),
        "avatar" => Some((du.join("thumbnails").join("avatar"), du.join("characters"))),
        "persona" => Some((du.join("thumbnails").join("persona"), du.join("User Avatars"))),
        _ => None,
    }
}

/// GET /thumbnail?type=&file=（manifest #30）—— Phase 1 直接 serve 原图
pub fn thumbnail(st: &BackendState, query: &str, origin: Option<HeaderValue>) -> Response {
    // 极简 query 解析（type 与 file 两个键）
    let mut t = "";
    let mut file = "";
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        match (kv.next(), kv.next()) {
            (Some("type"), Some(v)) => t = v,
            (Some("file"), Some(v)) => file = v,
            _ => {}
        }
    }
    if t.is_empty() || file.is_empty() {
        return err_resp(StatusCode::BAD_REQUEST);
    }
    if !matches!(t, "bg" | "avatar" | "persona") {
        return err_resp(StatusCode::BAD_REQUEST);
    }
    // ST：file !== sanitize(file) → 403（thumbnails.js:258）
    if file != crate::pathguard::sanitize(file) {
        return err_resp(StatusCode::FORBIDDEN);
    }
    let Some((_, original)) = thumb_dirs(st, t) else {
        return err_resp(StatusCode::BAD_REQUEST);
    };
    let Ok(safe) = crate::pathguard::component(file) else {
        return err_resp(StatusCode::FORBIDDEN);
    };
    let p = original.join(safe);
    if !p.is_file() {
        return err_resp(StatusCode::NOT_FOUND);
    }
    match std::fs::read(&p) {
        Ok(bytes) => image_response(bytes, file, origin),
        Err(_) => err_resp(StatusCode::NOT_FOUND),
    }
}

/// GET /characters/<file> 等（manifest #31-#33 静态面，鉴权豁免路径的落盘读取）
pub fn static_asset(st: &BackendState, path: &str, origin: Option<HeaderValue>) -> Response {
    // path 形如 /characters/x.png、/backgrounds/b.jpg、/user-images/u.png
    let (prefix, rel) = match path.split_once('/') {
        Some((_, rest)) => match rest.split_once('/') {
            Some(pair) => pair,
            None => return err_resp(StatusCode::NOT_FOUND),
        },
        None => return err_resp(StatusCode::NOT_FOUND),
    };
    let base = match prefix {
        "characters" => du(st).join("characters"),
        "backgrounds" => du(st).join("backgrounds"),
        "user-images" => du(st).join("user").join("images"),
        _ => return err_resp(StatusCode::NOT_FOUND),
    };
    let Ok(safe) = crate::pathguard::component(rel.split('?').next().unwrap_or(rel)) else {
        return err_resp(StatusCode::FORBIDDEN);
    };
    let safe_str = safe.clone();
    let p = base.join(&safe);
    if !p.is_file() {
        return err_resp(StatusCode::NOT_FOUND);
    }
    match std::fs::read(&p) {
        Ok(bytes) => image_response(bytes, &safe_str, origin),
        Err(_) => err_resp(StatusCode::NOT_FOUND),
    }
}

/// POST /api/avatars/get（manifest #35）：图片文件名数组（getImages name 排序）
pub fn avatars_get(st: &BackendState, origin: Option<HeaderValue>) -> Response {
    let dir = du(st).join("User Avatars");
    let _ = std::fs::create_dir_all(&dir);
    let mut names: Vec<String> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| is_image_name(n))
            .collect(),
        Err(e) => {
            return apply_cors_resp(
                err_json_internal(&format!("avatars 目录读取失败：{e}")).into_response(),
                origin,
            )
        }
    };
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    ok_json(origin, json!(names))
}

/// POST /api/avatars/upload（manifest #36）：multipart avatar → {path}
pub async fn avatars_upload(
    st: &BackendState,
    content_type: Option<&str>,
    body: axum::body::Bytes,
    origin: Option<HeaderValue>,
) -> Response {
    let m = match super::characters::parse_multipart(content_type.unwrap_or(""), body).await {
        Ok(m) => m,
        Err(_) => return err_resp(StatusCode::BAD_REQUEST),
    };
    let Some(bytes) = m.avatar else { return err_resp(StatusCode::BAD_REQUEST) };
    // S16：魔数校验（persona 头像必须真图片）
    let declared = m
        .avatar_filename
        .as_deref()
        .and_then(|f| f.rsplit('.').next())
        .unwrap_or("png");
    if let Err(e) = super::uploads::check_magic(declared, &bytes) {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, Json(json!({ "error": { "message": e.to_string() } }))).into_response(),
            origin,
        );
    }
    let dir = du(st).join("User Avatars");
    let _ = std::fs::create_dir_all(&dir);
    // overwrite_name 或 时间戳.png（avatars.js:56）
    let filename = match m.fields.get("overwrite_name").filter(|s| !s.trim().is_empty()) {
        Some(ow) => crate::pathguard::sanitize(ow),
        None => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            format!("{ts}.png")
        }
    };
    if filename.is_empty() {
        return err_resp(StatusCode::BAD_REQUEST);
    }
    let p = dir.join(&filename);
    match std::fs::write(&p, &bytes) {
        Ok(()) => ok_json(origin, json!({ "path": filename })),
        Err(e) => apply_cors_resp(
            err_json_internal(&format!("写入失败：{e}")).into_response(),
            origin,
        ),
    }
}

/// POST /api/avatars/delete {avatar}（manifest #37）：恶意名 403；{result:'ok'}/404
pub fn avatars_delete(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(avatar) = body.get("avatar").and_then(|v| v.as_str()) else {
        return err_resp(StatusCode::BAD_REQUEST);
    };
    if avatar != crate::pathguard::sanitize(avatar) {
        return err_resp(StatusCode::FORBIDDEN);
    }
    let Ok(safe) = crate::pathguard::component(avatar) else {
        return err_resp(StatusCode::FORBIDDEN);
    };
    let p = du(st).join("User Avatars").join(&safe);
    if p.is_file() {
        let _ = std::fs::remove_file(&p);
        // 缩略图失效（avatars.js:34 invalidateThumbnail persona）
        let thumb = du(st).join("thumbnails").join("persona").join(&safe);
        let _ = std::fs::remove_file(thumb);
        ok_json(origin, json!({ "result": "ok" }))
    } else {
        err_resp(StatusCode::NOT_FOUND)
    }
}

/// POST /api/backgrounds/all（manifest #38）：{images:[{filename,isAnimated}], config}
pub fn backgrounds_all(st: &BackendState, origin: Option<HeaderValue>) -> Response {
    let dir = du(st).join("backgrounds");
    let _ = std::fs::create_dir_all(&dir);
    let mut names: Vec<String> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| is_image_name(n))
            .collect(),
        Err(e) => {
            return apply_cors_resp(
                err_json_internal(&format!("backgrounds 目录读取失败：{e}")).into_response(),
                origin,
            )
        }
    };
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    // isAnimated：metadata 索引是 ST 增强件（bg metadata），native 一期 false
    let images: Vec<Value> = names
        .iter()
        .map(|n| json!({ "filename": n, "isAnimated": false }))
        .collect();
    // config：thumbnailDimensions.bg（image-metadata.js；ST 默认 bg=[320,180]）
    ok_json(origin, json!({ "images": images, "config": { "width": 320, "height": 180 } }))
}

/// POST /api/images/upload {image, format, ch_name, filename}（manifest #39）
/// 群头像：base64 解码 → S16 魔数 → S17 服务端命名 → {path}
pub fn images_upload(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let b64 = body.get("image").and_then(|v| v.as_str()).unwrap_or("");
    let format = body.get("format").and_then(|v| v.as_str()).unwrap_or("png");
    let ch_name = body.get("ch_name").and_then(|v| v.as_str()).unwrap_or("");
    // 剥 data URL 前缀（前端 resizeImageDataUrl 产物 data:image/jpeg;base64,...）
    let b64 = b64.split(',').last().unwrap_or(b64);
    let bytes = match use_base64_decode(b64) {
        Ok(b) => b,
        Err(_) => {
            return apply_cors_resp(
                err_json_internal("base64 解码失败").into_response(),
                origin,
            )
        }
    };
    if let Err(e) = super::uploads::check_extension(format) {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, Json(json!({ "error": e.to_string() }))).into_response(),
            origin,
        );
    }
    if let Err(e) = super::uploads::check_magic(format, &bytes) {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, Json(json!({ "error": e.to_string() }))).into_response(),
            origin,
        );
    }
    if let Err(e) = super::uploads::check_size(bytes.len(), st.max_body_bytes) {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, Json(json!({ "error": e.to_string() }))).into_response(),
            origin,
        );
    }
    // 服务端命名（S17）：`<ch_name>_<ms>`（对齐前端 groups.ts:77 的语义，
    // 但用户提供的 filename 只作参考）
    let safe_ch = crate::pathguard::sanitize(ch_name);
    if safe_ch.is_empty() {
        return apply_cors_resp(
            (StatusCode::BAD_REQUEST, Json(json!({ "error": "ch_name 无效" }))).into_response(),
            origin,
        );
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let ext = if format == "jpg" { "jpg" } else { format };
    let dir = du(st).join("user").join("images").join(&safe_ch);
    let _ = std::fs::create_dir_all(&dir);
    let file_name = format!("{safe_ch}_{ts}.{ext}");
    let p = dir.join(&file_name);
    if let Err(e) = std::fs::write(&p, &bytes) {
        return apply_cors_resp(
            err_json_internal(&format!("写入失败：{e}")).into_response(),
            origin,
        );
    }
    // 返回路径（ST images.js：`user/images/<ch_name>/<file>` 相对形态，
    // 前端 groups.ts:78 消费 r.path 并写回 group.avatar_url）
    ok_json(origin, json!({ "path": format!("user/images/{safe_ch}/{file_name}") }))
}

/// GET /api/sprites/get?name=（manifest #40）
pub fn sprites_get(st: &BackendState, query: &str, origin: Option<HeaderValue>) -> Response {
    let name = query
        .split('&')
        .find_map(|p| p.strip_prefix("name="))
        .unwrap_or("");
    // name 可含子目录 `<char>/<sub>`（sprites.js:18-38）
    let (char_name, sub) = match name.split_once('/') {
        Some((c, s)) => (c, Some(s)),
        None => (name, None),
    };
    let Ok(safe_char) = crate::pathguard::component(char_name) else {
        return ok_json(origin, json!([]));
    };
    let dir = match sub {
        Some(s) => {
            let Ok(safe_sub) = crate::pathguard::component(s) else {
                return ok_json(origin, json!([]));
            };
            du(st).join("characters").join(safe_char).join(safe_sub)
        }
        None => du(st).join("characters").join(safe_char),
    };
    let mut out = Vec::new();
    if dir.is_dir() {
        let mut names: Vec<String> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| is_image_name(n))
                .collect(),
            Err(_) => Vec::new(),
        };
        names.sort();
        for f in names {
            let p = dir.join(&f);
            // mtime → yyyymmddhhmmss（sprites.js:133 缓存参数）
            let mtime_tag = std::fs::metadata(&p)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| {
                    let (y, mth, dd, h, mi, s) =
                        super::settings::civil_utc(d.as_secs() as i64);
                    format!("{y:04}{mth:02}{dd:02}{h:02}{mi:02}{s:02}")
                })
                .unwrap_or_default();
            // label：文件名 stem 的小写，剥首个 -/. 后缀（sprites.js:138）
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            let label = stem
                .split(['-', '.'])
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(&stem)
                .to_string();
            let q = if mtime_tag.is_empty() { String::new() } else { format!("?t={mtime_tag}") };
            out.push(json!({
                "label": label,
                "path": format!("/characters/{name}/{f}{q}"),
            }));
        }
    }
    ok_json(origin, json!(out))
}

/// 图片扩展名判定（getImages 的 mime lookup 等价面）
fn is_image_name(n: &str) -> bool {
    let ext = n.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "apng" | "jfif")
}

fn ok_json(origin: Option<HeaderValue>, v: Value) -> Response {
    apply_cors_resp(Json(v).into_response(), origin)
}

fn err_json_internal(msg: &str) -> axum::response::Response {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": { "message": msg } }))).into_response()
}

/// base64 解码（独立 helper：避免 uploads.rs 引入 base64 依赖重复）
fn use_base64_decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s)
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
        let root = std::env::temp_dir().join(format!("stchat-assets-{tag}-{}", std::process::id()));
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

    fn real_png() -> Vec<u8> {
        crate::compat::png::minimal_png()
    }

    #[test]
    fn thumbnail_serves_original_and_rejects() {
        let st = st_with("thumb");
        let chars = du(&st).join("characters");
        std::fs::create_dir_all(&chars).unwrap();
        std::fs::write(chars.join("a.png"), real_png()).unwrap();

        // 正常取图
        let r = thumbnail(&st, "type=avatar&file=a.png", None);
        assert_eq!(r.status().as_u16(), 200);
        assert_eq!(r.headers().get("content-type").and_then(|v| v.to_str().ok()), Some("image/png"));
        assert_eq!(
            r.headers().get("x-content-type-options").and_then(|v| v.to_str().ok()),
            Some("nosniff")
        );
        // 恶意名 403（ST thumbnails.js:258）
        let r = thumbnail(&st, "type=avatar&file=../evil.png", None);
        assert_eq!(r.status().as_u16(), 403);
        // 非法 type 400
        let r = thumbnail(&st, "type=x&file=a.png", None);
        assert_eq!(r.status().as_u16(), 400);
        // 不存在 404
        let r = thumbnail(&st, "type=avatar&file=nope.png", None);
        assert_eq!(r.status().as_u16(), 404);
    }

    #[test]
    fn static_assets_by_prefix() {
        let st = st_with("static");
        std::fs::create_dir_all(du(&st).join("backgrounds")).unwrap();
        std::fs::write(du(&st).join("backgrounds").join("b.jpg"), real_png()).unwrap();
        let r = static_asset(&st, "/backgrounds/b.jpg", None);
        assert_eq!(r.status().as_u16(), 200);
        let r = static_asset(&st, "/backgrounds/../../x", None);
        assert!(r.status().as_u16() == 403 || r.status().as_u16() == 404);
        let r = static_asset(&st, "/unknown/x.png", None);
        assert_eq!(r.status().as_u16(), 404);
    }

    #[test]
    fn avatars_lifecycle() {
        let st = st_with("avatars");
        let dir = du(&st).join("User Avatars");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.png"), real_png()).unwrap();

        let r = avatars_get(&st, None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);

        // delete 恶意名 403
        let r = avatars_delete(&st, &json!({ "avatar": "../x.png" }), None);
        assert_eq!(r.status().as_u16(), 403);
        // delete 成功
        let r = avatars_delete(&st, &json!({ "avatar": "p.png" }), None);
        assert_eq!(r.status().as_u16(), 200);
        assert!(!dir.join("p.png").exists());
    }

    #[tokio::test]
    async fn avatars_upload_magic_check() {
        let st = st_with("avup");
        // 伪 PNG → 400
        let mp = "--B\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"a.png\"\r\n\r\n<svg>\r\n--B--\r\n";
        let r = avatars_upload(&st, Some("multipart/form-data; boundary=B"), axum::body::Bytes::from(mp), None).await;
        assert_eq!(r.status().as_u16(), 400);
        // 真 PNG → {path}
        let b64img = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(real_png())
        };
        let mp = format!(
            "--B\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"a.png\"\r\n\r\n{b64img}\r\n--B--\r\n"
        );
        // 注意：multipart 文件字段是原始字节，不是 base64 —— 用真字节
        let mut body = Vec::new();
        body.extend_from_slice(b"--B\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\n");
        body.extend_from_slice(&real_png());
        body.extend_from_slice(b"\r\n--B--\r\n");
        let r = avatars_upload(
            &st,
            Some("multipart/form-data; boundary=B"),
            axum::body::Bytes::from(body),
            None,
        )
        .await;
        assert_eq!(r.status().as_u16(), 200);
        let bytes = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v["path"].as_str().unwrap().ends_with(".png"));
        let _ = b64img;
    }

    #[test]
    fn backgrounds_all_shape() {
        let st = st_with("bg");
        let dir = du(&st).join("backgrounds");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.png"), real_png()).unwrap();
        std::fs::write(dir.join("note.txt"), "not image").unwrap();

        let r = backgrounds_all(&st, None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = v["images"].as_array().unwrap();
        assert_eq!(arr.len(), 1, "非图片文件被过滤");
        assert_eq!(arr[0]["filename"], "b.png");
        assert_eq!(arr[0]["isAnimated"], false);
        assert_eq!(v["config"]["width"], 320);
    }

    #[test]
    fn images_upload_server_names_file() {
        let st = st_with("imgup");
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(real_png());
        let r = images_upload(
            &st,
            &json!({ "image": b64, "format": "png", "ch_name": "grp1", "filename": "用户起的" }),
            None,
        );
        assert_eq!(r.status().as_u16(), 200);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let path = v["path"].as_str().unwrap();
        assert!(path.starts_with("user/images/grp1/grp1_"), "{path}");
        assert!(path.ends_with(".png"));
        // 落盘验证
        assert!(du(&st).join(path).is_file());
    }

    #[test]
    fn sprites_list_with_labels() {
        let st = st_with("sprites");
        let dir = du(&st).join("characters").join("甲");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("joy.png"), real_png()).unwrap();
        std::fs::write(dir.join("joy-1.png"), real_png()).unwrap();
        std::fs::write(dir.join("sad.expressive.png"), real_png()).unwrap();
        std::fs::write(dir.join("readme.txt"), "skip").unwrap();

        let r = sprites_get(&st, "name=甲", None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 3, "非图片跳过");
        let joy = arr.iter().find(|s| s["label"] == "joy").unwrap();
        // 注意：joy-1.png 与 joy.png 同 label，排序在前者先返回（ST 同语义）
        let joy_path = joy["path"].as_str().unwrap();
        assert!(
            joy_path.starts_with("/characters/甲/joy.png?t=") || joy_path.starts_with("/characters/甲/joy-1.png?t="),
            "{joy_path}"
        );
        assert!(arr.iter().any(|s| s["label"] == "sad"), "sad.expressive → label=sad");
        // 子目录形态
        std::fs::create_dir_all(du(&st).join("characters").join("乙").join("sub")).unwrap();
        std::fs::write(
            du(&st).join("characters").join("乙").join("sub").join("calm.png"),
            real_png(),
        )
        .unwrap();
        let r = sprites_get(&st, "name=乙/sub", None);
        let bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()
        });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v.as_array().unwrap()[0]["label"], "calm");
    }
}
