//! backend/characters —— 角色卡数据服务（Phase 1 族二 · manifest #1-#8）
//!
//! ST 契约（characters.js:406-1497，已逐行核对）：
//! - `/all`：读 characters/ 下全部 .png → processCharacter 列表（shallow 形态：
//!   顶层仅统计字段 + data 子集 —— ST 默认 lazyLoadCharacters=false 即全量；
//!   App 的 data.ts 消费全量字段，这里实现 shallow=false 形态）。
//! - `/get {avatar_url}`：单卡全量；404（不存在）/ 500（读失败）。
//! - `/chats {avatar_url, metadata, simple}`：无 chats 目录 → `{error:true}`；
//!   空目录 → `[]`；simple=true → 仅文件名。
//! - `/create`（multipart）/`/import`（multipart）/`/export`/`/rename`/`/edit`/`/delete`。
//!
//! 本批次范围（评审批准）：all / get / chats / delete / rename 的**非 multipart**
//! 端点（JSON 面）；create/import/export/edit 的 multipart 与裁剪面在族三接线
//! （依赖 uploads.rs 的魔数校验 + 服务端命名）。

use super::{apply_cors_resp, BackendState};
use crate::compat::chara_card;
use crate::compat::png;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

fn characters_dir(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user").join("characters")
}

fn chats_dir(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user").join("chats")
}

/// 路径解析：avatar_url 过 pathguard（评审 S6：单组件接口）
fn character_path(st: &BackendState, avatar_url: &str) -> Result<std::path::PathBuf, String> {
    let safe = crate::pathguard::component(avatar_url).map_err(|e| e.to_string())?;
    Ok(characters_dir(st).join(&safe))
}

fn err_json(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": { "message": msg } }))).into_response()
}

/// processCharacter（characters.js:406-441）的 Rust 等价：
/// 读 PNG → read_chara → to_v2_response → 挂统计字段
fn process_character(st: &BackendState, file_name: &str) -> Result<Value, String> {
    let p = character_path(st, file_name)?;
    let raw = std::fs::read(&p).map_err(|e| format!("读取失败：{e}"))?;
    let meta = png::read_chara(&raw).map_err(|e| e.to_string())?;
    let card: Value = serde_json::from_str(&meta).map_err(|e| format!("卡 JSON 解析失败：{e}"))?;
    let mut character = chara_card::to_v2_response(card);

    let stat = std::fs::metadata(&p).map_err(|e| format!("stat 失败：{e}"))?;
    let created = stat
        .created()
        .or_else(|_| stat.modified())
        .unwrap_or(UNIX_EPOCH);
    let ctime_ms = created
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // create_date：ISO 字符串（characters.js:418）
    let iso = iso8601_from_unix(
        created
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    );

    // chat 统计（calculateChatSize：chats/<name>/ 字节和 + 最新 mtime）
    let stem = file_name.trim_end_matches(".png");
    let cdir = chats_dir(st).join(stem);
    let (chat_size, date_last_chat) = match std::fs::read_dir(&cdir) {
        Ok(rd) => {
            let mut size = 0u64;
            let mut last = 0u64;
            for e in rd.flatten() {
                if let Ok(m) = e.metadata() {
                    if m.is_file() {
                        size += m.len();
                        last = last.max(
                            m.modified()
                                .ok()
                                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0),
                        );
                    }
                }
            }
            (size, last)
        }
        Err(_) => (0, 0),
    };

    let obj = character.as_object_mut().ok_or("卡不是 JSON 对象")?;
    obj.insert("avatar".into(), json!(file_name));
    obj.insert("json_data".into(), json!(meta));
    obj.insert("date_added".into(), json!(ctime_ms));
    obj.insert("create_date".into(), json!(iso));
    obj.insert("date_last_chat".into(), json!(date_last_chat));
    obj.insert("chat_size".into(), json!(chat_size));
    obj.insert(
        "data_size".into(),
        json!(chara_card::calculate_data_size(
            obj.get("data").unwrap_or(&Value::Null)
        )),
    );
    Ok(character)
}

/// Unix 秒 → ISO8601 字符串（create_date 契约；UTC 后缀 Z）
fn iso8601_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, m, s) = (
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    );
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if mth <= 2 { y + 1 } else { y };
    format!("{year:04}-{mth:02}-{d:02}T{h:02}:{m:02}:{s:02}.000Z")
}

/* ------------------------------------------------------------------ *
 * Handlers
 * ------------------------------------------------------------------ */

/// POST /api/characters/all（manifest #1）
pub fn all(st: &BackendState, origin: Option<HeaderValue>) -> Response {
    let dir = characters_dir(st);
    let mut out = Vec::new();
    match std::fs::read_dir(&dir) {
        Ok(rd) => {
            let mut names: Vec<String> = rd
                .flatten()
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("png"))
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            names.sort();
            for n in names {
                match process_character(st, &n) {
                    Ok(c) => {
                        // 过滤无名卡（ST .filter(c => c.name)）
                        if c.get("name")
                            .and_then(|v| v.as_str())
                            .map(|s| !s.is_empty())
                            .unwrap_or(false)
                        {
                            out.push(c);
                        }
                    }
                    Err(e) => {
                        // ST：单张读失败只打日志、跳过（Promise.all 里 try/catch）
                        eprintln!("[backend] characters/all 跳过 {n}：{e}");
                    }
                }
            }
        }
        Err(_) => {
            // 目录不存在：ST 同语义抛错 → 500 {error:true}
            return apply_cors_resp(
                err_json(StatusCode::INTERNAL_SERVER_ERROR, "characters 目录读取失败")
                    .into_response(),
                origin,
            );
        }
    }
    apply_cors_resp(Json(json!(out)).into_response(), origin)
}

/// POST /api/characters/get {avatar_url}（manifest #2）
pub fn get(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(avatar) = body.get("avatar_url").and_then(|v| v.as_str()) else {
        return apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "缺少 avatar_url").into_response(),
            origin,
        );
    };
    let p = match character_path(st, avatar) {
        Ok(p) => p,
        Err(e) => {
            return apply_cors_resp(err_json(StatusCode::FORBIDDEN, &e).into_response(), origin)
        }
    };
    if !p.is_file() {
        return apply_cors_resp(
            err_json(StatusCode::NOT_FOUND, "角色卡不存在").into_response(),
            origin,
        );
    }
    match process_character(st, avatar) {
        Ok(c) => apply_cors_resp(Json(c).into_response(), origin),
        Err(e) => apply_cors_resp(
            err_json(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
            origin,
        ),
    }
}

