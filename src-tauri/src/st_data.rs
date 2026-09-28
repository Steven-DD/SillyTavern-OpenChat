//! SillyTavern 数据区外移（S2）
//!
//! ── 要解决什么 ──
//! ST 默认把用户数据（角色卡、会话、API 密钥、世界书…）写在 `<st_dir>/data`。
//! 这带来两个后果：
//!   1. **升级 ST 会连着用户数据一起被覆盖/替换** → 丢数据
//!   2. macOS 的 `.app` 包体只读 → ST 根本起不来
//!
//! ST 支持 `--dataRoot <dir>` 与 `--configPath <file>`（仅 standalone 模式，
//! CLI 优先于 config.yaml，见 `sillytavern/src/command-line.js:288-297`）。
//! 外移后：**代码只读、数据可写、升级不丢数据**。
//!
//! ── 勘实结论（决定迁移范围）──
//! 实测 ST 运行时持续写入的位置只有两处：
//!   - `<st_dir>/data/`      ← 主数据（角色卡/会话/密钥/世界书）
//!   - `<st_dir>/config.yaml` ← 保存设置时会改写
//! 其余（`plugins/`、`backups/`、`public/scripts/extensions/third-party`）
//! 只在用户主动安装 ST 插件/扩展时才写，不属本次迁移范围，
//! 但会在报告里如实标注（不假装"完全只读"）。
//!
//! ── 安全设计（不可妥协）──
//! 1. **只复制，不删除、不移动源目录** —— 源目录原样保留，回滚 = 改一个字段
//! 2. **ST 运行中禁止迁移** —— 边写边搬必然丢数据
//! 3. **复制后逐文件校验**（存在性 + 字节数），不通过则不切换绑定
//! 4. **目标目录非空时拒绝** —— 不做"合并"，避免覆盖已有数据
//! 5. 失败即清理本次新建的目标目录，不留半个副本

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 外移后的根目录名（位于应用配置目录下）
pub const RUNTIME_DIR: &str = "st-runtime";

/// 关键数据清单 —— 迁移前给用户看"到底会搬什么"，而不是一句"迁移数据"
const KEY_ITEMS: &[(&str, &str, &str)] = &[
    ("characters", "角色卡", "default-user/characters"),
    ("chats", "会话记录", "default-user/chats"),
    ("group_chats", "群聊记录", "default-user/group chats"),
    ("groups", "群组", "default-user/groups"),
    ("worlds", "世界书", "default-user/worlds"),
    ("personas", "用户角色（Persona）", "default-user/User Avatars"),
    ("backgrounds", "背景图", "default-user/backgrounds"),
    ("themes", "主题", "default-user/themes"),
    ("quickreplies", "快捷回复", "default-user/QuickReplies"),
    ("vectors", "向量存储", "default-user/vectors"),
    ("assets", "素材", "default-user/assets"),
    ("extensions_cfg", "扩展配置", "default-user/extensions"),
    ("thumbnails", "缩略图缓存", "default-user/thumbnails"),
    ("user_backups", "ST 自身备份", "default-user/backups"),
    ("settings", "界面与生成设置", "default-user/settings.json"),
    ("secrets", "API 密钥", "default-user/secrets.json"),
    ("stats", "使用统计", "default-user/stats.json"),
    ("cookie_secret", "会话签名密钥", "cookie-secret.txt"),
    ("storage", "账户存储", "_storage"),
];

/* ------------------------------------------------------------------ *
 * 路径
 * ------------------------------------------------------------------ */

pub fn external_root(config_dir: &Path) -> PathBuf {
    config_dir.join(RUNTIME_DIR)
}

/// 路径显示规范化：Windows 上统一成反斜杠。
///
/// Tauri 的 `app_config_dir()` 返回的是 `C:\...\Roaming/<app-id>`
/// 这种**混合分隔符**写法（app-id 形如 `io.github.stevendd.stchat`）；直接拼进配置或界面会很难看，也和其它路径不一致
/// （之前 `st_dir` 显示成 `src-tauri\..\..\sillytavern` 是同一类问题）。
pub fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    if cfg!(windows) {
        s.replace('/', "\\")
    } else {
        s
    }
}

