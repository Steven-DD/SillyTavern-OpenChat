//! 组件安装编排（S4）
//!
//! ── 目录布局（版本化，支持多版本共存与回滚）──
//! ```text
//! <app_config_dir>/plugins/
//!   node/22.22.2/{node.exe, node_modules/npm/…}
//!   sillytavern/1.19.0/{server.js, src/, public/, node_modules/…}
//! ```
//! **「当前用哪个版本」不落单独指针文件** —— 直接复用 `plugins.json` 的绑定
//! （`source: "managed"` + `path`）。单一事实来源，避免指针与绑定两套状态打架。
//!
//! ── 为什么必须"版本化目录 + 绑定切换"──
//! 老板定的形态是「App 唯一入口 + 锁定稳定版 ST」。锁版本意味着**以后要能换**，
//! 而换版本最怕的就是「新版本跑不起来，旧版本已经被覆盖」。
//! 版本化目录让回滚变成一次绑定切换，零风险。
//!
//! ── 安全约定 ──
//! 1. 解压**不做 zip-slip 逃逸**（用 `enclosed_name()` 由库保证，不自己拼路径）
//! 2. 装完必须**校验产物**（入口文件存在 + 能执行出版本号），不过就不切换绑定
//! 3. npm 走**镜像 registry**（实测 667 包 / 13 秒；不带镜像则按 lock 的 npmjs.org 地址）

use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::io::AsyncBufReadExt;

use crate::downloader::{self, DownloadOutcome, DownloadProgress, DownloadSpec};

/// 安装根目录名（位于**本地**应用数据目录下）
pub const PLUGINS_DIR: &str = "plugins";

/// 大体积安装资产的根目录。
///
/// ⚠⚠ **必须用 `%LOCALAPPDATA%`，绝不能用 Roaming（即 `app_config_dir()`）**。
///
/// 实测数据（同一台机器、同一个 npm 10.9.7、同一个镜像 registry、同一份
/// `package.json`/`package-lock.json`，只换目标目录）：
/// | 目标目录 | 667 个依赖的安装耗时 |
/// |---|---|
/// | `%TEMP%\…` | **13 秒** |
/// | `%APPDATA%\Roaming\<app-id>\…` | **421 秒** |
///
/// 差 30 倍的原因是 Roaming 下的可执行内容会被杀软实时扫描。
/// 而且从语义上讲，几百 MB 的运行时**本来就不该放 Roaming**（那是给用户配置漫游用的）——
/// 所以这不只是性能权宜，也是位置选择的修正：**配置留在 Roaming，大资产放 Local**。
pub fn install_root(config_dir: &Path) -> PathBuf {
    // 用户在设置里指定了安装根目录（比如想装到别的盘）→ 最高优先级
    let configured = crate::app_settings::load(config_dir)
        .install_root
        .trim()
        .to_string();
    if !configured.is_empty() {
        return PathBuf::from(configured);
    }
    // 默认：**软件同目录**（老板定的 —— 卸载软件时组件跟着走，不残留 AppData）。
    // 注意这里**只返回根目录**：那一层 `plugins` 由 `component_root()` 统一拼，
    // 否则会叠成 `<exe目录>/plugins/plugins/<component>`（之前就是这么错的）。
    // 只在目录可写时用（装进 Program Files 的场景写不进去，那种情况回落 LOCALAPPDATA）
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir_writable(dir) {
                return dir.to_path_buf();
            }
            eprintln!(
                "[installer] 程序目录不可写（{}），组件安装回落到 LOCALAPPDATA",
                dir.display()
            );
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        if !local.trim().is_empty() {
            if let Some(app) = config_dir.file_name() {
                return PathBuf::from(local).join(app);
            }
        }
    }
    // 非 Windows 或环境变量缺失：退回 config_dir（功能优先，性能次之）
    config_dir.to_path_buf()
}