/// POST /api/characters/chats {avatar_url, metadata, simple}（manifest #3）
pub fn chats(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(avatar) = body.get("avatar_url").and_then(|v| v.as_str()) else {
        return apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "缺少 avatar_url").into_response(),
            origin,
        );
    };
    let stem = avatar.trim_end_matches(".png");
    let dir = match crate::pathguard::component(stem) {
        Ok(safe) => chats_dir(st).join(safe),
        Err(e) => {
            return apply_cors_resp(
                err_json(StatusCode::FORBIDDEN, &e.to_string()).into_response(),
                origin,
            )
        }
    };
    if !dir.exists() {
        // ST：无 chats 目录 → {error:true}（前端归一化为 []）
        return apply_cors_resp(Json(json!({ "error": true })).into_response(), origin);
    }
    let simple = body
        .get("simple")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let with_metadata = body
        .get("metadata")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut names: Vec<String> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect(),
        Err(e) => {
            return apply_cors_resp(
                err_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("读取 chats 失败：{e}"),
                )
                .into_response(),
                origin,
            );
        }
    };
    names.sort();

    if simple {
        let out: Vec<Value> = names
            .iter()
            .map(|f| json!({ "file_name": f, "file_id": f.trim_end_matches(".jsonl") }))
            .collect();
        return apply_cors_resp(Json(json!(out)).into_response(), origin);
    }

    let mut out = Vec::new();
    for f in &names {
        let p = dir.join(f);
        let stat = match std::fs::metadata(&p) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mtime_ms = stat
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut item = json!({
            "file_name": f,
            "file_id": f.trim_end_matches(".jsonl"),
            "file_size": stat.len().to_string(),
            "last_mes": mtime_ms,
        });
        if with_metadata {
            // getChatInfo 的 metadata 段：读 JSONL 首行 chat_metadata
            if let Ok(text) = std::fs::read_to_string(&p) {
                if let Some(first) = text.lines().next() {
                    if let Ok(head) = serde_json::from_str::<Value>(first) {
                        if let Some(meta) = head.get("chat_metadata") {
                            item.as_object_mut()
                                .unwrap()
                                .insert("chat_metadata".into(), meta.clone());
                        }
                    }
                }
            }
        }
        out.push(item);
    }
    apply_cors_resp(Json(json!(out)).into_response(), origin)
}

/// POST /api/characters/delete {avatar_url, delete_chats}（manifest #6）
pub fn delete(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(avatar) = body.get("avatar_url").and_then(|v| v.as_str()) else {
        return apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "缺少 avatar_url").into_response(),
            origin,
        );
    };
    // ST：avatar_url !== sanitize(avatar_url) → 403（characters.js:1421）
    if avatar != crate::pathguard::sanitize(avatar) {
        return apply_cors_resp(
            err_json(StatusCode::FORBIDDEN, "非法文件名").into_response(),
            origin,
        );
    }
    let p = match character_path(st, avatar) {
        Ok(p) => p,
        Err(e) => {
            return apply_cors_resp(err_json(StatusCode::FORBIDDEN, &e).into_response(), origin)
        }
    };
    if !p.is_file() {
        return apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "角色卡不存在").into_response(),
            origin,
        );
    }
    if let Err(e) = std::fs::remove_file(&p) {
        return apply_cors_resp(
            err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("删除失败：{e}")).into_response(),
            origin,
        );
    }
    let stem = avatar.trim_end_matches(".png");
    if body
        .get("delete_chats")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        if stem.is_empty() {
            return apply_cors_resp(
                err_json(StatusCode::FORBIDDEN, "非法目录名").into_response(),
                origin,
            );
        }
        if let Ok(safe) = crate::pathguard::component(stem) {
            let _ = std::fs::remove_dir_all(chats_dir(st).join(safe));
        }
    }
    apply_cors_resp(StatusCode::OK.into_response(), origin)
}

/// POST /api/characters/rename {avatar_url, new_name}（manifest #7）
/// ST 返回 JSON `{avatar}`（characters.js:1094）；chats 目录迁移 + 旧文件删除。
pub fn rename(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let (Some(avatar), Some(new_name)) = (
        body.get("avatar_url").and_then(|v| v.as_str()),
        body.get("new_name").and_then(|v| v.as_str()),
    ) else {
        return apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "缺少 avatar_url 或 new_name").into_response(),
            origin,
        );
    };
    let old_path = match character_path(st, avatar) {
        Ok(p) => p,
        Err(e) => {
            return apply_cors_resp(err_json(StatusCode::FORBIDDEN, &e).into_response(), origin)
        }
    };
    if !old_path.is_file() {
        return apply_cors_resp(
            err_json(StatusCode::INTERNAL_SERVER_ERROR, "角色卡不存在").into_response(),
            origin,
        );
    }
    let safe_new = crate::pathguard::sanitize(new_name);
    if safe_new.is_empty() {
        return apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "新名称净化后为空").into_response(),
            origin,
        );
    }
    // getPngName：重名追加序号（characters.js:1543-1547）
    let stem = get_png_name(&characters_dir(st), &safe_new);
    let new_file = format!("{stem}.png");
    if new_file == avatar {
        return apply_cors_resp(Json(json!({ "avatar": new_file })).into_response(), origin);
    }

    // 读旧卡 → 改 name 两处 → 写新文件
    let raw = match std::fs::read(&old_path) {
        Ok(r) => r,
        Err(e) => {
            return apply_cors_resp(
                err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("读取失败：{e}"))
                    .into_response(),
                origin,
            )
        }
    };
    let meta = match png::read_chara(&raw) {
        Ok(m) => m,
        Err(e) => {
            return apply_cors_resp(
                err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
                origin,
            )
        }
    };
    let mut card: Value = match serde_json::from_str(&meta) {
        Ok(c) => c,
        Err(e) => {
            return apply_cors_resp(
                err_json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("JSON 失败：{e}"),
                )
                .into_response(),
                origin,
            )
        }
    };
    if let Some(o) = card.as_object_mut() {
        o.insert("name".into(), json!(safe_new));
        if let Some(d) = o.get_mut("data").and_then(|d| d.as_object_mut()) {
            d.insert("name".into(), json!(safe_new));
        }
    }
    let new_data = serde_json::to_string(&card).unwrap_or(meta.clone());
    let new_png = match png::write_chara(&raw, &new_data) {
        Ok(b) => b,
        Err(e) => {
            return apply_cors_resp(
                err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
                origin,
            )
        }
    };
    let new_path = old_path.with_file_name(&new_file);
    if let Err(e) = std::fs::write(&new_path, &new_png) {
        return apply_cors_resp(
            err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("写入失败：{e}")).into_response(),
            origin,
        );
    }
    // chats 目录迁移（旧名 → 新名）
    let old_stem = avatar.trim_end_matches(".png");
    let (old_c, new_c) = (chats_dir(st).join(old_stem), chats_dir(st).join(&stem));
    if old_c.is_dir() && !new_c.exists() {
        let _ = std::fs::rename(&old_c, &new_c);
    }
    let _ = std::fs::remove_file(&old_path);
    apply_cors_resp(Json(json!({ "avatar": new_file })).into_response(), origin)
}

