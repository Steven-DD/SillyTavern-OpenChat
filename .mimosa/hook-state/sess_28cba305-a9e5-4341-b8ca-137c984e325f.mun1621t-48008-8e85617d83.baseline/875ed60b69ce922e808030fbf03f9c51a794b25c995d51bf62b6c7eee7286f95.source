//! 插件探测层
//!
//! 负责回答一个问题：**这个插件现在能不能用、绑的是哪个文件、版本满足要求吗？**
//!
//! 探测优先级（对应「用户本机已装就别下载」的设计）：
//!   1. 用户显式绑定（manual 路径）
//!   2. 系统 PATH 查找（process 型）
//!   3. 清单里声明的常见安装位置（search_paths）
//!   4. 安装根目录邻近扫描（install_root 本身 / 一级子目录 / plugins 下两层）
//!   5. 用户常见位置扫描（家目录 / Desktop / Documents / Downloads / 各盘根，
//!      按 `dir_name_patterns` 匹配目录名）
//!
//! 隐私边界：第 4、5 步**只看固定位置的目录名 + marker 文件**，
//! 不做递归遍历、不读文件内容（marker 仅判断存在性）。
//! 命中与否都会在 message 里带上来源，方便用户排查。
//!
//! 版本约束校验对**所有来源一视同仁** —— 系统里探测到 node v16 而要求 >=20，
//! 就不能假装满足（否则会「看起来能用、一跑就崩」）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use super::{Manifest, ProbeSpec, VersionFrom};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/* ------------------------------------------------------------------ *
 * 版本工具
 * ------------------------------------------------------------------ */

