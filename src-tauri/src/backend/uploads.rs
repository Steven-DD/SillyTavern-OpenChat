//! backend/uploads —— 上传资产校验（评审整改 T9 / 安全规格 S16-S18）
//!
//! ST 侧现状（评审 P0-6 已核）：multer 无 fileFilter、limits 用 fieldSize 500MB
//! （fileSize 实际无上限）、images.js 只查扩展名白名单不查魔数。
//! native 后端借机修正为：扩展名白名单（剔除视频）+ 魔数嗅探 + fileSize 上限
//! + 服务端生成落盘名 + 按魔数判 Content-Type + nosniff + 非图片 attachment。
//!
//! 魔数嗅探表为静态资源返回与上传校验共用（单一来源）。
//! BMP（BM→image/bmp）在本表覆盖。

/// 允许上传的图片扩展名（对齐 ST MEDIA_EXTENSIONS 并**剔除视频**）
pub const ALLOWED_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "jfif"];

/// 上传校验失败原因
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadError(pub String);

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "上传被拒绝：{}", self.0)
    }
}
impl std::error::Error for UploadError {}

/// S16-①：扩展名白名单（大小写不敏感；剔除视频）
pub fn check_extension(format: &str) -> Result<(), UploadError> {
    let f = format.trim().to_ascii_lowercase();
    if ALLOWED_EXTENSIONS.contains(&f.as_str()) {
        Ok(())
    } else if ["mp4", "avi", "mov", "wmv", "flv", "webm", "mkv"].contains(&f.as_str()) {
        Err(UploadError(format!("视频格式不再允许上传（{f}）")))
    } else {
        Err(UploadError(format!("不支持的图片格式（{f}）")))
    }
}

/// S16-②：魔数嗅探 —— 声明格式必须与内容一致（
/// 判定表；base64 解码后的首字节）
pub fn check_magic(declared: &str, head: &[u8]) -> Result<(), UploadError> {
    let actual = sniff_image_mime(head);
    let declared = format!("image/{}", declared.trim().to_ascii_lowercase());
    // jpeg/jfif 同族：jfif 是 JPEG 交换格式，声明任一都按 image/jpeg 判
    let declared = if declared == "image/jfif" {
        "image/jpeg".to_string()
    } else {
        declared
    };
    match actual {
        Some(m) if m == declared => Ok(()),
        Some(m) => Err(UploadError(format!(
            "内容与声明不符：声明 {declared}，实际 {m}（魔数不符）"
        ))),
        None => Err(UploadError("无法识别的内容格式（非已知图片魔数）".into())),
    }
}

/// S15：fileSize 上限校验（解码后字节）
pub fn check_size(len: usize, max: usize) -> Result<(), UploadError> {
    if len == 0 {
        return Err(UploadError("空文件".into()));
    }
    if len > max {
        return Err(UploadError(format!(
            "文件超过上限（{} 字节 > {} 字节）",
            len, max
        )));
    }
    Ok(())
}

/// S17：落盘文件名服务端生成。用户提供的 filename 仅作参考 —— 取其扩展名，
/// 且该扩展名必须在白名单内（不在即拒绝，不静默替换：静默替换会掩盖声明与
/// 内容不一致的攻击）；主体用 `{ch_name}_{毫秒时间戳}`（对齐 ST images.js 语义）。
/// ch_name 过 pathguard::sanitize。返回安全文件名。
pub fn server_filename(
    ch_name: &str,
    provided: Option<&str>,
    fallback_ext: &str,
) -> Result<String, UploadError> {
    let ext = match provided.map(|p| p.rsplit('.').next().unwrap_or("").to_ascii_lowercase()) {
        Some(e) if !e.is_empty() => e,
        _ => fallback_ext.to_ascii_lowercase(),
    };
    if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
        return Err(UploadError(format!("扩展名不合法（{ext}）")));
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    // ch_name 过 pathguard（剥分隔符/控制字符/尾点），允许中文
    let safe_ch = crate::pathguard::sanitize(ch_name);
    if safe_ch.is_empty() {
        return Err(UploadError("ch_name 净化后为空".into()));
    }
    Ok(format!("{safe_ch}_{ts}.{ext}"))
}