/// getPngName（characters.js:1543-1547）：重名时 base + 序号，最多 10000 次
fn get_png_name(dir: &std::path::Path, base: &str) -> String {
    for i in 0..10_000u32 {
        let cand = if i == 0 {
            base.to_string()
        } else {
            format!("{base}{i}")
        };
        if !dir.join(format!("{cand}.png")).exists() {
            return cand;
        }
    }
    base.to_string()
}

/* ------------------------------------------------------------------ *
 * 族三：multipart 面（create / edit / import）+ export
 * ------------------------------------------------------------------ */

/// multipart 解析结果（对齐 ST multer single('avatar')：文件字段固定 avatar +
/// 其余文本字段收集为 Map）
pub struct MultipartData {
    /// avatar 字段的文件内容（None = 未上传文件，走默认头像路径）
    pub avatar: Option<Vec<u8>>,
    /// 上传文件声明的文件名（filename 部分；服务端不落盘用此名 —— S17）
    pub avatar_filename: Option<String>,
    /// 其余文本字段
    pub fields: std::collections::HashMap<String, String>,
}

/// 解析 multipart/form-data。content_type 需含 boundary。
pub async fn parse_multipart(
    content_type: &str,
    body: axum::body::Bytes,
) -> Result<MultipartData, String> {
    let boundary =
        multer::parse_boundary(content_type).map_err(|e| format!("boundary 解析失败：{e}"))?;
    let stream = futures_util::stream::once(async move { Ok::<_, std::io::Error>(body) });
    let mut m = multer::Multipart::new(stream, boundary);
    let mut out = MultipartData {
        avatar: None,
        avatar_filename: None,
        fields: Default::default(),
    };
    while let Some(field) = m
        .next_field()
        .await
        .map_err(|e| format!("multipart 字段读取失败：{e}"))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "avatar" {
            out.avatar_filename = field.file_name().map(|s| s.to_string());
            let bytes = field
                .bytes()
                .await
                .map_err(|e| format!("avatar 读取失败：{e}"))?;
            out.avatar = Some(bytes.to_vec());
        } else {
            let v = field
                .text()
                .await
                .map_err(|e| format!("字段 {name} 读取失败：{e}"))?;
            out.fields.insert(name, v);
        }
    }
    Ok(out)
}

/// writeCharacterData 的 Rust 等价（characters.js:220-265）：
/// 输入图（已存在文件路径内容或默认图）→ 写入 chara 数据 → 落盘 `<stem>.png`
fn write_character_data(
    st: &BackendState,
    input_image: &[u8],
    data: &str,
    internal_name: &str,
) -> Result<std::path::PathBuf, String> {
    // 输入不是合法 PNG 时兜底为最小图（对齐 ST tryReadImage 的 fallback 分支）
    let image = if png::extract_chunks(input_image).is_ok() {
        input_image.to_vec()
    } else {
        png::minimal_png()
    };
    let out = png::write_chara(&image, data).map_err(|e| e.to_string())?;
    let dir = characters_dir(st);
    std::fs::create_dir_all(&dir).map_err(|e| format!("目录创建失败：{e}"))?;
    let path = dir.join(format!("{internal_name}.png"));
    std::fs::write(&path, &out).map_err(|e| format!("写入失败：{e}"))?;
    Ok(path)
}

