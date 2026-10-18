//! backend/extensions —— 扩展发现/QR + git 安装族（Phase 5 · T14）
//!
//! ST `src/endpoints/extensions.js`（1.19.0）逐行核对：
//! - `/install`（:92-156）/update（:169-215）/branches（:217-264）
//!   /switch（:266-327）/move（:329-375)/version（:387-432）/delete（:442-474）
//!   /discover（:480-534）
//!
//! T14 安全约束（评审 P1-1，均为**收紧**）：
//! - **仅 https://**（ST 允许 http —— native 收紧）；http/file/ssh/git 前缀与
//!   `ext::` 一律 400 纯文本错误，服务端校验不依赖前端
//! - **git2/libgit2 本地库**：不起 system git（无命令执行向量）；S19 整改：
//!   install/update 前用 `git2::Config::open_default` 探测全局 gitconfig，
//!   存在任意 `url.*.insteadOf` 键即拒 400 —— libgit2 对 insteadOf 的 URL
//!   重写支持不完整且版本相关，不能依赖其缺席，须显式设防（纵深防御）
//! - sanitize + `pathguard::join_under` 拼接兜底（穿越禁杀）
//! - manifest 校验失败 → **递归清理**再报错（extensions.js:148-152 语义）
//! - clone/fetch 失败亦清理残留（评审"fail→cleanup"）
//! - global 目录 = `<data_root>/global-extensions`；单用户模型下 ST 的 admin
//!   校验恒真（登记清单，global 面开放）
//!
//! 错误语义（ST 字面）：**纯文本**响应体（App postExt 直接把 text 当错误消息）
//! + 204 无体。网络路径（clone/fetch）依赖上游可用性 → CI 测试为离线面
//!（本地 init 仓库 + 手工目录），真网路径不进回归。

use super::{apply_cors_resp, BackendState};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use git2::build::{CheckoutBuilder, RepoBuilder};
use git2::{FetchOptions, Repository};
use serde_json::{json, Value};

fn text_resp(status: StatusCode, msg: impl Into<String>, origin: Option<HeaderValue>) -> Response {
    apply_cors_resp((status, msg.into()).into_response(), origin)
}

fn ok_json(origin: Option<HeaderValue>, v: Value) -> Response {
    apply_cors_resp(Json(v).into_response(), origin)
}

fn no_content(origin: Option<HeaderValue>) -> Response {
    apply_cors_resp(StatusCode::NO_CONTENT.into_response(), origin)
}

/* ============================================================ *
 * 路径/名规（服务端校验）
 * ============================================================ */

pub(crate) fn user_ext_dir(st: &BackendState) -> std::path::PathBuf {
    st.data_root.join("default-user").join("extensions")
}

fn global_ext_dir(st: &BackendState) -> std::path::PathBuf {
    // ST globalExtensions = public/scripts/extensions/third-party（多用户共享）；
    // App 单用户 → data_root/global-extensions（语义保留）
    st.data_root.join("global-extensions")
}

fn ext_dir_for(st: &BackendState, global: bool) -> std::path::PathBuf {
    if global {
        global_ext_dir(st)
    } else {
        user_ext_dir(st)
    }
}

/// sanitize-filename 等价 + 非空挡板
fn sanitize_name(raw: &str) -> Result<String, String> {
    let s = crate::pathguard::component(raw).map_err(|_| "非法扩展名".to_string())?;
    if s.is_empty() {
        return Err("非法扩展名".into());
    }
    Ok(s)
}

/// 解析扩展目录（404 文本 ST 字面："Directory does not exist at <path>"）
fn resolve_ext_dir(
    st: &BackendState,
    name: &str,
    global: bool,
    origin: Option<HeaderValue>,
) -> Result<std::path::PathBuf, Response> {
    let safe = match sanitize_name(name) {
        Ok(s) => s,
        Err(_) => {
            return Err(text_resp(
                StatusCode::BAD_REQUEST,
                "Bad Request: A valid extensionName is required in the request body.",
                origin,
            ))
        }
    };
    let base = ext_dir_for(st, global);
    let join = crate::pathguard::join_under(&base, &[&safe])
        .map_err(|_| text_resp(StatusCode::FORBIDDEN, "扩展名不得越出基础目录", origin.clone()))?;
    if !join.is_dir() {
        return Err(text_resp(
            StatusCode::NOT_FOUND,
            format!("Directory does not exist at {}", join.display()),
            origin,
        ));
    }
    Ok(join)
}

/// T14 URL 校验：仅 https；禁 ext::/file/ssh/git；取 basename（去 .git）
fn validate_extension_url(raw: &str) -> Result<(String, String), String> {
    let raw = raw.trim();
    if !raw.starts_with("https://") {
        return Err("Bad Request: Only HTTPS protocols are supported for the Extension URL.".into());
    }
    let lower = raw.to_ascii_lowercase();
    if lower.contains("ext::") || lower.contains("file:") || lower.contains("ssh:") {
        return Err("Bad Request: URL scheme is not allowed.".into());
    }
    let path_part = raw
        .trim_start_matches("https://")
        .split_once('/')
        .map(|(_, r)| r)
        .unwrap_or("");
    let last = path_part
        .split('/')
        .filter(|s| !s.is_empty())
        .last()
        .unwrap_or("");
    let stem = last.strip_suffix(".git").unwrap_or(last);
    let name = sanitize_name(stem).map_err(|_| {
        "Could not determine the extension name from the URL. Please provide a valid git repository URL."
            .to_string()
    })?;
    Ok((raw.to_string(), name))
}