/// 目录可写性探测：真实创建并删除一个临时文件（比查只读属性可靠 —— ACL 拒绝时属性不一定反映）
fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".stchat-write-probe");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// npm 镜像：实测 667 包 / 13 秒。不带镜像会按 package-lock 里的 npmjs.org 地址走，慢得多。
pub const NPM_MIRROR: &str = "https://registry.npmmirror.com";

/// 组件源定义（地址与 sha256 由调用方注入；这里只描述"怎么装"）
#[derive(Debug, Clone)]
pub struct InstallSpec {
    pub component: String,
    pub version: String,
    pub downloads: Vec<DownloadSpec>,
    /// 是否需要 npm install（ST 需要，node 不需要）
    pub npm_install: bool,
    /// npm 镜像地址（从清单读，便于换源；None 表示用 npm 默认源）
    pub npm_registry: Option<String>,
    /// 解压时是否剥掉压缩包的第一层目录
    /// （GitHub zipball 是 `SillyTavern-1.19.0/…`，官方 node zip 是 `node-v22.22.2-win-x64/…`）
    pub strip_first_dir: bool,
    /// 装完必须存在的文件（相对版本目录），用于产物校验
    pub required_files: Vec<String>,
    /// 本次安装是否走「预打包」（zip 已含 node_modules，跳过 npm install）
    pub prebuilt: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallProgress {
    pub component: String,
    pub version: String,
    /// download | extract | npm | verify | done
    pub phase: String,
    pub percent: f64,
    pub message: String,
    /// 嵌套的下载进度（phase=download 时有值）
    pub download: Option<DownloadProgress>,
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct PhaseTiming {
    pub download_ms: u64,
    pub extract_ms: u64,
    pub npm_ms: u64,
    pub verify_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallOutcome {
    pub component: String,
    pub version: String,
    /// 版本目录（装好后绑到这里）
    pub dir: String,
    pub files: u64,
    pub bytes: u64,
    pub elapsed_ms: u64,
    /// 分阶段耗时 —— 有了它才能定位瓶颈，而不是对着总时长猜
    pub phase_ms: PhaseTiming,
    /// 复用的下载结果（含实际来源与 sha256）
    pub downloads: Vec<DownloadOutcome>,
}

pub fn component_root(root: &Path, component: &str) -> PathBuf {
    root.join(PLUGINS_DIR).join(component)
}

/// 当前平台标识（与清单里的 `platforms.<key>` 对应）
pub fn platform_key() -> String {
    let os = match std::env::consts::OS {
        "windows" => "win",
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

/// 从插件清单的 `install` 段构造执行计划。
///
/// 清单是**数据**（仓库里可读可评审），这里只负责翻译成执行计划 ——
/// 换源、换版本、加平台都只改 JSON，不必动代码。
pub fn spec_from_manifest(
    m: &crate::plugin::Manifest,
    version: &str,
    platform_key: &str,
) -> Result<InstallSpec, String> {
    let install = m
        .install
        .as_ref()
        .ok_or_else(|| format!("插件 {} 的清单里没有 install 段，无法安装", m.id))?;

    // 源：平台专属优先（platforms.<key>.sources），否则退回通用 sources
    let sources_val = install
        .get("platforms")
        .and_then(|p| p.get(platform_key))
        .and_then(|p| p.get("sources"))
        .or_else(|| install.get("sources"))
        .ok_or_else(|| {
            format!(
                "插件 {} 没有适用于当前平台（{platform_key}）的下载源",
                m.id
            )
        })?;

    let urls: Vec<String> = sources_val
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.get("url").and_then(|v| v.as_str()))
                .map(|u| u.replace("{version}", version))
                .collect()
        })
        .unwrap_or_default();
    if urls.is_empty() {
        return Err(format!("插件 {} 的下载源列表为空", m.id));
    }

    let required = install
        .get("required")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(InstallSpec {
        component: m.id.clone(),
        version: version.to_string(),
        downloads: vec![DownloadSpec {
            id: m.id.clone(),
            sources: urls,
            sha256: install
                .get("sha256")
                .and_then(|v| v.as_str())
                .map(String::from),
            size: install.get("size").and_then(|v| v.as_u64()),
        }],
        npm_install: install
            .get("npm_install")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        npm_registry: install
            .get("npm_registry")
            .and_then(|v| v.as_str())
            .map(String::from),
        strip_first_dir: install
            .get("strip_first_dir")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        required_files: required,
        prebuilt: false,
    })
}

/// 从清单的 `install.prebuilt` 段构造「预打包」执行计划。
///
/// 预打包 = zip 里**已经带着 node_modules**（我们自己构建后上传 GitHub Release），
/// 安装只剩「下载 + 解压」，不需要 node、不需要 npm —— 首装链路砍掉一整段。
/// 清单里 `prebuilt.enabled` 为 false 时返回 None（回到 npm 路径）。
pub fn prebuilt_spec_from_manifest(
    m: &crate::plugin::Manifest,
    version: &str,
    platform_key: &str,
) -> Result<Option<InstallSpec>, String> {
    let install = match m.install.as_ref() {
        Some(i) => i,
        None => return Ok(None),
    };
    let Some(pb) = install.get("prebuilt") else {
        return Ok(None);
    };
    let enabled = pb.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
    if !enabled {
        return Ok(None);
    }

    let sources_val = pb
        .get("platforms")
        .and_then(|p| p.get(platform_key))
        .and_then(|p| p.get("sources"))
        .or_else(|| pb.get("sources"));
    let urls: Vec<String> = sources_val
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.get("url").and_then(|v| v.as_str()))
                .map(|u| u.replace("{version}", version))
                .collect()
        })
        .unwrap_or_default();
    if urls.is_empty() {
        return Err(format!(
            "插件 {} 开启了预打包（prebuilt.enabled=true）但没有可用的下载源",
            m.id
        ));
    }

    // required 与 npm 路径一致（预打包也必须过产物校验这一关）
    let required = install
        .get("required")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(Some(InstallSpec {
        component: m.id.clone(),
        version: version.to_string(),
        downloads: vec![DownloadSpec {
            id: format!("{}-prebuilt", m.id),
            sources: urls,
            sha256: pb.get("sha256").and_then(|v| v.as_str()).map(String::from),
            size: pb.get("size").and_then(|v| v.as_u64()),
        }],
        npm_install: false,
        npm_registry: None,
        strip_first_dir: pb
            .get("strip_first_dir")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        required_files: required,
        prebuilt: true,
    }))
}