/// POST /api/characters/create（manifest #4）—— multipart 或 JSON。
/// 返回**纯文本** avatarName（ST characters.js:1039/1045 response.send(avatarName)，
/// 前端 data.ts:107-110 按 text() 消费）。
pub async fn create(
    st: &BackendState,
    content_type: Option<&str>,
    body: axum::body::Bytes,
    origin: Option<HeaderValue>,
) -> Response {
    // 统一收集为「表单字段 Map + 可选头像字节」
    let fields: std::collections::HashMap<String, String>;
    let avatar: Option<Vec<u8>>;
    let avatar_filename: Option<String>;
    if content_type
        .map(|c| c.starts_with("multipart/form-data"))
        .unwrap_or(false)
    {
        match parse_multipart(content_type.unwrap(), body).await {
            Ok(m) => {
                fields = m.fields;
                avatar = m.avatar;
                avatar_filename = m.avatar_filename;
            }
            Err(e) => {
                return apply_cors_resp(
                    err_json(StatusCode::BAD_REQUEST, &e).into_response(),
                    origin,
                )
            }
        }
    } else {
        match serde_json::from_slice::<Value>(&body) {
            Ok(v) => {
                fields = v
                    .as_object()
                    .map(|o| {
                        o.iter()
                            .filter(|(_, v)| v.is_string() || v.is_boolean() || v.is_number())
                            .map(|(k, v)| {
                                let s = match v {
                                    Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                (k.clone(), s)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                avatar = None;
                avatar_filename = None;
            }
            Err(_) => return apply_cors_resp(bad_json_body().into_response(), origin),
        }
    }

    // ch_name sanitize（ST create:1028）；空名 → 400
    let raw_name = fields.get("ch_name").cloned().unwrap_or_default();
    let safe_name = crate::pathguard::sanitize(&raw_name);
    if safe_name.is_empty() {
        return apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "ch_name 缺失或净化后为空").into_response(),
            origin,
        );
    }

    // 上传了头像文件 → 魔数校验（S16：声明 png 内容必须真 PNG；T9 接线点）
    if let Some(bytes) = &avatar {
        let declared_ext = avatar_filename
            .as_deref()
            .and_then(|f| f.rsplit('.').next())
            .unwrap_or("png");
        if let Err(e) = super::uploads::check_extension(declared_ext) {
            return apply_cors_resp(
                err_json(StatusCode::BAD_REQUEST, &e.to_string()).into_response(),
                origin,
            );
        }
        if let Err(e) = super::uploads::check_magic(declared_ext, bytes) {
            return apply_cors_resp(
                err_json(StatusCode::BAD_REQUEST, &e.to_string()).into_response(),
                origin,
            );
        }
        if let Err(e) = super::uploads::check_size(bytes.len(), st.max_body_bytes) {
            return apply_cors_resp(
                err_json(StatusCode::BAD_REQUEST, &e.to_string()).into_response(),
                origin,
            );
        }
    }

    // charaFormatData：fields（含 JSON 值字段）→ Value
    let mut form = serde_json::Map::new();
    for (k, v) in &fields {
        form.insert(k.clone(), Value::String(v.clone()));
    }
    let char = chara_card::format_for_save(&Value::Object(form));
    let char_str = serde_json::to_string(&char).unwrap_or_default();

    // internalName：file_name 优先，否则 getPngName(ch_name)
    let internal = match fields.get("file_name").filter(|s| !s.trim().is_empty()) {
        Some(f) => crate::pathguard::sanitize(f),
        None => get_png_name(&characters_dir(st), &safe_name),
    };
    if internal.is_empty() {
        return apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "内部名净化后为空").into_response(),
            origin,
        );
    }
    let avatar_name = format!("{internal}.png");

    // chats 目录（ST create:1035：存在则不建）
    let cdir = chats_dir(st).join(&internal);
    if !cdir.exists() {
        let _ = std::fs::create_dir_all(&cdir);
    }

    let input: Vec<u8> = avatar.clone().unwrap_or_else(png::minimal_png);
    match write_character_data(st, &input, &char_str, &internal) {
        Ok(_) => {
            // 返回纯文本 avatarName（补 .png 的语义在前端 data.ts:110）
            let resp = (StatusCode::OK, avatar_name.clone());
            apply_cors_resp(resp.into_response(), origin)
        }
        Err(e) => apply_cors_resp(
            err_json(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
            origin,
        ),
    }
}

/// POST /api/characters/edit（manifest #8）—— 全字段重写卡内容；可选新头像。
/// 成功 200 空 body（ST sendStatus(200)）。
pub async fn edit(
    st: &BackendState,
    content_type: Option<&str>,
    body: axum::body::Bytes,
    origin: Option<HeaderValue>,
) -> Response {
    let fields: std::collections::HashMap<String, String>;
    let avatar: Option<Vec<u8>>;
    if content_type
        .map(|c| c.starts_with("multipart/form-data"))
        .unwrap_or(false)
    {
        match parse_multipart(content_type.unwrap(), body).await {
            Ok(m) => {
                fields = m.fields;
                avatar = m.avatar;
            }
            Err(e) => {
                return apply_cors_resp(
                    err_json(StatusCode::BAD_REQUEST, &e).into_response(),
                    origin,
                )
            }
        }
    } else {
        match serde_json::from_slice::<Value>(&body) {
            Ok(v) => {
                fields = v
                    .as_object()
                    .map(|o| {
                        o.iter()
                            .filter(|(_, v)| v.is_string() || v.is_boolean() || v.is_number())
                            .map(|(k, v)| {
                                let s = match v {
                                    Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                (k.clone(), s)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                avatar = None;
            }
            Err(_) => return apply_cors_resp(bad_json_body().into_response(), origin),
        }
    }
    let get = |k: &str| fields.get(k).map(|s| s.as_str()).unwrap_or("");

    // ch_name 校验（characters.js:1108-1112）
    let ch_name = get("ch_name");
    if ch_name.is_empty() || ch_name == "." {
        return apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "invalid name").into_response(),
            origin,
        );
    }
    let avatar_url = get("avatar_url");
    if avatar_url.is_empty() {
        return apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "缺少 avatar_url").into_response(),
            origin,
        );
    }
    let old_path = match character_path(st, avatar_url) {
        Ok(p) => p,
        Err(e) => {
            return apply_cors_resp(err_json(StatusCode::FORBIDDEN, &e).into_response(), origin)
        }
    };
    if !old_path.is_file() {
        return apply_cors_resp(
            err_json(StatusCode::NOT_FOUND, "角色卡不存在").into_response(),
            origin,
        );
    }
    // 上传了新头像 → 魔数校验
    if let Some(bytes) = &avatar {
        if let Err(e) = super::uploads::check_magic("png", bytes) {
            return apply_cors_resp(
                err_json(StatusCode::BAD_REQUEST, &e.to_string()).into_response(),
                origin,
            );
        }
    }

    // 保留原卡的 chat / create_date（ST edit:1115-1116 原样带过），
    // 其余字段按表单重写
    let mut form = serde_json::Map::new();
    for (k, v) in &fields {
        form.insert(k.clone(), Value::String(v.clone()));
    }
    if let Ok(raw) = std::fs::read(&old_path) {
        if let Ok(meta) = png::read_chara(&raw) {
            if let Ok(orig) = serde_json::from_str::<Value>(&meta) {
                for k in ["chat", "create_date"] {
                    if let Some(v) = orig.get(k).and_then(|v| v.as_str()) {
                        form.insert(k.into(), Value::String(v.to_string()));
                    }
                }
            }
        }
    }
    let char = chara_card::format_for_save(&Value::Object(form));
    let char_str = serde_json::to_string(&char).unwrap_or_default();

    let target_stem = avatar_url.trim_end_matches(".png");
    let input = avatar
        .clone()
        .map(Ok::<Vec<u8>, String>)
        .unwrap_or_else(|| std::fs::read(&old_path).map_err(|e| format!("读取原卡失败：{e}")));
    let image = match input {
        Ok(b) => b,
        Err(e) => {
            return apply_cors_resp(
                err_json(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
                origin,
            )
        }
    };
    match write_character_data(st, &image, &char_str, target_stem) {
        Ok(_) => apply_cors_resp(StatusCode::OK.into_response(), origin),
        Err(e) => apply_cors_resp(
            err_json(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
            origin,
        ),
    }
}

/// POST /api/characters/import（manifest #5）—— multipart `avatar` 文件 + `file_type`。
/// 支持 json/png（yaml/charx/byaf 在族四；ST 支持 6 种，灰度按连续性清单 §1.1 降级）。
/// 返回 JSON `{file_name}`（前端 data.ts:133 消费）；失败 `{error:true}`（400）。
pub async fn import(
    st: &BackendState,
    content_type: Option<&str>,
    body: axum::body::Bytes,
    origin: Option<HeaderValue>,
) -> Response {
    let origin_ref = origin.clone();
    let error_true = move |code: StatusCode| -> Response {
        apply_cors_resp(
            (code, Json(json!({ "error": true }))).into_response(),
            origin_ref.clone(),
        )
    };
    if !content_type
        .map(|c| c.starts_with("multipart/form-data"))
        .unwrap_or(false)
    {
        return error_true(StatusCode::BAD_REQUEST);
    }
    let m = match parse_multipart(content_type.unwrap(), body).await {
        Ok(m) => m,
        Err(_) => return error_true(StatusCode::BAD_REQUEST),
    };
    let Some(file_bytes) = m.avatar else {
        return error_true(StatusCode::BAD_REQUEST);
    };
    let Some(_file_name) = m.avatar_filename.clone() else {
        return error_true(StatusCode::BAD_REQUEST);
    };
    let format = m
        .fields
        .get("file_type")
        .cloned()
        .unwrap_or_default()
        .to_ascii_lowercase();

    // 大小校验（S15）
    if super::uploads::check_size(file_bytes.len(), st.max_body_bytes).is_err() {
        return error_true(StatusCode::BAD_REQUEST);
    }

    let now_iso = iso8601_from_unix(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    );

    // 统一的落地路径：归一化 JSON → getPngName → write_character_data（默认图兜底）
    let import_card = |data: &str, display_name: &str| -> Option<Response> {
        let safe = crate::pathguard::sanitize(display_name);
        if safe.is_empty() {
            return None;
        }
        let png_name = get_png_name(&characters_dir(st), &safe);
        let mut card: Value = serde_json::from_str(data).ok()?;
        if let Some(o) = card.as_object_mut() {
            o.insert("name".into(), json!(safe));
            if let Some(d) = o.get_mut("data").and_then(|d| d.as_object_mut()) {
                d.insert("name".into(), json!(safe));
            }
            o.insert("create_date".into(), json!(now_iso.clone()));
        }
        let char_str = serde_json::to_string(&card).ok()?;
        let image = if format == "png" {
            file_bytes.clone()
        } else {
            png::minimal_png()
        };
        write_character_data(st, &image, &char_str, &png_name).ok()?;
        Some(apply_cors_resp(
            Json(json!({ "file_name": format!("{png_name}.png") })).into_response(),
            origin.clone(),
        ))
    };

    match format.as_str() {
        "json" => {
            // BOM 剥除（前端 data.ts:123-127 的既有兼容在服务端做实）
            let text = String::from_utf8_lossy(&file_bytes);
            let text = text.trim_start_matches('\u{FEFF}');
            let json_data: Value = match serde_json::from_str(text) {
                Ok(v) => v,
                Err(_) => return error_true(StatusCode::BAD_REQUEST),
            };
            let display = json_data
                .get("data")
                .and_then(|d| d.get("name"))
                .and_then(|n| n.as_str())
                .or_else(|| json_data.get("name").and_then(|n| n.as_str()))
                .unwrap_or("")
                .to_string();
            if display.is_empty() {
                return error_true(StatusCode::BAD_REQUEST);
            }
            // spec 缺失 → 走 chara_card::to_v2_response（V1 形态收敛）
            let normalized = if json_data.get("spec").is_some() {
                json_data
            } else {
                chara_card::to_v2_response(json_data)
            };
            let data = serde_json::to_string(&normalized).unwrap_or_default();
            import_card(&data, &display).unwrap_or_else(|| error_true(StatusCode::BAD_REQUEST))
        }
        "png" => {
            // 魔数校验（S16：导入面同样不做豁免）
            if super::uploads::check_magic("png", &file_bytes).is_err() {
                return error_true(StatusCode::BAD_REQUEST);
            }
            let meta = match png::read_chara(&file_bytes) {
                Ok(m) => m,
                Err(_) => return error_true(StatusCode::BAD_REQUEST),
            };
            let json_data: Value = match serde_json::from_str(&meta) {
                Ok(v) => v,
                Err(_) => return error_true(StatusCode::BAD_REQUEST),
            };
            let display = json_data
                .get("data")
                .and_then(|d| d.get("name"))
                .and_then(|n| n.as_str())
                .or_else(|| json_data.get("name").and_then(|n| n.as_str()))
                .unwrap_or("")
                .to_string();
            if display.is_empty() {
                return error_true(StatusCode::BAD_REQUEST);
            }
            let normalized = if json_data.get("spec").is_some() {
                json_data
            } else {
                chara_card::to_v2_response(json_data)
            };
            let data = serde_json::to_string(&normalized).unwrap_or_default();
            import_card(&data, &display).unwrap_or_else(|| error_true(StatusCode::BAD_REQUEST))
        }
        // yaml/charx/byaf：族四（灰度降级 —— 明确 501，不伪造成功）
        other => {
            let resp = (
                StatusCode::NOT_IMPLEMENTED,
                Json(json!({
                    "error": {
                        "message": format!("导入格式 {other} 尚未迁移（Phase 1 族四范围）"),
                        "code": "NOT_MIGRATED",
                    }
                })),
            );
            apply_cors_resp(resp.into_response(), origin)
        }
    }
}

/// POST /api/characters/export {avatar_url, format}（manifest #8 附属）。
/// png → 私有字段清理后的 PNG 字节流（attachment）；json → 归一化 JSON。
pub fn export(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(format) = body.get("format").and_then(|v| v.as_str()) else {
        return apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "缺少 format").into_response(),
            origin,
        );
    };
    let Some(avatar) = body.get("avatar_url").and_then(|v| v.as_str()) else {
        return apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "缺少 avatar_url").into_response(),
            origin,
        );
    };
    let p = match character_path(st, avatar) {
        Ok(p) => p,
        Err(e) => {
            return apply_cors_resp(err_json(StatusCode::FORBIDDEN, &e).into_response(), origin)
        }
    };
    if !p.is_file() {
        return apply_cors_resp(
            err_json(StatusCode::NOT_FOUND, "角色卡不存在").into_response(),
            origin,
        );
    }
    let raw = match std::fs::read(&p) {
        Ok(r) => r,
        Err(e) => {
            return apply_cors_resp(
                err_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("读取失败：{e}"))
                    .into_response(),
                origin,
            )
        }
    };
    // unsetPrivateFields（characters.js:498-502）：fav=false + 移除 chat
    let sanitized_card = |meta: &str| -> Option<Value> {
        let mut card: Value = serde_json::from_str(meta).ok()?;
        let o = card.as_object_mut()?;
        o.insert("fav".into(), json!(false));
        o.remove("chat");
        if let Some(ext) = o
            .get_mut("data")
            .and_then(|d| d.as_object_mut())
            .and_then(|d| d.get_mut("extensions"))
            .and_then(|e| e.as_object_mut())
        {
            ext.insert("fav".into(), json!(false));
        }
        Some(card)
    };
    match format {
        "png" => {
            let meta = match png::read_chara(&raw) {
                Ok(m) => m,
                Err(e) => {
                    return apply_cors_resp(
                        err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
                        origin,
                    )
                }
            };
            let cleaned = match sanitized_card(&meta) {
                Some(c) => serde_json::to_string(&c).unwrap_or(meta),
                None => meta,
            };
            let out = match png::write_chara(&raw, &cleaned) {
                Ok(b) => b,
                Err(e) => {
                    return apply_cors_resp(
                        err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
                        origin,
                    )
                }
            };
            let mut resp = (StatusCode::OK, out).into_response();
            resp.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("image/png"),
            );
            let fname = format!("attachment; filename=\"{}\"", urlencoding_file_name(avatar));
            if let Ok(v) = HeaderValue::from_str(&fname) {
                resp.headers_mut()
                    .insert(axum::http::header::CONTENT_DISPOSITION, v);
            }
            apply_cors_resp(resp, origin)
        }
        "json" => {
            let meta = match png::read_chara(&raw) {
                Ok(m) => m,
                Err(_) => {
                    return apply_cors_resp(
                        err_json(StatusCode::BAD_REQUEST, "卡读取失败").into_response(),
                        origin,
                    )
                }
            };
            let v2 = match sanitized_card(&meta).map(chara_card::to_v2_response) {
                Some(v) => v,
                None => {
                    return apply_cors_resp(
                        err_json(StatusCode::BAD_REQUEST, "卡 JSON 失败").into_response(),
                        origin,
                    )
                }
            };
            let pretty = serde_json::to_string_pretty(&v2).unwrap_or_default();
            let mut resp = (StatusCode::OK, pretty).into_response();
            resp.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            apply_cors_resp(resp, origin)
        }
        _ => apply_cors_resp(
            err_json(StatusCode::BAD_REQUEST, "不支持的导出格式").into_response(),
            origin,
        ),
    }
}