/* ============================================================ *
 * S19：全局 gitconfig 的 url.*.insteadOf 探测（install/update 前置闸门）
 * ============================================================ */

/// 谓词：配置里存在任意 `url.<节>.insteadof` 键（git 键名大小写不敏感，故统一小写比对）
fn config_has_insteadof(cfg: &git2::Config) -> bool {
    // git2 0.19：ConfigEntries 未实现 Iterator trait，但有固有 next() 方法
    let Ok(mut entries) = cfg.entries(None) else {
        return false;
    };
    while let Some(entry) = entries.next() {
        let Ok(entry) = entry else { continue };
        if let Some(name) = entry.name() {
            let lower = name.to_ascii_lowercase();
            if lower.starts_with("url.") && lower.ends_with(".insteadof") {
                return true;
            }
        }
    }
    false
}

/// 探测全局 gitconfig（system + XDG + user 全链，libgit2 open_default 语义）。
/// 读不到全局配置 = 无 insteadOf 面 → false（放行，不因配置缺失拒绝正常安装）。
fn global_insteadof_present() -> bool {
    match git2::Config::open_default() {
        Ok(cfg) => config_has_insteadof(&cfg),
        Err(_) => false,
    }
}

/// S19 闸门的统一 400 文本响应
fn insteadof_reject(origin: Option<HeaderValue>) -> Response {
    text_resp(
        StatusCode::BAD_REQUEST,
        "Bad Request: Global git configuration contains url.*.insteadOf rewrite rules; \
         extension install/update is disabled for safety.",
        origin,
    )
}

/* ============================================================ *
 * /install
 * ============================================================ */
pub async fn install(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(url_raw) = body.get("url").and_then(|u| u.as_str()) else {
        return text_resp(
            StatusCode::BAD_REQUEST,
            "Bad Request: A valid URL is required in the request body.",
            origin,
        );
    };
    // S19：任何落盘/网络动作之前拒绝（prevent 任何 clone 发生在重写劫持环境）
    if global_insteadof_present() {
        return insteadof_reject(origin);
    }
    let global = body.get("global").and_then(|g| g.as_bool()).unwrap_or(false);
    let branch = body
        .get("branch")
        .and_then(|b| b.as_str())
        .map(String::from);

    let (url, name) = match validate_extension_url(url_raw) {
        Ok(pair) => pair,
        Err(e) => return text_resp(StatusCode::BAD_REQUEST, e, origin),
    };

    let base = ext_dir_for(st, global);
    let _ = std::fs::create_dir_all(&base);
    let ext_path = match crate::pathguard::join_under(&base, &[&name]) {
        Ok(p) => p,
        Err(_) => return text_resp(StatusCode::FORBIDDEN, "扩展名不得越出基础目录", origin),
    };
    if ext_path.exists() {
        return text_resp(
            StatusCode::CONFLICT,
            format!("Directory already exists at {}", ext_path.display()),
            origin,
        );
    }

    // clone（depth 1 + branch 可选）
    let mut fo = FetchOptions::new();
    fo.depth(1);
    let mut rb = RepoBuilder::new();
    rb.fetch_options(fo);
    if let Some(b) = &branch {
        rb.branch(b);
    }
    if rb.clone(&url, &ext_path).is_err() {
        // fail→cleanup（不残留半写目录）
        let _ = std::fs::remove_dir_all(&ext_path);
        return text_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error. Check the server logs for more details.",
            origin,
        );
    }

    // manifest 校验（对象形态；失败 → 清理 + 500 文本）
    let manifest = std::fs::read_to_string(ext_path.join("manifest.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    if !manifest.as_ref().map(|m| m.is_object()).unwrap_or(false) {
        let _ = std::fs::remove_dir_all(&ext_path);
        return text_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error. Check the server logs for more details.",
            origin,
        );
    }
    let m = manifest.unwrap();
    let folder_name = ext_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    ok_json(
        origin,
        json!({
            "version": m.get("version").cloned().unwrap_or(Value::Null),
            "author": m.get("author").cloned().unwrap_or(Value::Null),
            "display_name": m.get("display_name").cloned().unwrap_or(Value::Null),
            "extensionPath": ext_path.to_string_lossy(),
            "folderName": folder_name,
        }),
    )
}

/* ---------------- git 小件 ---------------- */

fn open_repo(path: &std::path::Path, origin: Option<HeaderValue>) -> Result<Repository, Response> {
    Repository::open(path).map_err(|_| {
        text_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error. Check the server logs for more details.",
            origin,
        )
    })
}

/// fetch origin 全 refs（无 remote → Err）
fn fetch_origin(repo: &Repository) -> Result<git2::Oid, String> {
    let mut remote = repo
        .find_remote("origin")
        .map_err(|_| "no origin remote".to_string())?;
    let mut fo = FetchOptions::new();
    remote
        .fetch(&["refs/heads/*:refs/remotes/origin/*"], Some(&mut fo), None)
        .map_err(|e| e.to_string())?;
    // 远端默认分支：origin/HEAD → 指向分支
    if let Ok(head_ref) = repo.find_reference("refs/remotes/origin/HEAD") {
        if let Some(sym) = head_ref.symbolic_target() {
            if let Ok(r) = repo.find_reference(sym) {
                if let Some(t) = r.target() {
                    return Ok(t);
                }
            }
        }
    }
    // 仅一个有效远端分支时取它
    let mut names: Vec<String> = branch_list(repo, git2::BranchType::Remote)
        .into_iter()
        .map(|(n, _)| n)
        .filter(|n| n != "origin/HEAD")
        .collect();
    names.sort();
    if names.len() == 1 {
        if let Ok(r) = repo.find_reference(format!("refs/remotes/{}", names[0]).as_str()) {
            if let Some(t) = r.target() {
                return Ok(t);
            }
        }
    }
    Err("no remote head".into())
}