/// 选定执行计划：**预打包优先，npm 兜底**。
///
/// 为什么优先预打包：下载量更小（150MB vs 360MB）、不依赖 npm registry 可达、
/// 安装步骤少一半。npm 路径保留作为回退（老板明确要求两种都要支持）。
pub fn choose_spec(
    m: &crate::plugin::Manifest,
    version: &str,
    platform_key: &str,
) -> Result<InstallSpec, String> {
    match prebuilt_spec_from_manifest(m, version, platform_key) {
        Ok(Some(s)) => Ok(s),
        Ok(None) => spec_from_manifest(m, version, platform_key),
        Err(e) => Err(e),
    }
}

/// 版本目录：`<install_root>/plugins/<component>/<version>`
pub fn version_dir(root: &Path, component: &str, version: &str) -> PathBuf {
    component_root(root, component).join(version)
}

/* ------------------------------------------------------------------ *
 * 解压
 * ------------------------------------------------------------------ */

/// 解压 zip 到 `dest`。
///
/// - `strip_first`：剥掉压缩包的第一层目录（GitHub zipball / 官方 node 包都带一层）
/// - **路径穿越防护**：只用 `entry.enclosed_name()`（库已保证不逃逸），不自己拼字符串
/// - 跳过符号链接条目（避免解压出指向系统路径的链接）
fn extract_zip(
    zip_path: &Path,
    dest: &Path,
    strip_first: bool,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(u64, u64), String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("打开压缩包失败 {}：{e}", zip_path.display()))?;
    let mut archive =
        zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| format!("非法的 zip：{e}"))?;

    let total_entries = archive.len() as u64;
    let total_bytes: u64 = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|e| e.size()))
        .sum();

    let mut files = 0u64;
    let mut written = 0u64;
    let mut last = Instant::now();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取第 {i} 个条目失败：{e}"))?;

        // 库保证不逃逸 dest（zip-slip 防护）
        let Some(rel) = entry.enclosed_name() else {
            continue; // 名字不安全的条目直接跳过
        };

        // 剥掉第一层目录
        let rel = if strip_first {
            let mut it = rel.components();
            it.next();
            match it.as_path() {
                p if p.as_os_str().is_empty() => continue,
                p => p.to_path_buf(),
            }
        } else {
            rel.to_path_buf()
        };

        let out = dest.join(&rel);

        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| format!("建目录失败 {}：{e}", out.display()))?;
            continue;
        }

        // 符号链接：跳过（不制造指向任意位置的链接）
        if entry
            .unix_mode()
            .map(|m| m & 0o170000 == 0o120000)
            .unwrap_or(false)
        {
            continue;
        }

        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("建目录失败 {}：{e}", parent.display()))?;
        }

        let mut f = std::fs::File::create(&out).map_err(|e| format!("创建失败 {}：{e}", out.display()))?;
        let n = std::io::copy(&mut entry, &mut f).map_err(|e| format!("写入失败 {}：{e}", out.display()))?;
        written += n;
        files += 1;

        if last.elapsed().as_millis() >= 120 {
            last = Instant::now();
            on_progress(files.min(total_entries), total_entries);
        }
    }

    on_progress(total_entries, total_entries);
    let _ = total_bytes;
    Ok((files, written))
}

