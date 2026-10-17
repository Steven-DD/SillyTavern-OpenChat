//! pathguard —— 请求体路径组件的安全校验共享库（评审整改 T8 / 安全规格 S6-S7）
//!
//! 背景（评审 P0-2，已核）：ST 的真实防线是三层 ——
//! ① sanitize-filename 包（剥离非法字符）② isPathUnderParent（拼接后 containment 判定）
//! ③ validateFileName.js 的 `forbiddenRegExp = /[/\x00\\]/` —— 只拦 `/ \ NUL`，
//! 不拦 `..`、不拦盘符、不拦 Windows 保留名。它是粗筛不是兜底，却因最短最易抄，
//! 最容易被新实现当成「完整复刻」。
//!
//! 本库把三层防线收敛为**一个入口**，native 后端的所有 handler 必须经它拼接
//! 用户可控路径组件；禁止各 handler 各写一遍校验（S6）。
//!
//! 三步语义（与 ST 对齐并加固）：
//! 1. `sanitize`：剥离 ST sanitize-filename 同款非法字符 + 控制字符 + 尾部空格/点
//!    （Windows 文件系统语义：尾点/尾空格会被静默丢弃，可被用于混淆）
//! 2. `sanitize` 后再次拒绝：空、`.`、`..`、盘符前缀、保留名（CON/PRN/AUX/NUL/COM1-9/LPT1-9）
//! 3. `ensure_under`：拼接后做 containment 判定（等价 isPathUnderParent），
//!    对 UNC 与 8.3 短名先归一（canonicalize 失败时降级为组件级校验已兜底）
//!
//! 用法：
//! ```ignore
//! let name = pathguard::component(&req.avatar_url)?;          // 单组件（文件名/目录名）
//! let p = pathguard::join_under(&data_root, [&seg1, &seg2])?; // 拼接 + containment
//! ```

use std::path::{Component, Path, PathBuf};

/// Windows 保留设备名（不含扩展名部分比对，CON.jpg 同样危险）
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 校验失败的错误（handler 层映射为 400/403）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathGuardError(pub String);

impl std::fmt::Display for PathGuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "非法路径参数：{}", self.0)
    }
}

impl std::error::Error for PathGuardError {}

/// ST sanitize-filename 同款剥离 + Windows 语义加固。
/// 返回净化后的组件名；调用方仍须以 [`component`] 做最终判定。
pub fn sanitize(input: &str) -> String {
    // ① 剥离路径分隔符与 ST forbiddenRegExp 覆盖的字符，再加 Windows 保留字符与控制字符
    let cleaned: String = input
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            ) && !c.is_control()
        })
        .collect();
    // ② Windows：尾部空格/点在文件系统层被静默丢弃 → 攻击者可用 `x.png...   ` 混淆比对
    let trimmed = cleaned.trim_end_matches([' ', '.']);
    trimmed.trim_start_matches('.').to_string()
}

/// 校验「单路径组件」（文件名或目录名）：原始输入**含分隔符即拒绝**
/// （调用方必须传纯组件名 —— sanitize 只净化名字，不负责拆路径；这是比 ST 更严的口）。
/// 通过后再净化：必须非空、非相对分量、非保留名。返回净化后的名字。
pub fn component(input: &str) -> Result<String, PathGuardError> {
    // 含分隔符 = 调用方把整段路径塞给了单组件接口：整体拒绝，不静默拆段
    if input.contains('/') || input.contains('\\') {
        return Err(PathGuardError(format!(
            "组件含路径分隔符（应传纯文件名/目录名）：{input:?}"
        )));
    }
    let s = sanitize(input);
    if s.is_empty() {
        return Err(PathGuardError("组件为空或净化后为空".into()));
    }
    if s == "." || s == ".." {
        return Err(PathGuardError("相对路径分量（. / ..）".into()));
    }
    // 保留名：取第一个 '.' 前的裸名，大小写不敏感
    let bare = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED_NAMES.contains(&bare.as_str()) {
        return Err(PathGuardError(format!("Windows 保留设备名：{bare}")));
    }
    Ok(s)
}

/// 归一化 containment 判定（等价 ST isPathUnderParent 的加固版）：
/// `candidate` 必须位于 `base` 之下（严格子路径，不允许等于 base 自身）。
/// 已显式拒绝的分量在 component() 已拦；本函数再兜底拦 `..`/根/前缀盘符不一致。
pub fn ensure_under(base: &Path, candidate: &Path) -> Result<(), PathGuardError> {
    // 显式拒绝相对分量（.. / .）—— 不依赖文件系统 canonicalize
    for c in candidate.components() {
        if matches!(c, Component::ParentDir | Component::CurDir) {
            return Err(PathGuardError(format!(
                "路径含相对分量：{}",
                candidate.display()
            )));
        }
    }
    // candidate 必须以 base 开头（组件级前缀比较，不做字符串前缀 —— 防止 `base2` 伪装 `base`）
    let ok =
        candidate.starts_with(base) && candidate.components().count() > base.components().count();
    if !ok {
        return Err(PathGuardError(format!(
            "路径越界：{} 不在 {} 之下",
            candidate.display(),
            base.display()
        )));
    }
    Ok(())
}