/// 分支清单 helper（Branches 迭代收敛；name/oid 二元组）
fn branch_list(repo: &Repository, t: git2::BranchType) -> Vec<(String, git2::Oid)> {
    let Ok(bs) = repo.branches(Some(t)) else {
        return Vec::new();
    };
    bs.into_iter()
        .filter_map(|r| r.ok())
        .map(|(b, _)| {
            let name = b.name().ok().flatten().unwrap_or("").to_string();
            let oid = b.get().target().unwrap_or(git2::Oid::zero());
            (name, oid)
        })
        .collect()
}

/// (isUpToDate, remoteUrl)：无 remote → (true, "")；fetch 失败 → (false, url)
fn up_to_date_state(repo: &Repository) -> (bool, String) {
    let remote_url = repo
        .find_remote("origin")
        .ok()
        .and_then(|r| r.url().map(String::from))
        .unwrap_or_default();
    if remote_url.is_empty() {
        return (true, String::new());
    }
    match fetch_origin(repo) {
        Ok(remote_head) => {
            let local = repo
                .head()
                .ok()
                .and_then(|h| h.target())
                .unwrap_or(git2::Oid::zero());
            (local == remote_head, remote_url)
        }
        Err(_) => (false, remote_url),
    }
}

fn checkout_head_tree(repo: &Repository) {
    let mut cb = CheckoutBuilder::new();
    cb.force();
    let _ = repo.checkout_head(Some(&mut cb));
}

/* ============================================================ *
 * /update
 * ============================================================ */
pub async fn update(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    // S19：任何 fetch/merge 动作之前拒绝
    if global_insteadof_present() {
        return insteadof_reject(origin);
    }
    let Some(name) = body.get("extensionName").and_then(|n| n.as_str()).map(String::from) else {
        return text_resp(
            StatusCode::BAD_REQUEST,
            "Bad Request: A valid extensionName is required in the request body.",
            origin,
        );
    };
    let global = body.get("global").and_then(|g| g.as_bool()).unwrap_or(false);
    let ext_path = match resolve_ext_dir(st, &name, global, origin.clone()) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let repo = match open_repo(&ext_path, origin.clone()) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let (mut is_up_to_date, remote_url) = up_to_date_state(&repo);
    if !is_up_to_date {
        // ST git.pull —— 只做 fast-forward；分歧/网络失败不动本地
        //（比 ST 更保守：非 ff 不硬落。登记在清单 T14）
        if let Ok(remote_head) = fetch_origin(&repo) {
            let local_head = repo
                .head()
                .ok()
                .and_then(|h| h.target())
                .unwrap_or(git2::Oid::zero());
            if remote_head != local_head {
                let branch_sh = repo
                    .head()
                    .ok()
                    .and_then(|h| h.shorthand().map(String::from))
                    .unwrap_or_default();
                if let Ok(remote_ref) =
                    repo.find_reference(format!("refs/remotes/origin/{branch_sh}").as_str())
                {
                    if let Ok(annotated) = repo.reference_to_annotated_commit(&remote_ref) {
                        let analysis_ok = repo
                            .merge_analysis(&[&annotated])
                            .map(|(a, _)| a.is_fast_forward())
                            .unwrap_or(false);
                        if analysis_ok {
                            let mut cb = CheckoutBuilder::new();
                            cb.force();
                            if repo.merge(&[&annotated], None, Some(&mut cb)).is_ok() {
                                is_up_to_date = true;
                            }
                        }
                        // 非 ff：保持 isUpToDate=false（App 提示；保守登记）
                    }
                } else {
                    is_up_to_date = true; // 远端分支不存在（只留 HEAD 面）
                }
            }
        }
    }
    let head = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .unwrap_or(git2::Oid::zero());
    ok_json(
        origin,
        json!({
            "shortCommitHash": head.to_string().chars().take(7).collect::<String>(),
            "extensionPath": ext_path.to_string_lossy(),
            "isUpToDate": is_up_to_date,
            "remoteUrl": remote_url,
        }),
    )
}

/* ============================================================ *
 * /version
 * ============================================================ */
pub async fn version(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(name) = body.get("extensionName").and_then(|n| n.as_str()).map(String::from) else {
        return text_resp(
            StatusCode::BAD_REQUEST,
            "Bad Request: A valid extensionName is required in the request body.",
            origin,
        );
    };
    let global = body.get("global").and_then(|g| g.as_bool()).unwrap_or(false);
    let ext_path = match resolve_ext_dir(st, &name, global, origin.clone()) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    // 非 repo/无提交 → 全空 + isUpToDate true（ST :415-418 字面）
    let Ok(repo) = Repository::open(&ext_path) else {
        return ok_json(
            origin,
            json!({ "currentBranchName": "", "currentCommitHash": "", "isUpToDate": true, "remoteUrl": "" }),
        );
    };
    let Ok(head_ref) = repo.head() else {
        return ok_json(
            origin,
            json!({ "currentBranchName": "", "currentCommitHash": "", "isUpToDate": true, "remoteUrl": "" }),
        );
    };
    let current_branch_name = head_ref.shorthand().unwrap_or("").to_string();
    let current_commit_hash = head_ref.target().map(|t| t.to_string()).unwrap_or_default();
    let (is_up_to_date, remote_url) = up_to_date_state(&repo);
    ok_json(
        origin,
        json!({
            "currentBranchName": current_branch_name,
            "currentCommitHash": current_commit_hash,
            "isUpToDate": is_up_to_date,
            "remoteUrl": remote_url,
        }),
    )
}