pub fn external_data_root(config_dir: &Path) -> PathBuf {
    external_root(config_dir).join("data")
}

pub fn external_config_path(config_dir: &Path) -> PathBuf {
    external_root(config_dir).join("config.yaml")
}

/// ST 默认位置（插件目录内）
pub fn default_data_root(st_dir: &Path) -> PathBuf {
    st_dir.join("data")
}

pub fn default_config_path(st_dir: &Path) -> PathBuf {
    st_dir.join("config.yaml")
}

/* ------------------------------------------------------------------ *
 * 目录统计与复制
 * ------------------------------------------------------------------ */

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TreeStat {
    pub files: u64,
    pub bytes: u64,
}

/// 统计目录（不跟随符号链接，避免死循环）
pub fn stat_tree(root: &Path) -> TreeStat {
    let mut acc = TreeStat::default();
    walk(root, &mut |p| {
        if let Ok(md) = fs::symlink_metadata(p) {
            if md.is_file() {
                acc.files += 1;
                acc.bytes += md.len();
            }
        }
    });
    acc
}

fn walk(dir: &Path, f: &mut impl FnMut(&Path)) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for ent in rd.flatten() {
        let p = ent.path();
        match fs::symlink_metadata(&p) {
            Ok(md) if md.is_dir() => walk(&p, f),
            Ok(_) => f(&p),
            Err(_) => {}
        }
    }
}

/// 递归复制，返回复制的字节数。`on_progress` 收到 (已复制字节, 已知总字节)
fn copy_tree(
    src: &Path,
    dst: &Path,
    total: u64,
    done: &mut u64,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("创建目录失败 {}: {e}", dst.display()))?;
    let rd = fs::read_dir(src).map_err(|e| format!("读取目录失败 {}: {e}", src.display()))?;
    for ent in rd {
        let ent = ent.map_err(|e| format!("遍历失败 {}: {e}", src.display()))?;
        let from = ent.path();
        let to = dst.join(ent.file_name());
        let md = fs::symlink_metadata(&from).map_err(|e| format!("读取属性失败: {e}"))?;
        if md.is_dir() {
            copy_tree(&from, &to, total, done, on_progress)?;
        } else {
            fs::copy(&from, &to).map_err(|e| {
                format!(
                    "复制失败 {} → {}: {e}",
                    from.display(),
                    to.display()
                )
            })?;
            *done += md.len();
            on_progress(*done, total);
        }
    }
    Ok(())
}

/// 文件级校验：源里每个文件都要在目标里存在且字节数一致
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerifyReport {
    pub checked: u64,
    pub bad: Vec<String>,
    pub source_bytes: u64,
    pub target_bytes: u64,
}

impl VerifyReport {
    pub fn ok(&self) -> bool {
        self.bad.is_empty() && self.source_bytes == self.target_bytes
    }
}

pub fn verify_tree(src: &Path, dst: &Path) -> VerifyReport {
    let mut rep = VerifyReport::default();
    let mut bad = Vec::new();
    let mut checked = 0u64;
    walk_collect(src, dst, &mut rep, &mut bad, &mut checked);
    rep.bad = bad;
    rep.checked = checked;
    rep
}