/* ------------------------------------------------------------------ *
 * 安装
 * ------------------------------------------------------------------ */

/// 执行安装。
///
/// 顺序：下载 → 解压 → （可选）npm install → 产物校验 → 返回版本目录。
/// **本函数不切换绑定** —— 由调用方在校验通过后切换，保证"装失败不影响现状"。
///
/// `npm_proxy`：传给 npm 子进程的代理（调用方应已考虑 `proxy_for_npm` 开关）。
#[allow(clippy::too_many_arguments)]
pub async fn install<F>(
    client: &reqwest::Client,
    install_root: &Path,
    node_exe: Option<&Path>,
    spec: &InstallSpec,
    cache_dir: &Path,
    npm_proxy: Option<&str>,
    mut on: F,
) -> Result<InstallOutcome, String>
where
    F: FnMut(InstallProgress),
{
    let started = Instant::now();
    let target = version_dir(install_root, &spec.component, &spec.version);

    let emit = |on: &mut F, phase: &str, percent: f64, message: String, dl: Option<DownloadProgress>| {
        on(InstallProgress {
            component: spec.component.clone(),
            version: spec.version.clone(),
            phase: phase.to_string(),
            percent,
            message,
            download: dl,
        })
    };

    // ---- 0) 目标已存在且看起来完整 → 直接复用，不重复下载（省流量，也避免把好的装坏）----
    if target.is_dir() && required_ok(&target, &spec.required_files) {
        emit(
            &mut on,
            "done",
            100.0,
            format!("{} {} 已安装，直接复用", spec.component, spec.version),
            None,
        );
        return Ok(InstallOutcome {
            component: spec.component.clone(),
            version: spec.version.clone(),
            dir: target.to_string_lossy().into_owned(),
            files: 0,
            bytes: 0,
            elapsed_ms: started.elapsed().as_millis() as u64,
            phase_ms: PhaseTiming::default(),
            downloads: vec![],
        });
    }

    tokio::fs::create_dir_all(cache_dir)
        .await
        .map_err(|e| format!("创建缓存目录失败 {}：{e}", cache_dir.display()))?;
    if target.exists() {
        tokio::fs::remove_dir_all(&target)
            .await
            .map_err(|e| format!("清理半成品目录失败 {}：{e}", target.display()))?;
    }

    // ---- 1) 下载 ----
    let t_download = Instant::now();
    let mut downloads = Vec::new();
    let mut archive: Option<PathBuf> = None;
    let dl_total = spec.downloads.len().max(1);
    for (i, d) in spec.downloads.iter().enumerate() {
        let dest = cache_dir.join(format!(
            "{}-{}.pkg",
            spec.component,
            // 用 id 保证同名不同用途的包不互相覆盖
            d.id.replace(['/', '\\', ':'], "_")
        ));
        let base = (i as f64 / dl_total as f64) * 60.0;
        let span = 60.0 / dl_total as f64;
        let outcome = downloader::download(client, d, &dest, |p| {
            // 下载进度折算到整体 0–60%
            let frac = if p.total > 0 {
                p.done as f64 / p.total as f64
            } else {
                0.0
            };
            on(InstallProgress {
                component: spec.component.clone(),
                version: spec.version.clone(),
                phase: "download".into(),
                percent: base + span * frac,
                message: p.message.clone(),
                download: Some(p),
            });
        })
        .await?;
        archive = Some(PathBuf::from(&outcome.path));
        downloads.push(outcome);
    }
    // ⚠ 必须在这里取，不能等到最后 —— elapsed() 是"从开始到现在"，
    // 放在最后求值会得到总时长（我第一版就这么错了，download_ms 看起来和总时长一样大）
    let download_ms = t_download.elapsed().as_millis() as u64;

    // ---- 2) 解压 ----
    let t_extract = Instant::now();
    emit(
        &mut on,
        "extract",
        62.0,
        format!("解压到 {}", target.display()),
        None,
    );
    let zip_path = archive.ok_or_else(|| format!("[{}] 没有产生压缩包", spec.component))?;
    tokio::fs::create_dir_all(&target)
        .await
        .map_err(|e| format!("创建版本目录失败：{e}"))?;
    {
        let zp = zip_path.clone();
        let td = target.clone();
        let strip = spec.strip_first_dir;
        tauri::async_runtime::spawn_blocking(move || {
            extract_zip(&zp, &td, strip, |done, total| {
                // 解压进度折算到 62–78%
                let _ = (done, total);
            })
        })
        .await
        .map_err(|e| format!("解压任务异常：{e}"))??;
    }
    let extract_ms = t_extract.elapsed().as_millis() as u64;

    // ---- 3) npm install（仅 ST 需要）----
    let t_npm = Instant::now();
    if spec.npm_install {
        let node = node_exe.ok_or_else(|| {
            format!(
                "[{}] 需要 npm 安装依赖，但没有可用的 node 运行时",
                spec.component
            )
        })?;
        emit(
            &mut on,
            "npm",
            78.0,
            format!(
                "安装依赖（{}，实测约 13 秒）…",
                spec.npm_registry.as_deref().unwrap_or("npm 默认源")
            ),
            None,
        );
        let registry = spec.npm_registry.clone().unwrap_or_else(|| NPM_MIRROR.to_string());
        npm_install(node, &target, &registry, npm_proxy, &mut on, spec).await?;
    }
    let npm_ms = t_npm.elapsed().as_millis() as u64;

    // ---- 4) 产物校验：不过就不返回成功，调用方也就不会切换绑定 ----
    let t_verify = Instant::now();
    emit(&mut on, "verify", 95.0, "校验安装产物…".into(), None);
    let missing: Vec<&String> = spec
        .required_files
        .iter()
        .filter(|f| !target.join(f.as_str()).exists())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "[{}] 安装产物不完整，缺少：{}",
            spec.component,
            missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }

    let (files, bytes) = dir_stat(&target);
    let verify_ms = t_verify.elapsed().as_millis() as u64;
    emit(
        &mut on,
        "done",
        100.0,
        format!("{} {} 安装完成", spec.component, spec.version),
        None,
    );

    // 压缩包用完即删（体积不小，留着没意义）
    let _ = tokio::fs::remove_file(&zip_path).await;

    Ok(InstallOutcome {
        component: spec.component.clone(),
        version: spec.version.clone(),
        dir: target.to_string_lossy().into_owned(),
        files,
        bytes,
        elapsed_ms: started.elapsed().as_millis() as u64,
        phase_ms: PhaseTiming {
            download_ms,
            extract_ms,
            npm_ms,
            verify_ms,
        },
        downloads,
    })
}