/* ============================================================ *
 * /branches
 * ============================================================ */
pub async fn branches(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(name) = body.get("extensionName").and_then(|n| n.as_str()).map(String::from) else {
        return text_resp(
            StatusCode::BAD_REQUEST,
            "Bad Request: A valid extensionName is required in the request body.",
            origin,
        );
    };
    let global = body.get("global").and_then(|g| g.as_bool()).unwrap_or(false);
    let ext_path = match resolve_ext_dir(st, &name, global, origin.clone()) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let repo = match open_repo(&ext_path, origin.clone()) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let current = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(String::from))
        .unwrap_or_default();

    let mut out: Vec<Value> = Vec::new();
    // 本地分支
    for (name, oid) in branch_list(&repo, git2::BranchType::Local) {
        out.push(json!({
            "current": current == name,
            "commit": oid.to_string(),
            "name": name,
            "label": name,
        }));
    }
    // 远端分支：fetch 全 refs 后枚举（网络失败 → 只给本地，ST 语义容忍）
    if repo.find_remote("origin").is_ok() {
        let mut fo = FetchOptions::new();
        if let Ok(mut remote) = repo.find_remote("origin") {
            let _ = remote.fetch(&["refs/heads/*:refs/remotes/origin/*"], Some(&mut fo), None);
        }
        for (short, oid) in branch_list(&repo, git2::BranchType::Remote) {
            if short == "origin/HEAD" {
                continue;
            }
            let bare = short.trim_start_matches("origin/").to_string();
            out.push(json!({
                "current": current == bare,
                "commit": oid.to_string(),
                "name": short,
                "label": short,
            }));
        }
    }
    ok_json(origin, json!(out))
}

/* ============================================================ *
 * /switch
 * ============================================================ */
pub async fn switch_branch(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let Some(name) = body
        .get("extensionName")
        .and_then(|n| n.as_str())
        .map(String::from)
    else {
        return text_resp(
            StatusCode::BAD_REQUEST,
            "Bad Request: A valid extensionName and branch are required in the request body.",
            origin,
        );
    };
    let global = body.get("global").and_then(|g| g.as_bool()).unwrap_or(false);
    let Some(branch) = body
        .get("branch")
        .and_then(|b| b.as_str())
        .map(String::from)
    else {
        return text_resp(
            StatusCode::BAD_REQUEST,
            "Bad Request: A valid extensionName and branch are required in the request body.",
            origin,
        );
    };
    let ext_path = match resolve_ext_dir(st, &name, global, origin.clone()) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let repo = match open_repo(&ext_path, origin.clone()) {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    if branch.starts_with("origin/") {
        let local = branch.trim_start_matches("origin/").to_string();
        if repo
            .find_reference(format!("refs/heads/{local}").as_str())
            .is_ok()
        {
            let _ = repo.set_head(format!("refs/heads/{local}").as_str());
            checkout_head_tree(&repo);
            return no_content(origin);
        }
        // 从远端创建本地（checkoutBranch 语义）
        let remote_ref = match repo.find_reference(branch.as_str()) {
            Ok(r) => r,
            Err(_) => {
                return text_resp(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error. Check the server logs for more details.",
                    origin,
                )
            }
        };
        let commit = match remote_ref.peel_to_commit() {
            Ok(c) => c,
            Err(_) => {
                return text_resp(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error. Check the server logs for more details.",
                    origin,
                )
            }
        };
        if repo.branch(&local, &commit, false).is_err() {
            return text_resp(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error. Check the server logs for more details.",
                origin,
            );
        }
        let _ = repo.set_head(format!("refs/heads/{local}").as_str());
        checkout_head_tree(&repo);
        return no_content(origin);
    }

    // 本地分支必须存在（ST :306-309）
    if repo
        .find_reference(format!("refs/heads/{branch}").as_str())
        .is_err()
    {
        return text_resp(
            StatusCode::NOT_FOUND,
            format!("Branch {branch} does not exist locally"),
            origin,
        );
    }
    let is_current = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s == branch))
        .unwrap_or(false);
    if !is_current {
        let _ = repo.set_head(format!("refs/heads/{branch}").as_str());
        checkout_head_tree(&repo);
    }
    no_content(origin)
}

/* ============================================================ *
 * /move
 * ============================================================ */
