//! 组件下载器（S4）
//!
//! ── 设计依据全部来自实测，不是拍脑袋 ──
//! * node 官方源与 `cdn.npmmirror.com` 都返回 200 / 34.0MB / `Accept-Ranges: bytes`
//!   → **可以断点续传**
//! * ST 的源码快照（zipball）**不支持 Range**（动态生成、302 到 codeload）
//!   → 对它只能整包重下，本模块按服务端的实际响应自适应
//! * gh-proxy 拉 GitHub 只有 **~0.34 MB/s**，codeload 直连会超时
//!   → **多源回退是必需的，不是可选项**
//!
//! ── 三条安全约定 ──
//! 1. 先写 `.part` 临时文件，**校验通过才 rename** —— 绝不留下"看起来下好了"的脏文件
//! 2. **只有服务端真返回 206 才续传**；返回 200 就截断重下（避免拼接出损坏文件）
//! 3. 校验失败立即删除并报错，不留残骸

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// 单次「多久没收到新数据」的容忍时间。
/// 用停滞检测而不是总超时：大文件本来就要下几分钟，
/// 但只要一直有数据在流就不该被判失败。
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
/// 同一个源最多重试次数
const MAX_RETRY_PER_SOURCE: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSpec {
    pub id: String,
    /// 按优先级排列的地址（官方优先、镜像兜底）
    pub sources: Vec<String>,
    /// 期望的 sha256（十六进制，大小写不敏感）；给了就强校验
    pub sha256: Option<String>,
    /// 期望体积（进度显示与磁盘预检用；服务端给得出就以此为准）
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub id: String,
    /// connecting | downloading | verifying | retrying | done
    pub phase: String,
    pub source: String,
    /// 正在试第几个源（1-based）
    pub source_index: usize,
    pub source_total: usize,
    pub done: u64,
    pub total: u64,
    /// 从多少字节处续传（0 = 从头下）
    pub resumed_from: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadOutcome {
    pub id: String,
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    /// 最终成功的源
    pub source: String,
    pub resumed_from: u64,
    pub elapsed_ms: u64,
}

/// 构造 HTTP 客户端。
///
/// 默认**不走环境变量代理** —— 本机常驻沙箱代理（`127.0.0.1:57962`）会把公网请求打死，
/// 而实测直连 nodejs.org / GitHub 都通。用户确实需要代理时显式传入。
pub fn build_client(proxy: Option<&str>) -> Result<reqwest::Client, String> {
    let mut b = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .user_agent(concat!(
            "ST-Chat/",
            env!("CARGO_PKG_VERSION"),
            " (component-downloader)"
        ));

    match proxy.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            let proxy = reqwest::Proxy::all(p).map_err(|e| format!("代理地址无效（{p}）：{e}"))?;
            b = b.proxy(proxy);
        }
        None => b = b.no_proxy(),
    }

    b.build().map_err(|e| format!("HTTP 客户端创建失败：{e}"))
}

fn part_path(dest: &Path) -> PathBuf {
    PathBuf::from(format!("{}.part", dest.display()))
}