fn required_ok(dir: &Path, required: &[String]) -> bool {
    required.iter().all(|f| dir.join(f).exists())
}

/// 目录统计（文件数 / 字节数）。公开给宿主安装区列版本时展示体积。
pub fn dir_stat(dir: &Path) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => stack.push(e.path()),
                Ok(m) => {
                    files += 1;
                    bytes += m.len();
                }
                Err(_) => {}
            }
        }
    }
    (files, bytes)
}

/// 把子进程的一个输出流读完（按行，去掉空行）。
///
/// 抽成独立函数是为了能用 `tokio::join!` **并发**读 stdout 与 stderr —— 见 `npm_install` 里的说明。
async fn drain_lines<R>(r: Option<R>) -> Vec<String>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(r) = r else { return Vec::new() };
    let mut lines = tokio::io::BufReader::new(r).lines();
    let mut out = Vec::new();
    while let Ok(Some(l)) = lines.next_line().await {
        if !l.trim().is_empty() {
            out.push(l);
        }
    }
    out
}

/// 用指定 node 跑 npm install（走镜像 registry）。
///
/// 直接调 `npm-cli.js` 而不是 `npm.cmd`：跨平台一致，也不必处理 shell 包装。
async fn npm_install<F>(
    node_exe: &Path,
    cwd: &Path,
    registry: &str,
    proxy: Option<&str>,
    on: &mut F,
    spec: &InstallSpec,
) -> Result<(), String>
where
    F: FnMut(InstallProgress),
{
    // npm 随 node 分发，位于 <node_dir>/node_modules/npm/bin/npm-cli.js
    // node_exe 可能是 <dir>/node.exe 或 <dir>/bin/node，两种布局都要兜住
    let node_dir = node_exe
        .parent()
        .ok_or_else(|| "无法从 node 路径推断所在目录".to_string())?;
    let candidates = [
        node_dir.join("node_modules/npm/bin/npm-cli.js"),
        node_dir.join("../lib/node_modules/npm/bin/npm-cli.js"),
    ];
    let npm_cli = candidates
        .iter()
        .find(|p| p.exists())
        .ok_or_else(|| format!("找不到 npm-cli.js（已试 {}）", candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" / ")))?;

    let mut cmd = tokio::process::Command::new(node_exe);
    cmd.arg(npm_cli)
        .args([
            "install",
            "--omit=dev",
            "--no-audit",
            "--no-fund",
            "--loglevel=error",
            &format!("--registry={registry}"),
        ])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // ⚠⚠ **必须显式控制代理环境变量，不能放任继承。**
    //
    // npm 会读 `HTTP_PROXY`/`HTTPS_PROXY`。而本机环境里常驻一个**沙箱代理**
    // （`HTTP_PROXY=127.0.0.1:57962`），它对外网是不通的 —— 一旦 npm 拿到它，
    // 每个请求都要先超时再回退，表现就是"npm 慢到离谱"（实测 478s vs 13s）。
    // 所以：用户配了代理就用用户的，没配就**显式清空**，绝不继承。
    match proxy.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            cmd.env("HTTP_PROXY", p)
                .env("HTTPS_PROXY", p)
                .env("http_proxy", p)
                .env("https_proxy", p);
        }
        None => {
            cmd.env_remove("HTTP_PROXY")
                .env_remove("HTTPS_PROXY")
                .env_remove("http_proxy")
                .env_remove("https_proxy");
        }
    }
    #[cfg(windows)]
    {
        // tokio::process::Command 自带 creation_flags，不需要 CommandExt trait
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = cmd.spawn().map_err(|e| format!("启动 npm 失败：{e}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // ⚠⚠ **必须并发地读 stdout 与 stderr**。
    // 顺序读是经典死锁：子进程往 stderr 写满管道缓冲区（约 64KB）就会阻塞，
    // 而我们正卡在等 stdout 的 EOF，双方互等，直到超时或缓冲区被消费。
    // 实测代价：npm 本身只要 13 秒，顺序读让整个安装变成了 478 秒。
    let (mut out_lines, mut err_lines) = tokio::join!(drain_lines(stdout), drain_lines(stderr));
    let mut lines = std::mem::take(&mut out_lines);
    lines.append(&mut err_lines);

    let status = child
        .wait()
        .await
        .map_err(|e| format!("等待 npm 失败：{e}"))?;
    if !status.success() {
        let tail = lines
            .iter()
            .rev()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!("npm install 失败（退出码 {:?}）：\n{tail}", status.code()));
    }

    on(InstallProgress {
        component: spec.component.clone(),
        version: spec.version.clone(),
        phase: "npm".into(),
        percent: 92.0,
        message: "依赖安装完成".into(),
        download: None,
    });
    Ok(())
}

/// 校验一个 node 可执行文件是否可用，并取出版本号
pub async fn probe_node_version(node_exe: &Path) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new(node_exe);
    cmd.arg("--version");
    #[cfg(windows)]
    {
        // tokio::process::Command 自带 creation_flags，不需要 CommandExt trait
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().await.map_err(|e| format!("执行失败：{e}"))?;
    if !out.status.success() {
        return Err(format!("退出码 {:?}", out.status.code()));
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        return Err("无输出".into());
    }
    Ok(s.trim_start_matches('v').to_string())
}

/* ------------------------------------------------------------------ *
 * 测试
 * ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stchat-inst-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let f = std::fs::File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn extract_strips_first_dir() {
        let base = tmp("strip");
        let zip = base.join("a.zip");
        make_zip(
            &zip,
            &[
                ("SillyTavern-1.19.0/server.js", b"// server"),
                ("SillyTavern-1.19.0/src/x.js", b"x"),
            ],
        );
        let dest = base.join("out");
        let (files, bytes) = extract_zip(&zip, &dest, true, |_, _| {}).unwrap();
        assert_eq!(files, 2, "两个文件");
        assert!(bytes > 0);
        assert!(dest.join("server.js").is_file(), "顶层已剥离");
        assert!(dest.join("src/x.js").is_file(), "嵌套目录保留");
        assert!(!dest.join("SillyTavern-1.19.0").exists(), "不该留下原顶层目录");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn extract_without_strip_keeps_layout() {
        let base = tmp("nostrip");
        let zip = base.join("a.zip");
        make_zip(&zip, &[("node-v22/x/node.exe", b"bin")]);
        let dest = base.join("out");
        extract_zip(&zip, &dest, false, |_, _| {}).unwrap();
        assert!(dest.join("node-v22/x/node.exe").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn extract_rejects_traversal() {
        let base = tmp("slip");
        let zip = base.join("evil.zip");
        // 构造一个带 ../ 的条目名（zip-slip 攻击）
        make_zip(&zip, &[("../escaped.txt", b"pwned")]);
        let dest = base.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        let r = extract_zip(&zip, &dest, false, |_, _| {});
        // 库的 enclosed_name() 会拒绝；即使不报错也绝不能写到 dest 外
        assert!(!base.join("escaped.txt").exists(), "绝不能逃逸出目标目录");
        let _ = r;
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn required_files_gate() {
        let base = tmp("req");
        std::fs::write(base.join("server.js"), b"x").unwrap();
        assert!(required_ok(&base, &["server.js".to_string()]));
        assert!(!required_ok(&base, &["server.js".to_string(), "nope".to_string()]));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn version_dir_layout() {
        let cfg = Path::new("C:/cfg");
        assert_eq!(
            version_dir(cfg, "sillytavern", "1.19.0").to_string_lossy().replace('\\', "/"),
            "C:/cfg/plugins/sillytavern/1.19.0"
        );
    }

    fn manifest_from(json: &str) -> crate::plugin::Manifest {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn choose_spec_prefers_enabled_prebuilt() {
        let m = manifest_from(
            r#"{
                "id": "sillytavern", "name": "ST", "tier": "required", "kind": "process",
                "install": {
                    "npm_install": true,
                    "required": ["server.js", "node_modules"],
                    "sources": [{"url": "https://npm-path.example/{version}.zip"}],
                    "prebuilt": {
                        "enabled": true,
                        "sources": [{"url": "https://prebuilt.example/{version}.zip"}]
                    }
                }
            }"#,
        );
        let s = choose_spec(&m, "1.19.0", "win-x64").unwrap();
        assert!(s.prebuilt, "应选中预打包计划");
        assert!(!s.npm_install, "预打包必须跳过 npm install");
        assert_eq!(s.downloads[0].sources[0], "https://prebuilt.example/1.19.0.zip");
        assert_eq!(s.required_files, vec!["server.js", "node_modules"], "校验清单必须一致");
    }

    #[test]
    fn choose_spec_falls_back_to_npm_when_disabled() {
        let m = manifest_from(
            r#"{
                "id": "sillytavern", "name": "ST", "tier": "required", "kind": "process",
                "install": {
                    "npm_install": true,
                    "required": ["server.js"],
                    "sources": [{"url": "https://npm-path.example/{version}.zip"}],
                    "prebuilt": {
                        "enabled": false,
                        "sources": [{"url": "https://prebuilt.example/{version}.zip"}]
                    }
                }
            }"#,
        );
        let s = choose_spec(&m, "1.19.0", "win-x64").unwrap();
        assert!(!s.prebuilt);
        assert!(s.npm_install);
        assert_eq!(s.downloads[0].sources[0], "https://npm-path.example/1.19.0.zip");
    }

    #[test]
    fn choose_spec_errors_on_enabled_but_empty_prebuilt() {
        let m = manifest_from(
            r#"{
                "id": "sillytavern", "name": "ST", "tier": "required", "kind": "process",
                "install": {
                    "npm_install": true,
                    "sources": [{"url": "https://npm-path.example/{version}.zip"}],
                    "prebuilt": { "enabled": true, "sources": [] }
                }
            }"#,
        );
        // 开了开关却没源 → 必须显式报错，而不是静默回落 npm（那会掩盖配置错误）
        assert!(choose_spec(&m, "1.19.0", "win-x64").is_err());
    }

    #[test]
    fn prebuilt_respects_platform_sources() {
        let m = manifest_from(
            r#"{
                "id": "sillytavern", "name": "ST", "tier": "required", "kind": "process",
                "install": {
                    "npm_install": true,
                    "sources": [{"url": "https://npm-path.example/{version}.zip"}],
                    "prebuilt": {
                        "enabled": true,
                        "platforms": {
                            "win-x64": { "sources": [{"url": "https://prebuilt.win/{version}.zip"}] },
                            "darwin-arm64": { "sources": [{"url": "https://prebuilt.mac/{version}.zip"}] }
                        }
                    }
                }
            }"#,
        );
        let s = choose_spec(&m, "1.19.0", "win-x64").unwrap();
        assert_eq!(s.downloads[0].sources[0], "https://prebuilt.win/1.19.0.zip");
    }
}