/// 文件名 ASCII 化（Content-Disposition 里非 ASCII 用下划线替；对齐 encodeURI 兜底语义）
fn urlencoding_file_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-()".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// bad_json 的 characters 内副本（避免跨模块借用 api.rs 私有函数）
fn bad_json_body() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": { "message": "请求体不是合法 JSON" } })),
    )
        .into_response()
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

    fn st_with(tag: &str) -> BackendState {
        let root = std::env::temp_dir().join(format!("stchat-chars-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("default-user").join("characters")).unwrap();
        std::fs::create_dir_all(root.join("default-user").join("chats")).unwrap();
        BackendState {
            token: std::sync::Arc::new("t".into()),
            data_root: root,
            max_body_bytes: 1024,
            settings_svc: std::sync::Arc::new(super::super::settings::SettingsService::new()),
            chats_svc: new_chats(),
            maid_tokens: new_maid(),
          vault: std::sync::Arc::new(super::super::vault::Vault::new_memory()),
        }
    }

    /// 造一张 v2 卡 PNG（用 compat 的编码器）
    fn make_card(dir: &std::path::Path, name: &str, char_name: &str) {
        let png0 = crate::compat::png::minimal_png();
        let data = serde_json::json!({
            "spec": "chara_card_v2",
            "name": char_name,
            "description": "d",
            "data": { "name": char_name, "description": "d", "extensions": {} },
        })
        .to_string();
        let png = crate::compat::png::write_chara(&png0, &data).unwrap();
        std::fs::write(dir.join(name), &png).unwrap();
    }

    #[test]
    fn all_lists_cards_with_stats() {
        let st = st_with("all");
        let chars = characters_dir(&st);
        make_card(&chars, "a.png", "甲");
        make_card(&chars, "b.png", "乙");
        // 无 name 的坏卡：写入一个没有 name 的 v2 卡
        let png0 = crate::compat::png::minimal_png();
        let bad = crate::compat::png::write_chara(&png0, r#"{"spec":"chara_card_v2","data":{}}"#)
            .unwrap();
        std::fs::write(chars.join("c.png"), &bad).unwrap();
        // chats 统计
        std::fs::create_dir_all(chats_dir(&st).join("a")).unwrap();
        std::fs::write(chats_dir(&st).join("a").join("x.jsonl"), "{}\n{}\n").unwrap();

        let r = all(&st, None);
        assert_eq!(r.status().as_u16(), 200);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2, "无名卡被过滤");
        let a = arr.iter().find(|c| c["avatar"] == "a.png").unwrap();
        assert_eq!(a["name"], "甲");
        assert_eq!(a["chat_size"], 6);
        assert!(
            a["create_date"].as_str().unwrap().ends_with('Z'),
            "ISO 字符串"
        );
        assert!(a["json_data"].as_str().unwrap().contains("甲"));
    }

    #[test]
    fn get_404_and_forbidden() {
        let st = st_with("get");
        let r = get(&st, &json!({ "avatar_url": "../../evil.png" }), None);
        assert_eq!(r.status().as_u16(), 403);
        let r = get(&st, &json!({ "avatar_url": "nope.png" }), None);
        assert_eq!(r.status().as_u16(), 404);
    }

    #[test]
    fn chats_error_true_when_dir_missing() {
        let st = st_with("chats");
        let r = chats(&st, &json!({ "avatar_url": "x.png" }), None);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], true, "无目录时应返回 error=true");
    }

    #[test]
    fn delete_removes_card_and_chats() {
        let st = st_with("delete");
        let chars = characters_dir(&st);
        make_card(&chars, "a.png", "甲");
        std::fs::create_dir_all(chats_dir(&st).join("a")).unwrap();
        std::fs::write(chats_dir(&st).join("a").join("c.jsonl"), "{}\n").unwrap();

        let r = delete(
            &st,
            &json!({ "avatar_url": "a.png", "delete_chats": true }),
            None,
        );
        assert_eq!(r.status().as_u16(), 200);
        assert!(!chars.join("a.png").exists());
        assert!(!chats_dir(&st).join("a").exists(), "会话目录连带删除");
        // 恶意名 403
        let r = delete(&st, &json!({ "avatar_url": "..\\evil.png" }), None);
        assert_eq!(r.status().as_u16(), 403);
    }

    #[test]
    fn rename_creates_new_and_removes_old() {
        let st = st_with("rename");
        let chars = characters_dir(&st);
        make_card(&chars, "old.png", "旧名");
        std::fs::create_dir_all(chats_dir(&st).join("old")).unwrap();
        std::fs::write(chats_dir(&st).join("old").join("c.jsonl"), "{}\n").unwrap();

        let r = rename(
            &st,
            &json!({ "avatar_url": "old.png", "new_name": "新名" }),
            None,
        );
        assert_eq!(r.status().as_u16(), 200);
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let new_avatar = v["avatar"].as_str().unwrap();
        assert!(new_avatar.starts_with("新名"), "{new_avatar}");
        assert!(!chars.join("old.png").exists());
        assert!(chars.join(new_avatar).is_file());
        // 新卡内 name 已改
        let raw = std::fs::read(chars.join(new_avatar)).unwrap();
        let meta = crate::compat::png::read_chara(&raw).unwrap();
        assert!(meta.contains("新名"));
        // chats 目录迁移
        assert!(!chats_dir(&st).join("old").exists());
        assert!(chats_dir(&st)
            .join(new_avatar.trim_end_matches(".png"))
            .join("c.jsonl")
            .is_file());
    }

    #[test]
    fn rename_collision_appends_suffix() {
        let st = st_with("collision");
        let chars = characters_dir(&st);
        make_card(&chars, "a.png", "A");
        make_card(&chars, "dup.png", "D");
        // 把 a.png 改名成 dup → 应得到 dup1.png（getPngName 语义）
        let r = rename(
            &st,
            &json!({ "avatar_url": "a.png", "new_name": "dup" }),
            None,
        );
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["avatar"], "dup1.png");
    }

    /* ---- 族三：create / edit / import / export ---- */

    /// 构造 multipart/form-data body（测试用，含 avatar 文件字段与文本字段）
    fn make_multipart(
        boundary: &str,
        file_name: Option<&str>,
        file: Option<&[u8]>,
        fields: &[(&str, &str)],
    ) -> (String, Vec<u8>) {
        let mut body = Vec::new();
        let push = |body: &mut Vec<u8>, s: &str| body.extend_from_slice(s.as_bytes());
        if let (Some(fn_), Some(fbytes)) = (file_name, file) {
            push(&mut body, &format!("--{boundary}\r\n"));
            push(
                &mut body,
                &format!("Content-Disposition: form-data; name=\"avatar\"; filename=\"{fn_}\"\r\n"),
            );
            push(&mut body, "Content-Type: image/png\r\n\r\n");
            body.extend_from_slice(fbytes);
            push(&mut body, "\r\n");
        }
        for (k, v) in fields {
            push(&mut body, &format!("--{boundary}\r\n"));
            push(
                &mut body,
                &format!("Content-Disposition: form-data; name=\"{k}\"\r\n\r\n"),
            );
            push(&mut body, v);
            push(&mut body, "\r\n");
        }
        push(&mut body, &format!("--{boundary}--\r\n"));
        (format!("multipart/form-data; boundary={boundary}"), body)
    }

    #[tokio::test]
    async fn create_json_and_multipart() {
        let st = st_with("create");

        // JSON 形态（无头像 → 默认图兜底）
        let r = create(
            &st,
            Some("application/json"),
            axum::body::Bytes::from(json!({ "ch_name": "林晚棠", "description": "d" }).to_string()),
            None,
        )
        .await;
        assert_eq!(r.status().as_u16(), 200);
        let bytes = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let name = String::from_utf8(bytes.to_vec()).unwrap();
        assert_eq!(name, "林晚棠.png", "返回纯文本 avatarName");
        assert!(characters_dir(&st).join(&name).is_file());
        assert!(chats_dir(&st).join("林晚棠").is_dir(), "chats 目录同步创建");

        // multipart 形态 + 真 PNG 头像
        let png0 = crate::compat::png::minimal_png();
        let (ct, body) = make_multipart(
            "XyZ",
            Some("avatar.png"),
            Some(&png0),
            &[("ch_name", "乙"), ("description", "multipart 形态")],
        );
        let r = create(&st, Some(&ct), axum::body::Bytes::from(body), None).await;
        assert_eq!(r.status().as_u16(), 200);
        let bytes = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let name2 = String::from_utf8(bytes.to_vec()).unwrap();
        assert_eq!(name2, "乙.png");
        // 头像字节保留（写入的就是上传的 PNG）
        let raw = std::fs::read(characters_dir(&st).join(&name2)).unwrap();
        assert_eq!(&raw[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    }

    #[tokio::test]
    async fn create_rejects_fake_png_magic() {
        // S16 核心场景：声明 png 实为 HTML/SVG → 400
        let st = st_with("fakemagic");
        let (ct, body) = make_multipart(
            "XyZ",
            Some("evil.png"),
            Some(b"<svg onload=alert(1)>"),
            &[("ch_name", "恶意卡")],
        );
        let r = create(&st, Some(&ct), axum::body::Bytes::from(body), None).await;
        assert_eq!(r.status().as_u16(), 400, "魔数不符必须拒绝");
        assert!(
            characters_dir(&st)
                .read_dir()
                .map(|d| d.count())
                .unwrap_or(0)
                == 0,
            "未落盘"
        );
    }

    #[tokio::test]
    async fn create_rejects_empty_name() {
        let st = st_with("emptyname");
        let r = create(
            &st,
            Some("application/json"),
            axum::body::Bytes::from(json!({ "ch_name": "  " }).to_string()),
            None,
        )
        .await;
        assert_eq!(r.status().as_u16(), 400);
    }

    #[tokio::test]
    async fn edit_preserves_chat_and_create_date() {
        let st = st_with("edit");
        let chars = characters_dir(&st);
        make_card(&chars, "a.png", "A");
        // 先给原卡写入 chat/create_date
        let raw = std::fs::read(chars.join("a.png")).unwrap();
        let meta = crate::compat::png::read_chara(&raw).unwrap();
        let mut card: Value = serde_json::from_str(&meta).unwrap();
        card["chat"] = json!("A - 20260101-000000");
        card["create_date"] = json!("2026-01-01T00:00:00.000Z");
        let fixed = serde_json::to_string(&card).unwrap();
        let out = crate::compat::png::write_chara(&raw, &fixed).unwrap();
        std::fs::write(chars.join("a.png"), &out).unwrap();

        // edit：只改 description（JSON 形态）
        let r = edit(
            &st,
            Some("application/json"),
            axum::body::Bytes::from(
                json!({
                    "avatar_url": "a.png",
                    "ch_name": "A",
                    "description": "改过的描述",
                })
                .to_string(),
            ),
            None,
        )
        .await;
        assert_eq!(r.status().as_u16(), 200);

        // 读回：chat/create_date 保留，description 已改
        let raw = std::fs::read(chars.join("a.png")).unwrap();
        let meta = crate::compat::png::read_chara(&raw).unwrap();
        let card: Value = serde_json::from_str(&meta).unwrap();
        assert_eq!(card["description"], "改过的描述");
        assert_eq!(card["chat"], "A - 20260101-000000", "chat 原样带过");
        assert_eq!(
            card["create_date"], "2026-01-01T00:00:00.000Z",
            "create_date 原样带过"
        );
    }

    #[tokio::test]
    async fn import_json_roundtrip() {
        let st = st_with("import");
        // v2 JSON（带 BOM —— 前端 data.ts:123 的兼容场景在服务端做实）
        let card_json = "\u{FEFF}".to_string()
            + &serde_json::json!({
                "spec": "chara_card_v2",
                "name": "导入卡",
                "data": { "name": "导入卡", "description": "d" },
            })
            .to_string();
        let (ct, body) = make_multipart(
            "BnD",
            Some("card.json"),
            Some(card_json.as_bytes()),
            &[("file_type", "json")],
        );
        let r = import(&st, Some(&ct), axum::body::Bytes::from(body), None).await;
        assert_eq!(r.status().as_u16(), 200);
        let bytes = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["file_name"], "导入卡.png");
        assert!(characters_dir(&st).join("导入卡.png").is_file());

        // V1 JSON → 收敛为 v2
        let v1 = serde_json::json!({ "name": "v1卡", "description": "旧格式" }).to_string();
        let (ct, body) = make_multipart(
            "BnD2",
            Some("v1.json"),
            Some(v1.as_bytes()),
            &[("file_type", "json")],
        );
        let r = import(&st, Some(&ct), axum::body::Bytes::from(body), None).await;
        assert_eq!(r.status().as_u16(), 200);

        // 重名导入 → getPngName 加序号
        let (ct, body) = make_multipart(
            "BnD3",
            Some("card2.json"),
            Some(card_json.as_bytes()),
            &[("file_type", "json")],
        );
        let r = import(&st, Some(&ct), axum::body::Bytes::from(body), None).await;
        let bytes = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["file_name"], "导入卡1.png");
    }

    #[tokio::test]
    async fn import_png_and_unsupported_format() {
        let st = st_with("importpng");
        // PNG 导入：真卡 PNG
        let src = crate::compat::png::minimal_png();
        let data = serde_json::json!({
            "spec": "chara_card_v2",
            "name": "png卡",
            "data": { "name": "png卡" },
        })
        .to_string();
        let card_png = crate::compat::png::write_chara(&src, &data).unwrap();
        let (ct, body) = make_multipart(
            "PnD",
            Some("card.png"),
            Some(&card_png),
            &[("file_type", "png")],
        );
        let r = import(&st, Some(&ct), axum::body::Bytes::from(body), None).await;
        assert_eq!(r.status().as_u16(), 200);

        // 伪 PNG（无 chara chunk）→ 400
        let (ct, body) = make_multipart(
            "PnD2",
            Some("fake.png"),
            Some(&crate::compat::png::minimal_png()),
            &[("file_type", "png")],
        );
        let r = import(&st, Some(&ct), axum::body::Bytes::from(body), None).await;
        assert_eq!(r.status().as_u16(), 400);

        // charx（族四）→ 501 NOT_MIGRATED（S20 降级）
        let (ct, body) = make_multipart(
            "PnD3",
            Some("x.charx"),
            Some(b"PK\x03\x04"),
            &[("file_type", "charx")],
        );
        let r = import(&st, Some(&ct), axum::body::Bytes::from(body), None).await;
        assert_eq!(r.status().as_u16(), 501);
        let bytes = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], "NOT_MIGRATED");
    }

    #[test]
    fn export_png_strips_private_fields() {
        let st = st_with("export");
        let chars = characters_dir(&st);
        // 带私有字段的卡
        let src = crate::compat::png::minimal_png();
        let data = serde_json::json!({
            "spec": "chara_card_v2",
            "name": "导出卡",
            "chat": "导出卡 - 1",
            "fav": true,
            "data": { "name": "导出卡", "extensions": { "fav": true } },
        })
        .to_string();
        let png = crate::compat::png::write_chara(&src, &data).unwrap();
        std::fs::write(chars.join("e.png"), &png).unwrap();

        let r = export(
            &st,
            &json!({ "avatar_url": "e.png", "format": "png" }),
            None,
        );
        assert_eq!(r.status().as_u16(), 200);
        // 响应头：image/png + attachment（S18）
        assert_eq!(
            r.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("image/png")
        );
        assert!(
            r.headers()
                .get("content-disposition")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.starts_with("attachment;"))
                .unwrap_or(false),
            "导出必须 attachment（S18）"
        );

        // 导出的卡无 chat、fav=false
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        let meta = crate::compat::png::read_chara(&bytes).unwrap();
        let card: Value = serde_json::from_str(&meta).unwrap();
        assert!(card.get("chat").is_none(), "chat 已剥离");
        assert_eq!(card["fav"], false);
        assert_eq!(card["data"]["extensions"]["fav"], false);
    }

    #[test]
    fn export_json_and_404() {
        let st = st_with("exportjson");
        let chars = characters_dir(&st);
        make_card(&chars, "j.png", "J");
        let r = export(
            &st,
            &json!({ "avatar_url": "j.png", "format": "json" }),
            None,
        );
        assert_eq!(r.status().as_u16(), 200);
        assert_eq!(
            r.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
        // 不存在 → 404
        let r = export(
            &st,
            &json!({ "avatar_url": "nope.png", "format": "json" }),
            None,
        );
        assert_eq!(r.status().as_u16(), 404);
        // 非法格式 → 400
        let r = export(
            &st,
            &json!({ "avatar_url": "j.png", "format": "xml" }),
            None,
        );
        assert_eq!(r.status().as_u16(), 400);
    }
}