pub async fn move_ext(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(name) = body
        .get("extensionName")
        .and_then(|n| n.as_str())
        .map(String::from)
    else {
        return text_resp(
            StatusCode::BAD_REQUEST,
            "Bad Request: A valid extensionName, source, and destination are required in the request body.",
            origin,
        );
    };
    let source = body
        .get("source")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let destination = body
        .get("destination")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let safe = match sanitize_name(&name) {
        Ok(s) => s,
        Err(_) => {
            return text_resp(
                StatusCode::BAD_REQUEST,
                "Bad Request: A valid extensionName, source, and destination are required in the request body.",
                origin,
            )
        }
    };
    if source.is_empty() || destination.is_empty() {
        return text_resp(
            StatusCode::BAD_REQUEST,
            "Bad Request: A valid extensionName, source, and destination are required in the request body.",
            origin,
        );
    }
    let src_dir = ext_dir_for(st, source == "global");
    let dst_dir = ext_dir_for(st, destination == "global");
    let source_path = match crate::pathguard::join_under(&src_dir, &[&safe]) {
        Ok(p) => p,
        Err(_) => return text_resp(StatusCode::FORBIDDEN, "扩展名不得越出基础目录", origin),
    };
    let destination_path = match crate::pathguard::join_under(&dst_dir, &[&safe]) {
        Ok(p) => p,
        Err(_) => return text_resp(StatusCode::FORBIDDEN, "扩展名不得越出基础目录", origin),
    };
    if !source_path.is_dir() {
        return text_resp(StatusCode::NOT_FOUND, "Source directory does not exist.", origin);
    }
    if destination_path.exists() {
        return text_resp(
            StatusCode::CONFLICT,
            "Destination directory already exists.",
            origin,
        );
    }
    if source == destination {
        return text_resp(
            StatusCode::CONFLICT,
            "Source and destination directories are the same.",
            origin,
        );
    }
    match std::fs::rename(&source_path, &destination_path) {
        Ok(()) => {}
        Err(_) => {
            if copy_recursive(&source_path, &destination_path).is_err() {
                return text_resp(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error. Check the server logs for more details.",
                    origin,
                );
            }
            let _ = std::fs::remove_dir_all(&source_path);
        }
    }
    no_content(origin)
}

fn copy_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/* ============================================================ *
 * /delete
 * ============================================================ */
pub async fn delete(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(name) = body
        .get("extensionName")
        .and_then(|n| n.as_str())
        .map(String::from)
    else {
        return text_resp(
            StatusCode::BAD_REQUEST,
            "Bad Request: A valid extensionName is required in the request body.",
            origin,
        );
    };
    let global = body.get("global").and_then(|g| g.as_bool()).unwrap_or(false);
    let ext_path = match resolve_ext_dir(st, &name, global, origin.clone()) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let display = ext_path.display().to_string();
    match std::fs::remove_dir_all(&ext_path) {
        Ok(()) => text_resp(
            StatusCode::OK,
            format!("Extension has been deleted at {display}"),
            origin,
        ),
        Err(_) => text_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error. Check the server logs for more details.",
            origin,
        ),
    }
}

/* ============================================================ *
 * /discover
 * ============================================================ */
pub fn discover(st: &BackendState, origin: Option<HeaderValue>) -> Response {
    let ext_dir = user_ext_dir(st);
    let global_dir = global_ext_dir(st);
    let _ = std::fs::create_dir_all(&ext_dir);
    let _ = std::fs::create_dir_all(&global_dir);
    let mut out: Vec<Value> = Vec::new();

    let mut local_names: Vec<Value> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&ext_dir) {
        let mut names: Vec<String> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        for d in names {
            if !ext_dir.join(&d).join("manifest.json").is_file() {
                continue; // 幽灵扩展（extensions.js:497-506）
            }
            local_names.push(json!({ "type": "local", "name": format!("third-party/{d}") }));
        }
    }
    out.extend(local_names.clone());

    // global 组（冲突时用户目录优先 —— extensions.js:521-527）
    if let Ok(rd) = std::fs::read_dir(&global_dir) {
        let mut names: Vec<String> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        for d in names {
            if !global_dir.join(&d).join("manifest.json").is_file() {
                continue;
            }
            let name = json!(format!("third-party/{d}"));
            if local_names.iter().any(|l| l.get("name") == Some(&name)) {
                continue;
            }
            out.push(json!({ "type": "global", "name": name }));
        }
    }
    ok_json(origin, json!(out))
}

/// POST /api/quick-replies/save {name, ...}（manifest #67）
/// QR 集整文件落盘 QuickReplies/<name>.json；200 空 body（前端 stPostVoid）。
pub fn qr_save(st: &BackendState, body: &Value, origin: Option<HeaderValue>) -> Response {
    let Some(name) = body
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|n| !n.trim().is_empty())
    else {
        return apply_cors_resp(StatusCode::BAD_REQUEST.into_response(), origin);
    };
    let Ok(safe) = crate::pathguard::component(&format!("{name}.json")) else {
        return apply_cors_resp(
            (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": { "message": "非法文件名" } })),
            )
                .into_response(),
            origin,
        );
    };
    let dir = st.data_root.join("default-user").join("QuickReplies");
    if let Some(parent) = dir.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::create_dir_all(&dir);
    let pretty = serde_json::to_string_pretty(body).unwrap_or_default();
    let p = dir.join(&safe);
    let tmp = p.with_extension("json.tmp");
    match std::fs::write(&tmp, &pretty).and_then(|_| std::fs::rename(&tmp, &p)) {
        Ok(()) => apply_cors_resp(StatusCode::OK.into_response(), origin),
        Err(e) => apply_cors_resp(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "message": format!("QR 写入失败：{e}") } })),
            )
                .into_response(),
            origin,
        ),
    }
}