/// 解析形如 `v22.22.2` / `1.12.10` / `git version 2.51.0.windows.1` 的版本号（取前 3 段数字）
pub fn parse_version(s: &str) -> Option<Vec<u64>> {
    let mut out = Vec::new();
    for part in s.split(|c: char| !c.is_ascii_digit() && c != '.') {
        if part.is_empty() {
            continue;
        }
        for seg in part.split('.') {
            if seg.is_empty() {
                continue;
            }
            match seg.parse::<u64>() {
                Ok(n) => out.push(n),
                Err(_) => break,
            }
            if out.len() == 3 {
                return Some(out);
            }
        }
        if !out.is_empty() {
            return Some(out);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn cmp_ver(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
    for i in 0..3 {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

/// 版本约束校验。支持 `>=20`、`>= 20.0.0`、`>1.2`、`<=3`、`=2.0.0`，
/// 以及逗号分隔的组合（AND）。裸版本号视为 `>=`。
pub fn satisfies(version: &str, constraint: &str) -> bool {
    use std::cmp::Ordering;
    let Some(v) = parse_version(version) else {
        return false;
    };
    for raw in constraint.split(',') {
        let part = raw.trim();
        if part.is_empty() {
            continue;
        }
        let (op, rest) = ["<=", ">=", "<", ">", "="]
            .iter()
            .find_map(|op| part.strip_prefix(op).map(|r| (*op, r)))
            .unwrap_or((">=", part));
        let Some(t) = parse_version(rest.trim()) else {
            return false;
        };
        let ord = cmp_ver(&v, &t);
        let ok = match op {
            ">=" => ord != Ordering::Less,
            ">" => ord == Ordering::Greater,
            "<=" => ord != Ordering::Greater,
            "<" => ord == Ordering::Less,
            "=" => ord == Ordering::Equal,
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    true
}

/* ------------------------------------------------------------------ *
 * 探测结果
 * ------------------------------------------------------------------ */

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeOutcome {
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// 版本是否满足约束（无约束时为 true）
    pub satisfies: bool,
    /// 给用户看的一句话
    pub message: String,
}

impl ProbeOutcome {
    pub fn missing(msg: impl Into<String>) -> Self {
        Self {
            found: false,
            path: None,
            version: None,
            satisfies: false,
            message: msg.into(),
        }
    }
}

/* ------------------------------------------------------------------ *
 * 目录候选兜底扫描（L4 安装根邻近 / L5 常见位置）
 * ------------------------------------------------------------------ */

/// 目录名匹配模式：大小写不敏感；尾部 `*` = 前缀匹配，头部 `*` = 后缀匹配，否则全等
fn name_matches(name: &str, pattern: &str) -> bool {
    let n = name.to_lowercase();
    let p = pattern.to_lowercase();
    if let Some(prefix) = p.strip_suffix('*') {
        n.starts_with(prefix)
    } else if let Some(suffix) = p.strip_prefix('*') {
        n.ends_with(suffix)
    } else {
        n == p
    }
}

/// 目录是否满足全部 marker（只判断存在性，不读内容）
fn has_all_markers(dir: &Path, markers: &[String]) -> bool {
    markers.iter().all(|mk| dir.join(mk).is_file())
}

/// 扫描 base 的一级子目录：名字命中任一 pattern 且 marker 齐全 → 返回
fn scan_children(base: &Path, patterns: &[String], markers: &[String]) -> Option<PathBuf> {
    let entries = std::fs::read_dir(base).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let matched = e
            .file_name()
            .to_str()
            .map(|n| patterns.iter().any(|pat| name_matches(n, pat)))
            .unwrap_or(false);
        if matched && has_all_markers(&p, markers) {
            return Some(p);
        }
    }
    None
}

/// 扫描 base 的一级子目录：不按名字过滤，marker 齐全即命中
///（用于 `plugins/<组件>/<版本>` 的版本层 —— 版本目录名是 `1.19.0` 这类，不匹配组件名模式）
fn scan_children_markers_only(base: &Path, markers: &[String]) -> Option<PathBuf> {
    let entries = std::fs::read_dir(base).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() && has_all_markers(&p, markers) {
            return Some(p);
        }
    }
    None
}

/// 用户家目录（Windows 用 USERPROFILE，其余用 HOME）
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// 盘符根（Windows = A:\..Z:\ 中存在的；其余平台 = /）。
/// 只 stat 根目录本身，不遍历内容。
#[cfg(windows)]
fn drive_roots() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|c| PathBuf::from(format!("{}:\\", c as char)))
        .filter(|p| p.is_dir())
        .collect()
}

#[cfg(not(windows))]
fn drive_roots() -> Vec<PathBuf> {
    [PathBuf::from("/")]
        .into_iter()
        .filter(|p| p.is_dir())
        .collect()
}

/// L5 的固定候选位置：家目录 / Desktop / Documents / Downloads / 各盘根
fn user_bases() -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Some(home) = home_dir() {
        bases.push(home.clone());
        for sub in ["Desktop", "Documents", "Downloads"] {
            let d = home.join(sub);
            if d.is_dir() {
                bases.push(d);
            }
        }
    }
    bases.extend(drive_roots());
    bases
}

/// 对一组基目录逐个做一级子目录扫描（名字模式 + marker 校验）
fn scan_bases(bases: &[PathBuf], patterns: &[String], markers: &[String]) -> Option<PathBuf> {
    for base in bases {
        if let Some(hit) = scan_children(base, patterns, markers) {
            return Some(hit);
        }
    }
    None
}

/// 兜底探测：固定浅层位置 + 目录名模式 + marker 校验。
/// 只在 `patterns` 非空时生效；任何一步失败都静默跳过（探测不该报错）。
fn scan_common(
    install_root: Option<&Path>,
    patterns: &[String],
    markers: &[String],
) -> Option<(PathBuf, &'static str)> {
    if patterns.is_empty() {
        return None;
    }

    // L4：安装根目录邻近 —— 根目录本身 / 一级子目录 / plugins 下两层
    if let Some(root) = install_root {
        if has_all_markers(root, markers) {
            return Some((root.to_path_buf(), "安装根目录"));
        }
        if let Some(hit) = scan_children(root, patterns, markers) {
            return Some((hit, "安装根目录邻近"));
        }
        let plugins = root.join("plugins");
        if let Some(hit) = scan_children(&plugins, patterns, markers) {
            return Some((hit, "安装根目录邻近"));
        }
        // plugins 下再深一层（覆盖 plugins/<组件>/<版本>，如宿主安装的版本目录）
        if let Ok(comps) = std::fs::read_dir(&plugins) {
            for comp in comps.flatten() {
                let cp = comp.path();
                if cp.is_dir() {
                    if let Some(hit) = scan_children(&cp, patterns, markers)
                        .or_else(|| scan_children_markers_only(&cp, markers))
                    {
                        return Some((hit, "安装根目录邻近"));
                    }
                }
            }
        }
    }

    // L5：用户常见位置
    scan_bases(&user_bases(), patterns, markers).map(|p| (p, "常见安装位置"))
}

/* ------------------------------------------------------------------ *
 * 查找
 * ------------------------------------------------------------------ */

/// 在 PATH 中查找可执行文件（Windows 会按 PATHEXT 补扩展名）
fn find_in_path(names: &[String]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect()
    } else {
        Vec::new()
    };

    for dir in std::env::split_paths(&path) {
        for name in names {
            let direct = dir.join(name);
            if direct.is_file() {
                return Some(direct);
            }
            // 只在「名字本身没带扩展名」时补 PATHEXT，避免 node.exe.exe 这种荒谬组合
            if cfg!(windows) && Path::new(name).extension().is_none() {
                for e in &exts {
                    let cand = dir.join(format!("{name}{e}"));
                    if cand.is_file() {
                        return Some(cand);
                    }
                }
            }
        }
    }
    None
}

/// 展开 `%VAR%` / `$VAR` 形式的路径
fn expand(path: &str) -> PathBuf {
    if cfg!(windows) {
        let mut out = String::with_capacity(path.len());
        let bytes: Vec<char> = path.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == '%' {
                if let Some(end) = bytes[i + 1..].iter().position(|c| *c == '%') {
                    let name: String = bytes[i + 1..i + 1 + end].iter().collect();
                    match std::env::var(&name) {
                        Ok(v) => out.push_str(&v),
                        Err(_) => out.push_str(&format!("%{name}%")),
                    }
                    i = i + end + 2;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        PathBuf::from(out)
    } else {
        let mut out = path.to_string();
        for (k, v) in std::env::vars() {
            out = out.replace(&format!("${k}"), &v);
        }
        PathBuf::from(out)
    }
}

/// 执行 `exe args` 并取版本号
fn read_version(exe: &Path, spec: &ProbeSpec) -> Option<String> {
    let args = if spec.version_args.is_empty() {
        vec!["--version".to_string()]
    } else {
        spec.version_args.clone()
    };

    let mut cmd = std::process::Command::new(exe);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let out = cmd.output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let pattern = spec
        .version_pattern
        .clone()
        .unwrap_or_else(|| r"(\d+\.\d+(\.\d+)?)".to_string());
    let re = regex::Regex::new(&pattern).ok()?;
    re.captures(&text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// 读目录型插件的版本（例如 ST 的 package.json → version）
fn read_dir_version(dir: &Path, from: &VersionFrom) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(&from.file)).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    // 支持 "version" 或 "a.b" 形式的浅路径
    let mut cur = &json;
    for seg in from.json_path.split('.') {
        cur = cur.get(seg)?;
    }
    cur.as_str().map(|s| s.to_string())
}

/* ------------------------------------------------------------------ *
 * 主探测
 * ------------------------------------------------------------------ */

/// 规范化路径：去掉 `..` 片段，并剥掉 Windows `canonicalize` 会加的 `\\?\` 前缀。
///
/// 显示与执行共用这一份路径 —— 避免出现「界面显示 A、实际跑 B」的认知错位。
fn normalize(p: &Path) -> PathBuf {
    let canon = match std::fs::canonicalize(p) {
        Ok(c) => c,
        Err(_) => return p.to_path_buf(),
    };
    let text = canon.to_string_lossy().into_owned();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => canon,
    }
}

/// 探测一个插件。
/// * `manual`：用户显式绑定的路径（优先）
/// * `install_root`：宿主安装根目录（auto 兜底扫描 L4 用，显式绑定传 None）
/// * `cached`：上次 auto 探测的命中路径（快查用，仍要过 marker 校验）
pub fn probe(
    m: &Manifest,
    manual: Option<&Path>,
    install_root: Option<&Path>,
    cached: Option<&Path>,
) -> ProbeOutcome {
    let spec = &m.probe;
    let is_dir_plugin = !spec.dir_markers.is_empty();

    // 1) 用户显式绑定优先
    let mut candidate: Option<PathBuf> = None;
    let mut origin = "";

    if let Some(p) = manual {
        if p.exists() {
            candidate = Some(p.to_path_buf());
            origin = "用户指定";
        } else {
            return ProbeOutcome::missing(format!("绑定的路径不存在：{}", p.display()));
        }
    }

    // 1.5) auto 缓存快查：上次探测的命中路径。省掉整条扫描链，
    // 但 marker 校验照做 —— 目录被挪走/删掉就当没命中，回退全链。
    if candidate.is_none() {
        if let Some(c) = cached {
            let ok = if is_dir_plugin {
                spec.dir_markers.iter().all(|mk| c.join(mk).is_file())
            } else {
                c.is_file()
            };
            if ok {
                candidate = Some(c.to_path_buf());
                origin = "缓存命中";
            }
        }
    }

    // 2) 系统 PATH
    if candidate.is_none() && !spec.commands.is_empty() {
        if let Some(p) = find_in_path(&spec.commands) {
            candidate = Some(p);
            origin = "系统 PATH";
        }
    }

    // 3) 清单声明的常见位置
    if candidate.is_none() {
        for raw in &spec.search_paths {
            let p = expand(raw);
            let hit = if is_dir_plugin {
                spec.dir_markers.iter().all(|mk| p.join(mk).is_file())
            } else {
                p.is_file()
            };
            if hit {
                candidate = Some(p);
                origin = "常见安装位置";
                break;
            }
        }
    }

    // 4/5) 兜底扫描：安装根目录邻近 + 用户常见位置（仅目录型，仅固定浅层）
    if candidate.is_none() && is_dir_plugin {
        if let Some((p, o)) = scan_common(install_root, &spec.dir_name_patterns, &spec.dir_markers)
        {
            candidate = Some(p);
            origin = o;
        }
    }

    let Some(path) = candidate.map(|p| normalize(&p)) else {
        // 文案就要这么短：组件名即结论（「未找到 SillyTavern」/「未找到 node」）。
        // 修复路径由前端负责：启动屏「仍要进入」直达插件页，那里有手动指定与重新检测。
        return ProbeOutcome::missing(format!("未找到 {}", m.name));
    };

    // 目录型插件：校验 marker
    if is_dir_plugin {
        let bad: Vec<&str> = spec
            .dir_markers
            .iter()
            .filter(|mk| !path.join(mk.as_str()).is_file())
            .map(|s| s.as_str())
            .collect();
        if !bad.is_empty() {
            return ProbeOutcome {
                found: true,
                path: Some(path.to_string_lossy().into_owned()),
                version: None,
                satisfies: false,
                message: format!("目录缺少必需文件：{}", bad.join(", ")),
            };
        }
    }

    // 版本
    let version = if is_dir_plugin {
        spec.version_from
            .as_ref()
            .and_then(|vf| read_dir_version(&path, vf))
    } else {
        read_version(&path, spec)
    };

    // 约束校验（对所有来源一视同仁）
    let constraints: Vec<String> = spec.min_version.iter().cloned().collect();
    // 注意别叫 satisfies —— 会遮蔽上面的同名函数
    let mut version_ok = true;
    let mut fail_msg = String::new();
    if let Some(v) = version.as_ref() {
        for c in &constraints {
            if !satisfies(v, c) {
                version_ok = false;
                fail_msg = format!("版本 {v} 不满足要求 {c}");
                break;
            }
        }
    } else if !constraints.is_empty() {
        version_ok = false;
        fail_msg = "无法读取版本，无法校验要求".to_string();
    }

    let message = if !version_ok {
        fail_msg
    } else if let Some(v) = version.as_ref() {
        format!("{origin} · {v}")
    } else {
        format!("{origin} · 已就绪")
    };

    ProbeOutcome {
        found: true,
        path: Some(path.to_string_lossy().into_owned()),
        version,
        satisfies: version_ok,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parse_variants() {
        assert_eq!(parse_version("v22.22.2"), Some(vec![22, 22, 2]));
        assert_eq!(parse_version("1.12.10"), Some(vec![1, 12, 10]));
        assert_eq!(parse_version("git version 2.51.0.windows.1"), Some(vec![2, 51, 0]));
        assert_eq!(parse_version("v20"), Some(vec![20]));
        assert_eq!(parse_version("no digits here"), None);
    }

    #[test]
    fn constraint_matching() {
        assert!(satisfies("22.22.2", ">=20"));
        assert!(satisfies("20.0.0", ">=20.0.0"));
        assert!(!satisfies("16.20.0", ">=20"));
        assert!(!satisfies("19.9.9", ">=20.0.0"));
        assert!(satisfies("2.51.0", ">= 2.30.0"));
        assert!(satisfies("1.5.0", "<=2, >=1"));
        assert!(!satisfies("3.0.0", "<=2, >=1"));
        assert!(satisfies("2.0.0", "=2.0.0"));
        // 空约束 = 不限制
        assert!(satisfies("22.0.0", ""));
        // 无法解析的版本一律视为不满足（宁可不假装满足）
        assert!(!satisfies("not-a-version", ">=1"));
    }

    #[test]
    fn dir_name_pattern_matching() {
        assert!(name_matches("SillyTavern", "SillyTavern*"));
        assert!(name_matches("sillytavern-release", "SillyTavern*"));
        assert!(name_matches("SillyTavern-1.19.0", "sillytavern*"));
        assert!(!name_matches("MySillyTavern", "SillyTavern*"));
        assert!(name_matches("my-st", "*st"));
        assert!(name_matches("SillyTavern", "sillytavern"));
        assert!(!name_matches("SillyTavernX", "sillytavern"));
    }

    fn tmp_root(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "stchat-probe-test-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_fake_st(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("server.js"), "// fake").unwrap();
        std::fs::write(dir.join("package.json"), r#"{"name":"sillytavern","version":"1.19.0"}"#)
            .unwrap();
    }

    #[test]
    fn scan_finds_st_next_to_install_root() {
        let root = tmp_root("l4");
        // 模拟：install_root 下有 plugins/sillytavern/1.19.0（宿主安装布局）
        let st = root.join("plugins").join("sillytavern").join("1.19.0");
        make_fake_st(&st);

        let markers = vec!["server.js".to_string(), "package.json".to_string()];
        let patterns = vec!["SillyTavern*".to_string()];

        // L4：plugins 下两层
        let hit = scan_common(Some(&root), &patterns, &markers);
        assert!(hit.is_some(), "应命中 plugins/<组件>/<版本>");
        assert_eq!(hit.unwrap().0, st);

        // L4：根目录本身直接就是 ST（用户把安装根指到 ST 目录的场景）
        let st2 = tmp_root("l4b");
        make_fake_st(&st2);
        let hit2 = scan_common(Some(&st2), &patterns, &markers);
        assert_eq!(hit2.map(|h| h.0), Some(st2.clone()));

        // 无 patterns → 整个兜底关闭
        assert!(scan_common(Some(&root), &[], &markers).is_none());

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&st2);
    }

    #[test]
    fn scan_requires_markers_not_just_name() {
        let root = tmp_root("l4-marker");
        // 名字像 ST 但没有 marker 文件 → 不得命中
        let fake = root.join("SillyTavenport");
        std::fs::create_dir_all(&fake).unwrap();

        let markers = vec!["server.js".to_string(), "package.json".to_string()];
        let patterns = vec!["SillyTavern*".to_string()];
        assert!(scan_bases(std::slice::from_ref(&root), &patterns, &markers).is_none());

        // 名字命中且 marker 齐全 → 命中
        let real = root.join("SillyTavern");
        make_fake_st(&real);
        assert_eq!(
            scan_bases(std::slice::from_ref(&root), &patterns, &markers),
            Some(real.clone())
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    fn bare_dir_manifest() -> Manifest {
        serde_json::from_str(
            r#"{
            "id": "sillytavern", "name": "SillyTavern",
            "tier": "required", "kind": "process",
            "probe": { "dir_markers": ["server.js", "package.json"] }
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn cached_auto_path_is_used_and_validated() {
        let root = tmp_root("cache");
        let st = root.join("SillyTavern");
        make_fake_st(&st);
        let m = bare_dir_manifest();

        // 有效缓存 → 直接命中，不跑扫描链
        let out = probe(&m, None, None, Some(&st));
        assert!(out.found, "{}", out.message);
        assert!(out.message.contains("缓存命中"), "{}", out.message);

        // 缓存失效（marker 缺失）→ 当没命中，回退全链
        let bad = root.join("Empty");
        std::fs::create_dir_all(&bad).unwrap();
        let out2 = probe(&m, None, None, Some(&bad));
        assert!(!out2.found, "坏缓存不得命中：{}", out2.message);
        // 失败文案 = 简短结论（组件名即结论）
        assert_eq!(out2.message, "未找到 SillyTavern", "{}", out2.message);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_message_is_short_and_named() {
        // process 型且找不到：文案就是「未找到 Node.js」
        let m: Manifest = serde_json::from_str(
            r#"{
            "id": "node", "name": "Node.js",
            "tier": "required", "kind": "process",
            "probe": { "commands": ["definitely-not-exist-xyz"] }
        }"#,
        )
        .unwrap();
        let out = probe(&m, None, None, None);
        assert!(!out.found);
        assert_eq!(out.message, "未找到 Node.js", "{}", out.message);
    }
}