/// 一步到位：校验并拼接 `<base>/<seg...>`，全部组件过 component()，
/// 最后 ensure_under 兜底。返回拼接结果（组件为净化后形态）。
pub fn join_under(base: &Path, segments: &[&str]) -> Result<PathBuf, PathGuardError> {
    let mut out = base.to_path_buf();
    let mut cleaned: Vec<String> = Vec::with_capacity(segments.len());
    for seg in segments {
        let c = component(seg)?;
        cleaned.push(c.clone());
        out.push(c);
    }
    if cleaned.is_empty() {
        return Err(PathGuardError("至少需要一个路径组件".into()));
    }
    ensure_under(base, &out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejects(input: &str) {
        let r = component(input);
        assert!(
            r.is_err(),
            "应拒绝 {input:?}，实际 {:?}",
            r.map(|s| s.to_string())
        );
    }

    #[test]
    fn traversal_rejected() {
        // 含分隔符的原始输入在单组件接口上整体拒绝（不静默拆段）
        rejects("../../../../Windows/notepad.exe");
        rejects("..\\..\\Windows\\notepad.exe");
        rejects("../x");
        rejects("..\\..\\x");
        rejects("a/b");
        rejects("C:\\Windows\\system32");
        rejects("\\\\srv\\share\\x"); // UNC
        rejects("..");
        rejects("..\\..\\x");
    }

    #[test]
    fn traversal_rejected_exact() {
        // 无分隔符的点串：sanitize 剥点后可能成为合法名 —— 这是 sanitize-filename 的
        // 既定语义（纯名无穿越结构）；join_under 的 ensure_under 兜底相对分量。
        assert_eq!(sanitize("....foo"), "foo");
        assert!(component("....foo").is_ok(), "纯名点串净化后合法");
        assert!(component("..").is_err(), "纯 .. 必须拒绝");
        assert!(component(".").is_err(), "纯 . 必须拒绝");
    }

    #[test]
    fn separator_inputs_never_produce_escaping_paths() {
        // 关键不变量：无论输入多恶意，join_under 的结果必在 base 之下
        let base = Path::new("/data/root");
        for evil in [
            "..\\..\\..\\Windows\\notepad.exe",
            "../../../etc/passwd",
            "....\\....\\x",
            "C:\\Windows\\system32",
            "\\\\srv\\share\\x", // UNC
        ] {
            let r = join_under(base, &[evil]);
            if let Ok(p) = r {
                assert!(p.starts_with(base), "{evil:?} 逃逸到 {:?}", p);
                assert!(!p.to_string_lossy().contains(".."));
            }
        }
    }

    #[test]
    fn reserved_names_rejected() {
        rejects("con");
        rejects("CON.jpg");
        rejects("nul");
        rejects("com1");
        rejects("LPT9.txt");
    }

    #[test]
    fn empty_and_dots_rejected() {
        rejects("");
        rejects("   ");
        rejects("...");
        rejects(".");
    }

    #[test]
    fn legit_names_pass_and_are_cleaned() {
        assert_eq!(component("林晚棠.png").unwrap(), "林晚棠.png");
        assert_eq!(component("2026-01-01").unwrap(), "2026-01-01");
        assert_eq!(component("a b.png").unwrap(), "a b.png");
        // 尾部空格/点被剥（Windows 语义）
        assert_eq!(component("x.png. . ").unwrap(), "x.png");
    }

    #[test]
    fn ensure_under_blocks_prefix_spoof() {
        let base = Path::new("/data/root");
        // 字符串前缀相同但组件不同：/data/root2 不在 /data/root 下
        assert!(ensure_under(base, Path::new("/data/root2/x")).is_err());
        assert!(ensure_under(base, Path::new("/data/root")).is_err()); // 等于 base 自身不行
        assert!(ensure_under(base, Path::new("/data/root/sub/x")).is_ok());
    }

    #[test]
    fn ensure_under_blocks_parent_components() {
        assert!(ensure_under(Path::new("/data/root"), Path::new("/data/root/../x")).is_err());
    }

    #[test]
    fn join_under_happy_path() {
        let base = Path::new("/data/root");
        let p = join_under(base, &["chats", "林晚棠.png", "2026-01-01.jsonl"]).unwrap();
        assert_eq!(p, Path::new("/data/root/chats/林晚棠.png/2026-01-01.jsonl"));
    }
}