/// S18：静态返回的响应头 —— Content-Type 按魔数判定（嗅探失败回落扩展名映射）、
/// nosniff 恒置、非图片 attachment。
pub fn static_response_headers(
    sniffed: Option<&'static str>,
    ext_lower: &str,
) -> Vec<(&'static str, String)> {
    let ctype = sniffed
        .map(|s| s.to_string())
        .unwrap_or_else(|| ext_to_mime(ext_lower).to_string());
    vec![
        ("content-type", ctype),
        ("x-content-type-options", "nosniff".to_string()),
    ]
}

fn ext_to_mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// 按魔数判断真实图片类型（嗅探逻辑单一来源：静态资源返回与上传校验共用此表）。
/// BMP：`BM` 魔数（S18 整改项）。
pub fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, ..] => Some("image/png"),
        [b'G', b'I', b'F', b'8', _, b'a', ..] => Some("image/gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        [b'B', b'M', ..] => Some("image/bmp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_whitelist() {
        assert!(check_extension("png").is_ok());
        assert!(check_extension("JPEG").is_ok());
        assert!(check_extension("jfif").is_ok());
        // 明确拒绝视频类型，即使其包含在 ST 白名单中。
        assert!(check_extension("mp4").is_err());
        assert!(check_extension("avi").is_err());
        assert!(check_extension("exe").is_err());
        assert!(check_extension("").is_err());
    }

    #[test]
    fn magic_must_match_declaration() {
        // 真 PNG 内容 + 声明 png → 过
        assert!(check_magic("png", &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]).is_ok());
        // SVG/HTML 内容声明 png → 拒（评审 P0-6 的核心攻击面）
        assert!(check_magic("png", b"<svg onload=alert(1)>").is_err());
        assert!(check_magic("png", b"<html><body>x</body></html>").is_err());
        // JPEG 内容声明 png → 拒（ ST 缩略图事故的逆场景 ）
        assert!(check_magic("png", &[0xFF, 0xD8, 0xFF, 0xE0]).is_err());
        // jfif 声明按 jpeg 判
        assert!(check_magic("jfif", &[0xFF, 0xD8, 0xFF, 0xE0, b'J', b'F', b'I', b'F']).is_ok());
        // 无法识别 → 拒
        assert!(check_magic("png", b"\x00\x01\x02\x03").is_err());
        assert!(check_magic("png", b"").is_err());
    }

    #[test]
    fn size_limit() {
        assert!(check_size(1, 100).is_ok());
        assert!(check_size(0, 100).is_err());
        assert!(check_size(101, 100).is_err());
    }

    #[test]
    fn server_generates_filename() {
        let a = server_filename("林晚棠", Some("自命名.PNG"), "png").unwrap();
        assert!(a.starts_with("林晚棠_"), "{a}");
        assert!(a.ends_with(".png"), "{a}");
        // 两次生成不重名（毫秒时间戳）
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = server_filename("林晚棠", Some("自命名.PNG"), "png").unwrap();
        assert_ne!(a, b);
        // 恶意 filename 的扩展名被检
        assert!(server_filename("x", Some("evil.exe"), "png").is_err());
        // ch_name 过 sanitize（分隔符剥除）
        let c = server_filename("a/b", None, "png").unwrap();
        assert!(!c.contains('/'));
        // 「../..」经 sanitize 剥点后为空 → 显式拒绝（pathguard 兜底）
        assert!(server_filename("../..", None, "png").is_err(), "净化后为空必须拒绝");
    }

    #[test]
    fn static_headers_have_nosniff() {
        let h = static_response_headers(Some("image/png"), "png");
        assert!(h
            .iter()
            .any(|(k, v)| *k == "content-type" && v == "image/png"));
        assert!(h
            .iter()
            .any(|(k, v)| *k == "x-content-type-options" && v == "nosniff"));
        // 嗅探失败回落扩展名映射
        let h2 = static_response_headers(None, "gif");
        assert!(h2
            .iter()
            .any(|(k, v)| *k == "content-type" && v == "image/gif"));
    }

    #[test]
    fn sniff_table_covers_core_formats() {
        assert_eq!(sniff_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(
            sniff_image_mime(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            Some("image/png")
        );
    }
}