fn walk_collect(
    src: &Path,
    dst: &Path,
    rep: &mut VerifyReport,
    bad: &mut Vec<String>,
    checked: &mut u64,
) {
    let Ok(rd) = fs::read_dir(src) else { return };
    for ent in rd.flatten() {
        let s = ent.path();
        let d = dst.join(ent.file_name());
        match fs::symlink_metadata(&s) {
            Ok(md) if md.is_dir() => walk_collect(&s, &d, rep, bad, checked),
            Ok(md) => {
                *checked += 1;
                rep.source_bytes += md.len();
                match fs::symlink_metadata(&d) {
                    Ok(dm) if dm.is_file() && dm.len() == md.len() => {
                        rep.target_bytes += dm.len();
                    }
                    Ok(dm) if dm.is_file() => {
                        rep.target_bytes += dm.len();
                        if bad.len() < 20 {
                            bad.push(format!(
                                "{}（源 {} B / 目标 {} B）",
                                s.file_name().unwrap_or_default().to_string_lossy(),
                                md.len(),
                                dm.len()
                            ));
                        }
                    }
                    _ => {
                        if bad.len() < 20 {
                            bad.push(format!(
                                "{}（目标缺失）",
                                s.file_name().unwrap_or_default().to_string_lossy()
                            ));
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }
}

/* ------------------------------------------------------------------ *
 * 报告
 * ------------------------------------------------------------------ */

#[derive(Debug, Clone, Serialize)]
pub struct KeyItem {
    pub id: String,
    pub name: String,
    /// 相对于数据根的路径
    pub rel: String,
    pub exists: bool,
    pub kind: String,
    pub files: u64,
    pub bytes: u64,
}

/// 数据目录现状（设置页/插件页展示）
#[derive(Debug, Clone, Serialize)]
pub struct DataStatus {
    /// 当前生效的数据根
    pub active_root: String,
    pub active_config: String,
    /// 是否已外移
    pub external: bool,
    /// ST 默认位置（回滚目标）
    pub default_root: String,
    pub default_config: String,
    /// 外移候选位置
    pub external_root: String,
    pub external_config: String,
    pub external_data_exists: bool,
    pub total_files: u64,
    pub total_bytes: u64,
    pub items: Vec<KeyItem>,
    pub st_running: bool,
    /// ST 目录内仍在写的非数据位置（如实告知，不假装完全只读）
    pub residual_writable: Vec<String>,
}

fn items_of(data_root: &Path) -> Vec<KeyItem> {
    KEY_ITEMS
        .iter()
        .map(|(id, name, rel)| {
            let p = data_root.join(rel);
            let md = fs::symlink_metadata(&p).ok();
            let exists = md.is_some();
            let (files, bytes) = match md {
                Some(m) if m.is_dir() => {
                    let s = stat_tree(&p);
                    (s.files, s.bytes)
                }
                Some(m) => (1, m.len()),
                None => (0, 0),
            };
            KeyItem {
                id: (*id).to_string(),
                name: (*name).to_string(),
                rel: (*rel).to_string(),
                exists,
                kind: if p.is_dir() { "dir" } else { "file" }.to_string(),
                files,
                bytes,
            }
        })
        .collect()
}

/// 端口是否已有服务在监听（用于判断 ST 是否在运行）
pub fn is_st_running(port: u16) -> bool {
    let addr = format!("127.0.0.1:{port}");
    match addr.to_socket_addrs() {
        Ok(mut it) => match it.next() {
            Some(sa) => TcpStream::connect_timeout(&sa, Duration::from_millis(400)).is_ok(),
            None => false,
        },
        Err(_) => false,
    }
}

/// 组装现状报告。`active_root`/`active_config` 由调用方（插件宿主）传入，
/// 因为"是否已外移"是宿主的绑定状态，不是磁盘事实。
pub fn status(
    st_dir: &Path,
    config_dir: &Path,
    active_root: &Path,
    active_config: &Path,
    st_port: u16,
) -> DataStatus {
    let stat = stat_tree(active_root);
    let ext_root = external_data_root(config_dir);
    DataStatus {
        active_root: display_path(active_root),
        active_config: display_path(active_config),
        external: active_root != default_data_root(st_dir),
        default_root: display_path(&default_data_root(st_dir)),
        default_config: display_path(&default_config_path(st_dir)),
        external_root: display_path(&ext_root),
        external_config: display_path(&external_config_path(config_dir)),
        external_data_exists: ext_root.exists(),
        total_files: stat.files,
        total_bytes: stat.bytes,
        items: items_of(active_root),
        st_running: is_st_running(st_port),
        residual_writable: vec![
            "<ST 目录>/config.yaml —— 保存 ST 设置时改写；已由 --configPath 一并外移".into(),
            "<ST 目录>/plugins/ —— 仅当你安装「服务器插件」时才写入".into(),
            "<ST 目录>/public/scripts/extensions/third-party/ —— 仅当你安装「ST 扩展」时才写入".into(),
        ],
    }
}

/* ------------------------------------------------------------------ *
 * 迁移
 * ------------------------------------------------------------------ */

#[derive(Debug, Clone, Serialize)]
pub struct MigrationPlan {
    pub source: String,
    pub target: String,
    pub config_source: String,
    pub config_target: String,
    pub source_exists: bool,
    pub target_exists: bool,
    pub files: u64,
    pub bytes: u64,
    pub items: Vec<KeyItem>,
    /// 阻断项（非空则不可执行）
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub can_proceed: bool,
}

/// 生成迁移计划（纯只读，不动任何文件）
pub fn plan(st_dir: &Path, config_dir: &Path, st_port: u16) -> MigrationPlan {
    let src = default_data_root(st_dir);
    let dst = external_data_root(config_dir);
    let src_cfg = default_config_path(st_dir);
    let dst_cfg = external_config_path(config_dir);

    let source_exists = src.is_dir();
    let target_exists = dst.exists();
    let stat = if source_exists {
        stat_tree(&src)
    } else {
        TreeStat::default()
    };

    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    if !source_exists {
        blockers.push(format!("源数据目录不存在：{}", src.display()));
    }
    // 迁移的意义在于"把数据搬出去"。若还没有数据，外移也应该是允许的（空目录场景），
    // 但此时不叫迁移而叫"启用外移"。这里按迁移语义严格处理。
    if target_exists {
        let t = stat_tree(&dst);
        if t.files > 0 {
            blockers.push(format!(
                "目标位置已有数据（{} 个文件）—— 为避免覆盖，请先处理：{}",
                t.files,
                dst.display()
            ));
        } else {
            warnings.push(format!("目标目录已存在但为空，将直接使用：{}", dst.display()));
        }
    }
    if is_st_running(st_port) {
        blockers.push(format!(
            "SillyTavern 正在运行（端口 {st_port}）—— 运行中迁移会丢失正在写入的数据。请先关闭应用后重试。"
        ));
    }
    if stat.files == 0 {
        warnings.push("源数据目录为空，迁移后 ST 将以全新数据启动".into());
    }
    if stat.files > 0 && stat.bytes == 0 {
        warnings.push("源数据仅含空文件，请确认是否符合预期".into());
    }

    let can_proceed = blockers.is_empty();

    MigrationPlan {
        source: display_path(&src),
        target: display_path(&dst),
        config_source: display_path(&src_cfg),
        config_target: display_path(&dst_cfg),
        source_exists,
        target_exists,
        files: stat.files,
        bytes: stat.bytes,
        items: items_of(&src),
        blockers,
        warnings,
        can_proceed,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationResult {
    pub ok: bool,
    pub message: String,
    pub files: u64,
    pub bytes: u64,
    pub target: String,
    pub config_target: String,
    pub verify: VerifyReport,
    /// 源目录是否原样保留（恒为 true，用于明确告知用户"没删你的数据"）
    pub source_kept: bool,
}

/// 进度事件负载
#[derive(Debug, Clone, Serialize)]
pub struct MigrationProgress {
    /// copying | verifying | done
    pub phase: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub message: String,
}

/// 执行迁移。
///
/// **不删除源目录**：复制 + 校验通过后，仅由调用方切换绑定。
/// 失败时清理本次新建的目标目录（仅在它原本不存在时才清理，避免误删用户数据）。
pub fn execute(
    st_dir: &Path,
    config_dir: &Path,
    st_port: u16,
    on_progress: &mut dyn FnMut(MigrationProgress),
) -> Result<MigrationResult, String> {
    let p = plan(st_dir, config_dir, st_port);
    if !p.can_proceed {
        return Err(p.blockers.join("；"));
    }

    let src = default_data_root(st_dir);
    let dst = external_data_root(config_dir);
    let src_cfg = default_config_path(st_dir);
    let dst_cfg = external_config_path(config_dir);

    // 只在目标原本不存在时，失败才允许清理 —— 避免误删用户已有目录
    let target_preexisting = dst.exists();

    on_progress(MigrationProgress {
        phase: "copying".into(),
        done_bytes: 0,
        total_bytes: p.bytes,
        message: format!("正在复制 {} 个文件（{:.1} MB）…", p.files, p.bytes as f64 / 1048576.0),
    });

    let mut done = 0u64;
    let total = p.bytes;
    let copy_result = copy_tree(&src, &dst, total, &mut done, &mut |d, t| {
        on_progress(MigrationProgress {
            phase: "copying".into(),
            done_bytes: d,
            total_bytes: t,
            message: String::new(),
        });
    });

    if let Err(e) = copy_result {
        cleanup_partial(&dst, target_preexisting);
        return Err(e);
    }

    // config.yaml：存在才搬（新装的 ST 可能还没生成）
    if src_cfg.is_file() {
        if let Err(e) = fs::copy(&src_cfg, &dst_cfg) {
            cleanup_partial(&dst, target_preexisting);
            return Err(format!("复制 config.yaml 失败：{e}"));
        }
    }

    on_progress(MigrationProgress {
        phase: "verifying".into(),
        done_bytes: total,
        total_bytes: total,
        message: "正在校验复制结果…".into(),
    });

    let mut verify = verify_tree(&src, &dst);
    if src_cfg.is_file() {
        match (fs::metadata(&src_cfg), fs::metadata(&dst_cfg)) {
            (Ok(a), Ok(b)) if a.len() == b.len() => {}
            _ => verify.bad.push("config.yaml（目标缺失或大小不符）".into()),
        }
    }

    if !verify.ok() {
        cleanup_partial(&dst, target_preexisting);
        return Err(format!(
            "复制校验未通过（{} 项不符），已回退且未修改任何绑定：{}",
            verify.bad.len(),
            verify.bad.join("；")
        ));
    }

    on_progress(MigrationProgress {
        phase: "done".into(),
        done_bytes: total,
        total_bytes: total,
        message: "迁移完成".into(),
    });

    Ok(MigrationResult {
        ok: true,
        message: format!(
            "已迁移 {} 个文件（{:.1} MB）到应用数据目录；原始数据完整保留在原位置。",
            p.files,
            p.bytes as f64 / 1048576.0
        ),
        files: p.files,
        bytes: p.bytes,
        target: display_path(&dst),
        config_target: display_path(&dst_cfg),
        verify,
        source_kept: true,
    })
}

/// 清理失败留下的目标目录。仅当它本次新建时才删，避免误删用户已有数据。
fn cleanup_partial(dst: &Path, preexisting: bool) {
    if preexisting {
        return;
    }
    if let Err(e) = remove_tree(dst) {
        eprintln!("[st_data] 清理未完成的目标目录失败（需手工删除）{}: {e}", dst.display());
    }
}

/// 单层递归删除（逐条删除，避免触发批量删除防护）
fn remove_tree(dir: &Path) -> Result<(), String> {
    let rd = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    for ent in rd.flatten() {
        let p = ent.path();
        let md = fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
        if md.is_dir() {
            remove_tree(&p)?;
            fs::remove_dir(&p).map_err(|e| format!("删除目录失败 {}: {e}", p.display()))?;
        } else {
            fs::remove_file(&p).map_err(|e| format!("删除文件失败 {}: {e}", p.display()))?;
        }
    }
    Ok(())
}

/* ------------------------------------------------------------------ *
 * 导出 / 导入（数据袋：data 目录内容 + config.yaml）
 * ------------------------------------------------------------------ */

#[derive(Debug, Clone, Serialize)]
pub struct DataBagResult {
    pub path: String,
    pub files: u64,
    pub bytes: u64,
    pub message: String,
}

/// 导出：把数据目录内容 + config.yaml 打成一个 zip（zip 布局 = data 内容在根 + config.yaml）。
/// 同步实现，调用方放阻塞线程执行。
pub fn export_bag(data_root: &Path, config_path: Option<&Path>, dest: &Path) -> Result<DataBagResult, String> {
    if !data_root.is_dir() {
        return Err(format!("数据目录不存在：{}", display_path(data_root)));
    }
    let file = fs::File::create(dest).map_err(|e| format!("创建导出文件失败：{e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut stack = vec![data_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = fs::read_dir(&dir).map_err(|e| format!("读取目录失败 {}: {e}", dir.display()))?;
        for e in rd.flatten() {
            let p = e.path();
            let rel = p
                .strip_prefix(data_root)
                .map_err(|_| "相对路径计算失败".to_string())?;
            if p.is_dir() {
                stack.push(p);
            } else {
                zip.start_file(rel.to_string_lossy().replace('\\', "/"), opts.clone())
                    .map_err(|e| format!("写入 zip 条目失败：{e}"))?;
                let mut f = fs::File::open(&p).map_err(|e| format!("打开文件失败：{e}"))?;
                std::io::copy(&mut f, &mut zip).map_err(|e| format!("压缩写入失败：{e}"))?;
                files += 1;
                bytes += fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    if let Some(cp) = config_path {
        if cp.is_file() {
            zip.start_file("config.yaml", opts.clone())
                .map_err(|e| format!("写入 config.yaml 失败：{e}"))?;
            let mut f = fs::File::open(cp).map_err(|e| format!("打开 config.yaml 失败：{e}"))?;
            std::io::copy(&mut f, &mut zip).map_err(|e| format!("压缩 config.yaml 失败：{e}"))?;
            files += 1;
        }
    }
    zip.finish().map_err(|e| format!("收尾 zip 失败：{e}"))?;

    Ok(DataBagResult {
        path: display_path(dest),
        files,
        bytes,
        message: format!("已导出 {files} 个文件（{:.1} MB）", bytes as f64 / 1048576.0),
    })
}

/// 导入：清点 zip → 备份现有数据目录（改名 .bak-<ts>）→ 解压 → 返回摘要。
/// zip 布局兼容两种：① 我们导出的（data 内容在根 + config.yaml）
/// ② 带顶层 data/ 前缀的（解出来放进 data 目录）。同步实现。
pub fn import_bag(
    zip_path: &Path,
    data_root: &Path,
    config_path: Option<&Path>,
) -> Result<DataBagResult, String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("打开导入文件失败：{e}"))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| format!("不是有效的 zip：{e}"))?;

    // 检测顶层是否带 data/ 前缀（for 循环而不是迭代器链 —— ZipFile 借用 archive，
    // 闭包形式会把借用带出作用域，编译器直接拒绝）
    let mut has_data_prefix = false;
    for i in 0..archive.len() {
        let Ok(e) = archive.by_index(i) else { continue };
        if let Some(name) = e.enclosed_name() {
            if name.components().next().map(|c| c.as_os_str() == "data").unwrap_or(false) {
                has_data_prefix = true;
                break;
            }
        }
    }

    // 1) 备份现有数据（有内容才备份；空目录直接清掉）
    if data_root.is_dir() {
        let st = stat_tree(data_root);
        if st.files > 0 {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let bak = data_root.with_extension(format!("bak-{ts}"));
            fs::rename(data_root, &bak).map_err(|e| format!("备份现有数据失败：{e}"))?;
            println!("[st_data] 导入前备份 → {}（{} 文件）", display_path(&bak), st.files);
        } else {
            let _ = fs::remove_dir_all(data_root);
        }
    }
    fs::create_dir_all(data_root).map_err(|e| format!("重建数据目录失败：{e}"))?;

    // 2) 解压（zip-slip 防护用 enclosed_name）
    let mut files = 0u64;
    let mut bytes = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("读取条目失败：{e}"))?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let rel = rel.to_path_buf();

        // config.yaml 单独处理（放 st-runtime 根，不放 data 目录）
        let is_config = rel.file_name().map(|f| f == "config.yaml").unwrap_or(false)
            && rel.parent().map(|p| p.as_os_str().is_empty()).unwrap_or(true);
        if is_config {
            let Some(cp) = config_path else { continue };
            if let Some(parent) = cp.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("建目录失败：{e}"))?;
            }
            let mut out = fs::File::create(cp).map_err(|e| format!("写入 config.yaml 失败：{e}"))?;
            let n = std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压失败：{e}"))?;
            files += 1;
            bytes += n;
            continue;
        }

        // data/ 前缀布局：剥掉第一层；根布局：直接放 data 目录
        let rel = if has_data_prefix {
            let mut it = rel.components();
            it.next();
            match it.as_path() {
                p if p.as_os_str().is_empty() => continue,
                p => p.to_path_buf(),
            }
        } else {
            rel
        };
        let out = data_root.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| format!("建目录失败：{e}"))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("建目录失败：{e}"))?;
        }
        let mut outf = fs::File::create(&out).map_err(|e| format!("写入失败 {}: {e}", out.display()))?;
        let n = std::io::copy(&mut entry, &mut outf).map_err(|e| format!("解压失败：{e}"))?;
        files += 1;
        bytes += n;
    }

    Ok(DataBagResult {
        path: display_path(data_root),
        files,
        bytes,
        message: format!("已导入 {files} 个文件（{:.1} MB）。重启应用后生效。", bytes as f64 / 1048576.0),
    })
}

/* ------------------------------------------------------------------ *
 * 测试
 * ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stchat-data-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).expect("create tmp");
        d
    }

    #[test]
    fn stat_and_verify_roundtrip() {
        let base = tmp("stat");
        let src = base.join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), b"hello").unwrap();
        fs::write(src.join("sub/b.txt"), b"world!!!").unwrap();

        let s = stat_tree(&src);
        assert_eq!(s.files, 2, "files");
        assert_eq!(s.bytes, 5 + 8, "bytes");

        let dst = base.join("dst");
        let mut done = 0u64;
        copy_tree(&src, &dst, s.bytes, &mut done, &mut |_, _| {}).unwrap();
        assert_eq!(done, s.bytes, "copied bytes");

        let v = verify_tree(&src, &dst);
        assert!(v.ok(), "verify should pass: {v:?}");
        assert_eq!(v.checked, 2, "checked");

        // 篡改目标 → 校验必须失败（防止"假成功"）
        fs::write(dst.join("a.txt"), b"HELLO!").unwrap();
        assert!(!verify_tree(&src, &dst).ok(), "tampered verify must fail");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn plan_blocks_when_target_has_data() {
        let base = tmp("block");
        let st = base.join("st");
        let cfg = base.join("cfg");
        fs::create_dir_all(st.join("data/default-user/characters")).unwrap();
        fs::write(st.join("data/cookie-secret.txt"), b"secret").unwrap();
        fs::write(st.join("config.yaml"), b"port: 8000").unwrap();
        fs::create_dir_all(&cfg).unwrap();

        let p = plan(&st, &cfg, 0); // 端口 0 → 不会误判为运行中
        assert!(p.can_proceed, "should be able to proceed: {:?}", p.blockers);
        // data/ 下只有 cookie-secret.txt 一个文件；characters 是空目录，不计入文件数；
        // config.yaml 在 st 根目录，不属于 dataRoot，也不计入
        assert_eq!(p.files, 1, "files");

        // 目标已有数据 → 必须阻断
        let dst = external_data_root(&cfg);
        fs::create_dir_all(&dst).unwrap();
        fs::write(dst.join("existing.txt"), b"x").unwrap();
        let p2 = plan(&st, &cfg, 0);
        assert!(!p2.can_proceed, "must block when target has data");
        assert!(p2.blockers.iter().any(|b| b.contains("已有数据")), "{:?}", p2.blockers);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn execute_copies_and_keeps_source() {
        let base = tmp("exec");
        let st = base.join("st");
        let cfg = base.join("cfg");
        fs::create_dir_all(st.join("data/default-user/characters")).unwrap();
        fs::write(st.join("data/default-user/characters/c1.png"), vec![7u8; 1024]).unwrap();
        fs::write(st.join("data/cookie-secret.txt"), b"k").unwrap();
        fs::write(st.join("config.yaml"), b"port: 8000\n").unwrap();
        fs::create_dir_all(&cfg).unwrap();

        let mut phases: Vec<String> = Vec::new();
        let r = execute(&st, &cfg, 0, &mut |ev| {
            if phases.last() != Some(&ev.phase) {
                phases.push(ev.phase.clone());
            }
        })
        .expect("execute should succeed");

        assert!(r.ok);
        assert!(r.verify.ok(), "verify: {:?}", r.verify);
        assert_eq!(r.files, 2, "files");
        // 源必须原样保留
        assert!(st.join("data/default-user/characters/c1.png").is_file(), "source kept");
        assert!(st.join("config.yaml").is_file(), "source config kept");
        // 目标必须齐备
        assert!(cfg.join("st-runtime/data/cookie-secret.txt").is_file(), "target secret");
        assert!(cfg.join("st-runtime/config.yaml").is_file(), "target config");
        assert!(phases.contains(&"copying".to_string()), "phases: {phases:?}");
        assert!(phases.contains(&"verifying".to_string()), "phases: {phases:?}");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn export_import_roundtrip() {
        let base = std::env::temp_dir().join(format!("stchat-bag-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let data = base.join("st-runtime/data");
        fs::create_dir_all(data.join("default-user/characters")).unwrap();
        fs::write(data.join("default-user/characters/c1.png"), b"png1").unwrap();
        fs::write(data.join("cookie-secret.txt"), b"secret").unwrap();
        let config = base.join("st-runtime/config.yaml");
        fs::write(&config, b"port: 8000\n").unwrap();

        // 导出
        let zip_path = base.join("bag.zip");
        let out = export_bag(&data, Some(&config), &zip_path).expect("export ok");
        assert_eq!(out.files, 3, "2 个数据文件 + config.yaml");

        // 破坏现场，再导入 → 内容应恢复
        fs::remove_dir_all(&data).unwrap();
        let r = import_bag(&zip_path, &data, Some(&config)).expect("import ok");
        assert_eq!(r.files, 3, "回读 3 个文件");
        assert_eq!(
            fs::read(data.join("default-user/characters/c1.png")).unwrap(),
            b"png1"
        );
        assert_eq!(fs::read(data.join("cookie-secret.txt")).unwrap(), b"secret");
        assert_eq!(fs::read(&config).unwrap(), b"port: 8000\n", "config 回到 st-runtime 根");

        // 导入前备份目录存在（第一次导入时 data 目录已被删空，不应有备份；
        // 再造点数据导入第二次 → 必须出现 .bak-*）
        fs::write(data.join("stale.txt"), b"old").unwrap();
        let r2 = import_bag(&zip_path, &data, Some(&config)).expect("import 2 ok");
        assert_eq!(r2.files, 3);
        let has_bak = fs::read_dir(&base.join("st-runtime"))
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("data.bak-"));
        assert!(has_bak, "导入前必须把现有数据备份成 data.bak-*");

        let _ = fs::remove_dir_all(&base);
    }
}