#[allow(dead_code)]
fn _witness(_: axum::body::Body) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    fn st_with(tag: &str) -> BackendState {
        let root = std::env::temp_dir().join(format!("stchat-ext-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        BackendState {
            token: Arc::new("t".into()),
            data_root: root,
            max_body_bytes: 1024,
            settings_svc: Arc::new(super::super::settings::SettingsService::new()),
            chats_svc: Arc::new(super::super::chats::ChatsService::new()),
            maid_tokens: Arc::new(super::super::extras::MaidTokens::new()),
            vault: Arc::new(super::super::vault::Vault::new_memory()),
        }
    }

    fn body_bytes(r: Response) -> Value {
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap() });
        serde_json::from_slice::<Value>(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).to_string()))
    }

    #[test]
    fn url_validation_rejects_non_https_and_ext() {
        assert!(validate_extension_url("http://x/y.git").is_err(), "http 拒（native 收紧）");
        assert!(validate_extension_url("ext::sh -c x").is_err(), "ext:: 拒（P1-1 命令执行向量）");
        assert!(validate_extension_url("file:///etc/passwd").is_err());
        assert!(validate_extension_url("ssh://x").is_err());
        assert!(validate_extension_url("git://x").is_err());
        assert!(validate_extension_url("127.0.0.1:5001/x").is_err(), "无协议拒");
        let (url, name) = validate_extension_url("https://github.com/u/Some-Repo.git").unwrap();
        assert_eq!(url, "https://github.com/u/Some-Repo.git");
        assert_eq!(name, "Some-Repo");
        assert_eq!(
            validate_extension_url("https://git.deep.host/a/b/c/pkg").unwrap().1,
            "pkg"
        );
    }

    /// S19 测试的串行锁：libgit2 的配置搜索路径是**进程级全局态**（且首次使用后
    /// 缓存解析结果，之后改 HOME 不再生效）—— 所有触碰 install/update handler
    /// 或搜索路径的测试必须经此串行化
    static GIT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// libgit2 全局配置搜索路径守卫：set 时记录旧值，Drop 时还原
    ///（比 HOME 注入确定 —— 不依赖 libgit2 是否已缓存解析路径）
    struct SearchPathGuard(std::option::Option<std::ffi::CString>);
    impl SearchPathGuard {
        /// unsafe：mutates libgit2 global state（本测试进程独占，外部以 GIT_ENV_LOCK 同步）
        fn point_to(dir: &std::path::Path) -> Self {
            let old = unsafe { git2::opts::get_search_path(git2::ConfigLevel::Global) }.ok();
            unsafe {
                git2::opts::set_search_path(git2::ConfigLevel::Global, dir).unwrap();
                // XDG 一并清空，排除机器上 %APPDATA%\git\config 的干扰
                git2::opts::set_search_path(git2::ConfigLevel::XDG, dir).unwrap();
            }
            Self(old)
        }
    }
    impl Drop for SearchPathGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.0 {
                    Some(old) => {
                        let _ = git2::opts::set_search_path(
                            git2::ConfigLevel::Global,
                            std::path::PathBuf::from(old.to_string_lossy().to_string()),
                        );
                    }
                    None => {
                        let _ = git2::opts::reset_search_path(git2::ConfigLevel::Global);
                    }
                }
                let _ = git2::opts::reset_search_path(git2::ConfigLevel::XDG);
            }
        }
    }

    fn empty_cfg_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("stchat-ext-emptycfg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn install_rejects_existing_dir_with_409() {
        let _g = GIT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let clean = empty_cfg_dir("409");
        let _sp = SearchPathGuard::point_to(&clean);
        let st = st_with("inst409");
        std::fs::create_dir_all(user_ext_dir(&st).join("Some-Repo")).unwrap();
        let r = tokio::runtime::Runtime::new().unwrap().block_on(install(
            &st,
            &json!({ "url": "https://github.com/u/Some-Repo.git" }),
            None,
        ));
        assert_eq!(r.status().as_u16(), 409);
    }

    #[test]
    fn insteadof_predicate_detects_rewrite_rules() {
        // S19 谓词：可写 config 注入 insteadOf → 命中；纯普通键 → 不命中
        let dir =
            std::env::temp_dir().join(format!("stchat-ext-insteadof-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("gitconfig");
        std::fs::write(
            &cfg_path,
            "[url \"https://evil.example/\"]\n\tinsteadOf = https://github.com/\n",
        )
        .unwrap();
        let cfg = git2::Config::open(&cfg_path).unwrap();
        assert!(config_has_insteadof(&cfg), "insteadOf 键必须被探测到");
        // 大小写变体（git 键节大小写不敏感）
        std::fs::write(
            &cfg_path,
            "[URL \"ssh://evil/\"]\n\tinsteadof = https://github.com/\n",
        )
        .unwrap();
        let cfg = git2::Config::open(&cfg_path).unwrap();
        assert!(config_has_insteadof(&cfg), "大小写不敏感匹配");
        // 无 insteadOf → false
        std::fs::write(&cfg_path, "[user]\n\tname = t\n\temail = t@e\n").unwrap();
        let cfg = git2::Config::open(&cfg_path).unwrap();
        assert!(!config_has_insteadof(&cfg));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 测试期环境变量守卫已由 SearchPathGuard 取代（libgit2 缓存解析结果，
    /// HOME 注入不可靠，详见 GIT_ENV_LOCK 注释）

    #[test]
    fn extension_name_traversal_rejected_and_leaves_no_files() {
        // S7：extensionName 穿越 —— handler 级断言（400/403 文本错误），
        // 且数据目录外/扩展目录内无任何新文件
        let _g = GIT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let clean = empty_cfg_dir("trav");
        let _sp = SearchPathGuard::point_to(&clean);
        let st = st_with("extrav");
        std::fs::create_dir_all(user_ext_dir(&st)).unwrap();
        for name in ["../../evil", "a/b", "..\\evil"] {
            let r = tokio::runtime::Runtime::new().unwrap().block_on(update(
                &st,
                &json!({ "extensionName": name }),
                None,
            ));
            assert!(
                r.status().as_u16() == 400 || r.status().as_u16() == 403,
                "extensionName {name:?} 必须拒（400/403），实际 {}",
                r.status()
            );
            let r = tokio::runtime::Runtime::new().unwrap().block_on(delete(
                &st,
                &json!({ "extensionName": name }),
                None,
            ));
            assert!(
                r.status().as_u16() == 400 || r.status().as_u16() == 403,
                "delete extensionName {name:?} 必须拒，实际 {}",
                r.status()
            );
        }
        let outside_target = st.data_root.parent().unwrap().join("evil");
        assert!(!outside_target.exists(), "数据目录外无新文件");
        assert!(
            std::fs::read_dir(user_ext_dir(&st)).unwrap().count() == 0,
            "扩展目录内也不得有越界落盘"
        );
    }

    #[test]
    fn install_and_update_rejected_with_global_insteadof() {
        // S19 handler 级：全局 gitconfig（显式搜索路径注入，Windows 下 git2 可写
        // config 后还原）含 insteadOf → install/update 一律 400，且发生在任何
        // clone/fetch/落盘之前
        let _g = GIT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home =
            std::env::temp_dir().join(format!("stchat-ext-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join(".gitconfig"),
            "[url \"https://evil.example/\"]\n\tinsteadOf = https://github.com/\n",
        )
        .unwrap();
        let _sp = SearchPathGuard::point_to(&home);
        assert!(global_insteadof_present(), "open_default 必须读到注入的全局配置");

        let st = st_with("instblock");
        let r = tokio::runtime::Runtime::new().unwrap().block_on(install(
            &st,
            &json!({ "url": "https://github.com/u/Some-Repo.git" }),
            None,
        ));
        assert_eq!(
            r.status().as_u16(),
            400,
            "全局 gitconfig 含 insteadOf → install 拒 400"
        );
        assert!(
            !user_ext_dir(&st).join("Some-Repo").exists(),
            "拒绝必须发生在落盘之前"
        );
        let r = tokio::runtime::Runtime::new().unwrap().block_on(update(
            &st,
            &json!({ "extensionName": "Some-Repo" }),
            None,
        ));
        assert_eq!(
            r.status().as_u16(),
            400,
            "全局 gitconfig 含 insteadOf → update 拒 400"
        );
    }

    #[test]
    fn resolve_404_and_plain_dir_behaviors() {
        let _g = GIT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let clean = empty_cfg_dir("res404");
        let _sp = SearchPathGuard::point_to(&clean);
        let st = st_with("res404");
        // update：不存在 → 404 文本（ST 字面）
        let r = tokio::runtime::Runtime::new().unwrap().block_on(update(
            &st,
            &json!({ "extensionName": "ghost" }),
            None,
        ));
        assert_eq!(r.status().as_u16(), 404);
        // update：存在但非 repo → 500 文本
        std::fs::create_dir_all(user_ext_dir(&st).join("plain")).unwrap();
        let r = tokio::runtime::Runtime::new().unwrap().block_on(update(
            &st,
            &json!({ "extensionName": "plain" }),
            None,
        ));
        assert_eq!(r.status().as_u16(), 500);
        // version：非 repo → 全空 + isUpToDate true（ST :415-418）
        let r = tokio::runtime::Runtime::new().unwrap().block_on(version(
            &st,
            &json!({ "extensionName": "plain" }),
            None,
        ));
        assert_eq!(body_bytes(r)["currentCommitHash"], "");
    }

    fn make_local_repo(path: &std::path::Path) -> git2::Oid {
        let repo = Repository::init(path).unwrap();
        std::fs::write(path.join("manifest.json"), "{}").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("manifest.json")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("t", "t@e").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap()
    }

    #[test]
    fn local_repo_flows_offline() {
        let st = st_with("localrepo");
        let dir = user_ext_dir(&st);
        std::fs::create_dir_all(&dir).unwrap();
        let repo_dir = dir.join("real-ext");
        let oid = make_local_repo(&repo_dir);

        // /version：无 origin → 本地面（remoteUrl=""，isUpToDate=true）
        let r = tokio::runtime::Runtime::new().unwrap().block_on(version(
            &st,
            &json!({ "extensionName": "real-ext" }),
            None,
        ));
        let v = body_bytes(r);
        assert_eq!(v["currentCommitHash"].as_str().unwrap(), oid.to_string());
        assert_eq!(v["isUpToDate"], true);
        assert_eq!(v["remoteUrl"], "");

        // 本地分支 switch：建分支并切换
        {
            let repo = Repository::open(&repo_dir).unwrap();
            repo.branch("test-branch", &repo.find_commit(oid).unwrap(), false).unwrap();
        }
        let r = tokio::runtime::Runtime::new().unwrap().block_on(switch_branch(
            &st,
            &json!({ "extensionName": "real-ext", "branch": "test-branch" }),
            None,
        ));
        assert_eq!(r.status().as_u16(), 204);
        assert_eq!(
            Repository::open(&repo_dir)
                .unwrap()
                .head()
                .unwrap()
                .shorthand()
                .unwrap(),
            "test-branch"
        );
        // 不存在的本地分支 → 404（ST 字面）
        let r = tokio::runtime::Runtime::new().unwrap().block_on(switch_branch(
            &st,
            &json!({ "extensionName": "real-ext", "branch": "nope" }),
            None,
        ));
        assert_eq!(r.status().as_u16(), 404);
        // 已在当前分支 → 204 幂等
        let r = tokio::runtime::Runtime::new().unwrap().block_on(switch_branch(
            &st,
            &json!({ "extensionName": "real-ext", "branch": "test-branch" }),
            None,
        ));
        assert_eq!(r.status().as_u16(), 204);

        // /branches：本地 + 无远端（网络面未启用 → 只给本地）
        let r = tokio::runtime::Runtime::new().unwrap().block_on(branches(
            &st,
            &json!({ "extensionName": "real-ext" }),
            None,
        ));
        let arr = body_bytes(r).as_array().unwrap().clone();
        assert!(arr.iter().any(|b| b["name"] == "test-branch" && b["current"] == true));
    }

    #[test]
    fn move_and_delete_flows() {
        let st = st_with("movedel");
        let dir = user_ext_dir(&st);
        let gdir = global_ext_dir(&st);
        std::fs::create_dir_all(dir.join("ext-a").join("sub")).unwrap();
        std::fs::write(dir.join("ext-a").join("sub").join("f.txt"), "x").unwrap();

        // global 目标有同名 → 409
        std::fs::create_dir_all(gdir.join("ext-a")).unwrap();
        let r = tokio::runtime::Runtime::new().unwrap().block_on(move_ext(
            &st,
            &json!({ "extensionName": "ext-a", "source": "user", "destination": "global" }),
            None,
        ));
        assert_eq!(r.status().as_u16(), 409);
        // 源 == 目标语义（dest 已存在先触发 —— 与 ST 检查序一致）
        let r = tokio::runtime::Runtime::new().unwrap().block_on(move_ext(
            &st,
            &json!({ "extensionName": "ext-a", "source": "user", "destination": "user" }),
            None,
        ));
        assert_eq!(r.status().as_u16(), 409);
        // 不存在 → 404
        let r = tokio::runtime::Runtime::new().unwrap().block_on(move_ext(
            &st,
            &json!({ "extensionName": "ghost", "source": "user", "destination": "global" }),
            None,
        ));
        assert_eq!(r.status().as_u16(), 404);
        // 正常移动
        std::fs::remove_dir_all(gdir.join("ext-a")).unwrap();
        let r = tokio::runtime::Runtime::new().unwrap().block_on(move_ext(
            &st,
            &json!({ "extensionName": "ext-a", "source": "user", "destination": "global" }),
            None,
        ));
        assert_eq!(r.status().as_u16(), 204);
        assert!(gdir.join("ext-a").join("sub").join("f.txt").is_file());
        assert!(!dir.join("ext-a").exists());

        // delete → 200 + 文本（ST 字面）
        let r = tokio::runtime::Runtime::new().unwrap().block_on(delete(
            &st,
            &json!({ "extensionName": "ext-a", "global": true }),
            None,
        ));
        assert_eq!(r.status().as_u16(), 200);
        assert!(!gdir.join("ext-a").exists());
        // 再删 → 404
        let r = tokio::runtime::Runtime::new().unwrap().block_on(delete(
            &st,
            &json!({ "extensionName": "ext-a", "global": true }),
            None,
        ));
        assert_eq!(r.status().as_u16(), 404);
    }

    #[test]
    fn discover_lists_local_and_global_groups() {
        let st = st_with("discgrp");
        let dir = user_ext_dir(&st);
        let gdir = global_ext_dir(&st);
        std::fs::create_dir_all(dir.join("mine")).unwrap();
        std::fs::write(dir.join("mine").join("manifest.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("ghost")).unwrap(); // 幽灵
        std::fs::create_dir_all(gdir.join("shared-ext")).unwrap();
        std::fs::write(gdir.join("shared-ext").join("manifest.json"), "{}").unwrap();
        std::fs::create_dir_all(gdir.join("mine")).unwrap(); // 冲突 → 用户优先

        let r = discover(&st, None);
        let arr = body_bytes(r).as_array().unwrap().clone();
        assert_eq!(arr.len(), 2, "ghost 跳过 + 冲突取用户侧");
        assert_eq!(arr[0]["name"], "third-party/mine");
        assert_eq!(arr[0]["type"], "local");
        assert_eq!(arr[1]["name"], "third-party/shared-ext");
        assert_eq!(arr[1]["type"], "global");
    }

    #[test]
    fn qr_save_roundtrip() {
        let st = st_with("qr");
        let body = json!({ "name": "我的集", "qrList": [{ "label": "hi" }] });
        let r = qr_save(&st, &body, None);
        assert_eq!(r.status().as_u16(), 200);
        let pretty = std::fs::read_to_string(
            st.data_root
                .join("default-user")
                .join("QuickReplies")
                .join("我的集.json"),
        )
        .unwrap();
        assert!(pretty.contains("qrList"));
        // 无 name 400
        let r = qr_save(&st, &json!({}), None);
        assert_eq!(r.status().as_u16(), 400);
    }
}