/// 计算文件 sha256
async fn sha256_file(p: &Path) -> Result<String, String> {
    let mut f = tokio::fs::File::open(p)
        .await
        .map_err(|e| format!("读取失败 {}：{e}", p.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f
            .read(&mut buf)
            .await
            .map_err(|e| format!("读取失败 {}：{e}", p.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 下载到 `dest`。多源顺序尝试，任一成功即返回。
///
/// `on` 会被高频调用，调用方自行节流或直接转发（本模块已按 200ms 限流）。
pub async fn download<F>(
    client: &reqwest::Client,
    spec: &DownloadSpec,
    dest: &Path,
    mut on: F,
) -> Result<DownloadOutcome, String>
where
    F: FnMut(DownloadProgress),
{
    if spec.sources.is_empty() {
        return Err(format!("[{}] 没有可用的下载源", spec.id));
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建目录失败 {}：{e}", parent.display()))?;
    }

    let part = part_path(dest);
    let total_sources = spec.sources.len();
    let started = Instant::now();
    let mut errors: Vec<String> = Vec::new();

    for (idx, url) in spec.sources.iter().enumerate() {
        let source_index = idx + 1;

        for attempt in 1..=MAX_RETRY_PER_SOURCE {
            // 每次尝试前重新读 .part 大小 —— 上一次失败可能已经下了一部分
            let existing = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);

            on(DownloadProgress {
                id: spec.id.clone(),
                phase: if attempt > 1 { "retrying".into() } else { "connecting".into() },
                source: url.clone(),
                source_index,
                source_total: total_sources,
                done: existing,
                total: spec.size.unwrap_or(0),
                resumed_from: existing,
                message: if attempt > 1 {
                    format!("第 {source_index} 个源第 {attempt} 次尝试…")
                } else {
                    format!("连接第 {source_index}/{total_sources} 个源…")
                },
            });

            match try_once(client, spec, url, &part, existing, &mut on, source_index, total_sources)
                .await
            {
                Ok(bytes) => {
                    // ---- 校验（不信任"下完了"，只信任"算出来一致"）----
                    on(DownloadProgress {
                        id: spec.id.clone(),
                        phase: "verifying".into(),
                        source: url.clone(),
                        source_index,
                        source_total: total_sources,
                        done: bytes,
                        total: bytes,
                        resumed_from: existing,
                        message: "校验文件完整性…".into(),
                    });

                    let actual = sha256_file(&part).await?;
                    if let Some(expect) = spec.sha256.as_ref().filter(|s| !s.trim().is_empty()) {
                        if !actual.eq_ignore_ascii_case(expect.trim()) {
                            let _ = tokio::fs::remove_file(&part).await;
                            errors.push(format!(
                                "{url} 校验失败（期望 {}… 实际 {}…），已删除",
                                &expect[..expect.len().min(12)],
                                &actual[..actual.len().min(12)]
                            ));
                            break; // 换下一个源
                        }
                    }

                    tokio::fs::rename(&part, dest)
                        .await
                        .map_err(|e| format!("重命名失败 {} → {}：{e}", part.display(), dest.display()))?;

                    let elapsed_ms = started.elapsed().as_millis() as u64;
                    on(DownloadProgress {
                        id: spec.id.clone(),
                        phase: "done".into(),
                        source: url.clone(),
                        source_index,
                        source_total: total_sources,
                        done: bytes,
                        total: bytes,
                        resumed_from: existing,
                        message: "下载完成".into(),
                    });

                    return Ok(DownloadOutcome {
                        id: spec.id.clone(),
                        path: dest.to_string_lossy().into_owned(),
                        bytes,
                        sha256: actual,
                        source: url.clone(),
                        resumed_from: existing,
                        elapsed_ms,
                    });
                }
                Err(e) => {
                    errors.push(format!("{url}（第 {attempt} 次）：{e}"));
                    // 网络类错误保留 .part 以便续传；只有明确"文件损坏"才清
                    if e.contains("文件扩展名不符") {
                        let _ = tokio::fs::remove_file(&part).await;
                    }
                }
            }
        }
    }

    Err(format!(
        "[{}] 所有 {} 个源都失败：\n{}",
        spec.id,
        total_sources,
        errors.join("\n")
    ))
}

/// 对单个源做一次下载尝试。返回累计字节数。
#[allow(clippy::too_many_arguments)]
async fn try_once<F>(
    client: &reqwest::Client,
    spec: &DownloadSpec,
    url: &str,
    part: &Path,
    existing: u64,
    on: &mut F,
    source_index: usize,
    source_total: usize,
) -> Result<u64, String>
where
    F: FnMut(DownloadProgress),
{
    let mut req = client.get(url);
    if existing > 0 {
        req = req.header("Range", format!("bytes={existing}-"));
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("请求失败：{e}"))?;
    let status = resp.status();

    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }

    // 只有 206 才是真的续传；200 说明服务端忽略了 Range，必须从头下
    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT && existing > 0;
    let already = if resumed { existing } else { 0 };

    let mut file = if resumed {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(part)
            .await
            .map_err(|e| format!("打开 .part 追加失败：{e}"))?
    } else {
        tokio::fs::File::create(part)
            .await
            .map_err(|e| format!("创建 .part 失败：{e}"))?
    };

    let total = match (resumed, resp.content_length()) {
        (true, Some(l)) => l + already,
        (false, Some(l)) => l,
        _ => spec.size.unwrap_or(0),
    };

    let mut done = already;
    let mut stream = resp.bytes_stream();
    let mut last_emit = Instant::now();

    loop {
        let next = tokio::time::timeout(STALL_TIMEOUT, stream.next()).await;
        let chunk = match next {
            Err(_) => {
                return Err(format!(
                    "超过 {}s 没有新数据（已下 {} 字节）",
                    STALL_TIMEOUT.as_secs(),
                    done
                ))
            }
            Ok(None) => break,
            Ok(Some(Err(e))) => return Err(format!("传输中断：{e}")),
            Ok(Some(Ok(c))) => c,
        };

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入失败：{e}"))?;
        done += chunk.len() as u64;

        if last_emit.elapsed() >= PROGRESS_INTERVAL {
            last_emit = Instant::now();
            on(DownloadProgress {
                id: spec.id.clone(),
                phase: "downloading".into(),
                source: url.to_string(),
                source_index,
                source_total,
                done,
                total,
                resumed_from: already,
                message: String::new(),
            });
        }
    }

    file.flush().await.map_err(|e| format!("flush 失败：{e}"))?;
    drop(file);

    // 体积明显不符 → 视为损坏（服务端给了长度才判）
    if total > 0 && done != total {
        return Err(format!("字节数不符（期望 {total}，实得 {done}）"));
    }

    Ok(done)
}

/* ------------------------------------------------------------------ *
 * 测试
 * ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_path_appends_suffix() {
        let p = part_path(Path::new("C:/x/node.zip"));
        assert!(p.to_string_lossy().ends_with("node.zip.part"));
    }

    #[tokio::test]
    async fn sha256_matches_known_vector() {
        let dir = std::env::temp_dir().join(format!("stchat-dl-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("a.txt");
        tokio::fs::write(&f, b"abc").await.unwrap();
        // "abc" 的 sha256 是众所周知的值，用它证明我们算的不是别的东西
        let got = sha256_file(&f).await.unwrap();
        assert_eq!(
            got,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn client_without_proxy_builds() {
        assert!(build_client(None).is_ok());
        assert!(build_client(Some("")).is_ok(), "空字符串应视作不配代理");
        assert!(build_client(Some("   ")).is_ok(), "纯空白同样视作不配代理");
        assert!(build_client(Some("http://127.0.0.1:7890")).is_ok());
        // 注：实测 reqwest **不在构建期**校验代理 URL 合法性（传乱字符串也能 build 成功），
        // 所以这里不断言"非法地址必须报错" —— 那是它的行为，不是我们的逻辑。
    }
}
