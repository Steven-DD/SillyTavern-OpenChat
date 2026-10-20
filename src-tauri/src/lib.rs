//! 全局：MSVC linker 本地化 stdout 会触发 linker_messages 警告（cargo 2024 起）
#![allow(linker_messages)]
//! SillyTavern-OpenChat · Tauri 应用入口（native backend 自包含架构）
//!
//! 启动顺序：
//!   0. 插件宿主已移除：数据根目录固定为应用数据目录下的 stoc-runtime/data
//!      （启动时一次性采用旧 identifier 目录下的 st-runtime，见 st_data::adopt_legacy_root）
//!   1. backend::start(data_root) —— Rust 本地 HTTP 后端（含鉴权 token）
//!   2. 把基地址与 token 通过 `backend_base` 命令交给前端
//!   3. 退出时无外部进程需要清理

pub mod backend;
pub mod compat;
pub mod pathguard;
pub mod st_data;

use std::path::{Path, PathBuf};
use tauri::Manager;

/// 日志级别：默认 Info；排障时设 `STOC_LOG=debug|trace`（重启生效）。
/// 旧名 `ST_CHAT_LOG` 仍兼容读取（一次性过渡，避免既有使用说明失效）。
///
/// 生产（release）无控制台，日志落 appLogDir 滚动文件；Info 默认既能覆盖
/// 请求生命周期/落盘/出站，又不会把 reqwest/hyper 的 debug 噪音写爆文件。
fn log_level() -> log::LevelFilter {
    match std::env::var("STOC_LOG").or_else(|_| std::env::var("ST_CHAT_LOG"))
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "trace" => log::LevelFilter::Trace,
        "debug" => log::LevelFilter::Debug,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        "off" => log::LevelFilter::Off,
        _ => log::LevelFilter::Info,
    }
}

/// 中继/后端端点信息（base + 鉴权 token）
#[derive(serde::Serialize)]
pub struct BackendEndpointInfo {
    pub base: String,
    pub token: String,
}

/// 数据根目录状态
pub struct DataRootState(pub PathBuf);

/// 后端控制槽
pub struct BackendSlot(pub std::sync::Mutex<Option<backend::BackendHandle>>);

#[tauri::command]
fn backend_base(state: tauri::State<'_, BackendSlot>) -> Result<BackendEndpointInfo, String> {
    let g = state.0.lock().map_err(|_| "backend 槽被污染".to_string())?;
    let h = g.as_ref().ok_or_else(|| {
        log::warn!(target: backend::logx::IPC, "backend_base 被调用但 native backend 未启动");
        "native backend 未启动".to_string()
    })?;
    log::debug!(target: backend::logx::IPC, "backend_base 下发 base=http://127.0.0.1:{}", h.port);
    Ok(BackendEndpointInfo {
        base: format!("http://127.0.0.1:{}", h.port),
        token: h.token.clone(),
    })
}



/* ================================================================== *
 * 会话 jsonl 行级增量操作
 * ================================================================== */

#[derive(serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ChatLineOp {
    Replace { index: usize, json: String },
    Delete { index: usize },
    Insert { index: usize, json: String },
}

fn chat_jsonl_path(data_root: &Path, avatar: &str, file: &str) -> Result<PathBuf, String> {
    let base = data_root.join("default-user").join("chats");
    let name = if file.ends_with(".jsonl") {
        file.to_string()
    } else {
        format!("{file}.jsonl")
    };
    let p = pathguard::join_under(&base, &[avatar, &name]).map_err(|e| e.to_string())?;
    Ok(p)
}

fn character_png_path(data_root: &Path, avatar: &str) -> Result<PathBuf, String> {
    let base = data_root.join("default-user").join("characters");
    let p = pathguard::join_under(&base, &[avatar]).map_err(|e| e.to_string())?;
    Ok(p)
}

#[tauri::command]
async fn chat_mutate_lines(
    state: tauri::State<'_, DataRootState>,
    avatar: String,
    file: String,
    ops: Vec<ChatLineOp>,
) -> Result<(), String> {
    let path = chat_jsonl_path(&state.0, &avatar, &file)?;
    let n = ops.len();
    log::info!(
        target: backend::logx::FS,
        "会话行级改写 avatar={} file={} ops={n}",
        backend::logx::redact(&avatar),
        backend::logx::redact(&file)
    );
    tauri::async_runtime::spawn_blocking(move || chat_apply_ops_to_file(&path, ops))
        .await
        .map_err(|e| format!("任务异常：{e}"))?
        .map_err(|e| {
            log::error!(target: backend::logx::FS, "会话行级改写失败 avatar={} file={} ops={n}: {e}", backend::logx::redact(&avatar), backend::logx::redact(&file));
            e
        })
}

fn chat_apply_ops_to_file(path: &Path, ops: Vec<ChatLineOp>) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("读取会话失败：{e}"))?;
    let lines = raw.lines().map(|s| s.to_string()).collect::<Vec<_>>();
    let lines = chat_ops_apply(lines, ops)?;
    chat_write_atomic(path, &lines)
}

fn chat_ops_apply(mut lines: Vec<String>, ops: Vec<ChatLineOp>) -> Result<Vec<String>, String> {
    for op in ops {
        match op {
            ChatLineOp::Replace { index, json } => {
                if index >= lines.len() {
                    return Err(format!("行号越界：{index}（共 {} 行）", lines.len()));
                }
                serde_json::from_str::<serde_json::Value>(&json)
                    .map_err(|e| format!("第 {index} 行不是合法 JSON：{e}"))?;
                lines[index] = json;
            }
            ChatLineOp::Delete { index } => {
                if index >= lines.len() {
                    return Err(format!("行号越界：{index}（共 {} 行）", lines.len()));
                }
                lines.remove(index);
            }
            ChatLineOp::Insert { index, json } => {
                serde_json::from_str::<serde_json::Value>(&json)
                    .map_err(|e| format!("插入内容不是合法 JSON：{e}"))?;
                let index = index.min(lines.len());
                lines.insert(index, json);
            }
        }
    }
    Ok(lines)
}

fn chat_write_atomic(path: &Path, lines: &[String]) -> Result<(), String> {
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, format!("{}\n", lines.join("\n")))
        .map_err(|e| format!("写入临时文件失败：{e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("替换会话文件失败：{e}"))
}

#[tauri::command]
async fn chat_export_file(
    state: tauri::State<'_, DataRootState>,
    avatar: String,
    file: String,
    dest: String,
) -> Result<(), String> {
    let src = chat_jsonl_path(&state.0, &avatar, &file)?;
    if !src.is_file() {
        log::warn!(target: backend::logx::FS, "导出会话失败：文件不存在 avatar={} file={}", backend::logx::redact(&avatar), backend::logx::redact(&file));
        return Err(format!("会话文件不存在：{}", src.display()));
    }
    let dest = PathBuf::from(dest.trim());
    log::info!(target: backend::logx::FS, "导出会话 avatar={} file={} -> {}", backend::logx::redact(&avatar), backend::logx::redact(&file), backend::logx::redact(&dest.display().to_string()));
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::copy(&src, &dest)
            .map(|_| ())
            .map_err(|e| format!("导出失败：{e}"))
    })
    .await
    .map_err(|e| format!("任务异常：{e}"))?
    .map_err(|e| {
        log::error!(target: backend::logx::FS, "导出会话失败: {e}");
        e
    })
}

#[tauri::command]
async fn character_export_png(
    state: tauri::State<'_, DataRootState>,
    avatar: String,
    dest: String,
) -> Result<(), String> {
    let src = character_png_path(&state.0, &avatar)?;
    if !src.is_file() {
        log::warn!(target: backend::logx::FS, "导出角色卡失败：文件不存在 avatar={}", backend::logx::redact(&avatar));
        return Err(format!("角色卡文件不存在：{}", src.display()));
    }
    let dest = PathBuf::from(dest.trim());
    log::info!(target: backend::logx::FS, "导出角色卡 avatar={} -> {}", backend::logx::redact(&avatar), backend::logx::redact(&dest.display().to_string()));
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::copy(&src, &dest)
            .map(|_| ())
            .map_err(|e| format!("导出失败：{e}"))
    })
    .await
    .map_err(|e| format!("任务异常：{e}"))?
    .map_err(|e| {
        log::error!(target: backend::logx::FS, "导出角色卡失败: {e}");
        e
    })
}

/* ================================================================== *
 * 数据袋（导入/导出）
 * ================================================================== */

#[tauri::command]
async fn st_data_export(
    state: tauri::State<'_, DataRootState>,
    dest: String,
    include_secrets: Option<bool>,
) -> Result<st_data::DataBagResult, String> {
    let root = state.0.clone();
    let dest = PathBuf::from(dest.trim());
    if dest.extension().map(|e| e != "zip").unwrap_or(true) {
        return Err("导出目标必须是 .zip 文件".into());
    }
    let cfg = root.join("config.yaml");
    let cfg_ref = cfg.is_file().then_some(cfg);
    let dest_log = backend::logx::redact(&dest.display().to_string());
    let with_secrets = include_secrets.unwrap_or(false);
    log::info!(target: backend::logx::FS, "导出数据袋 dest={dest_log} include_secrets={with_secrets}");
    tauri::async_runtime::spawn_blocking(move || {
        st_data::export_bag(&root, cfg_ref.as_deref(), &dest, include_secrets.unwrap_or(false))
    })
    .await
    .map_err(|e| format!("导出任务异常：{e}"))?
    .inspect(|r| {
        log::info!(target: backend::logx::FS, "导出数据袋完成 files={} bytes={}", r.files, r.bytes);
    })
    .map_err(|e| {
        log::error!(target: backend::logx::FS, "导出数据袋失败: {e}");
        e
    })
}

#[tauri::command]
async fn st_data_import(state: tauri::State<'_, DataRootState>, src: String) -> Result<st_data::DataBagResult, String> {
    let root = state.0.clone();
    let src = PathBuf::from(src.trim());
    if !src.is_file() {
        log::warn!(target: backend::logx::FS, "导入数据袋失败：文件不存在 {}", backend::logx::redact(&src.display().to_string()));
        return Err(format!("导入文件不存在：{}", src.display()));
    }
    let cfg = root.join("config.yaml");
    let cfg_ref = cfg.is_file().then_some(cfg);
    log::info!(target: backend::logx::FS, "导入数据袋 src={}", backend::logx::redact(&src.display().to_string()));
    tauri::async_runtime::spawn_blocking(move || {
        st_data::import_bag(&src, &root, cfg_ref.as_deref())
    })
    .await
    .map_err(|e| format!("导入任务异常：{e}"))?
    .inspect(|r| {
        log::info!(target: backend::logx::FS, "导入数据袋完成 files={} bytes={}", r.files, r.bytes);
    })
    .map_err(|e| {
        log::error!(target: backend::logx::FS, "导入数据袋失败: {e}");
        e
    })
}

#[tauri::command]
async fn st_sync_test(cfg: backend::sync::SyncConfig) -> Result<String, String> {
    backend::sync::test_connection(&cfg).await
}

#[tauri::command]
async fn st_sync_execute(
    state: tauri::State<'_, DataRootState>,
    cfg: backend::sync::SyncConfig,
) -> Result<backend::sync::SyncResult, String> {
    let root = state.0.clone();
    Ok(backend::sync::sync_push(&cfg, &root).await)
}

/* ================================================================== *
 * 入口
 * ================================================================== */

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_log::Builder::new()
                // 全量日志：stdout + appLogDir 滚动文件（2MB KeepOne，本地时区）
                .level(log_level())
                .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                .max_file_size(2_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            backend_base,
            chat_mutate_lines,
            chat_export_file,
            character_export_png,
            st_data_export,
            st_data_import,
            st_sync_test,
            st_sync_execute,
        ])
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let _ = std::fs::create_dir_all(&config_dir);
            // 旧数据根一次性采用（best-effort + 显式 log；失败不阻断启动）。
            // 调用点必须先于 create_dir_all(data_root)：新根一旦建出就不再采用。
            st_data::adopt_legacy_root(&config_dir);
            let data_root = st_data::external_data_root(&config_dir);
            let _ = std::fs::create_dir_all(&data_root);
            let _ = std::fs::create_dir_all(data_root.join("default-user"));
            app.manage(DataRootState(data_root.clone()));
            app.manage(BackendSlot(std::sync::Mutex::new(None)));

            log::info!(
                target: backend::logx::BOOT,
                "应用启动 config_dir={} data_root={} log_level={:?}",
                config_dir.display(),
                data_root.display(),
                log_level()
            );

            let handle = app.handle().clone();
            let h = tauri::async_runtime::block_on(backend::start(data_root.clone()))
                .map_err(|e| {
                    log::error!(target: backend::logx::BOOT, "native backend 启动失败: {e}");
                    format!("native backend 启动失败: {e}")
                })?;
            log::info!(target: backend::logx::BOOT, "native backend 就绪 127.0.0.1:{}", h.port);
            *handle.state::<BackendSlot>().0.lock().unwrap() = Some(h);

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_handle, _event| {});
}

/* ================================================================== *
 * T15 会话行级操作测试（文件面断言）
 * ================================================================== */

#[cfg(test)]
mod chat_line_tests {
    use super::*;
    use ChatLineOp::{Delete, Insert, Replace};

    fn lines_of(raw: &str) -> Vec<String> {
        raw.lines().map(|s| s.to_string()).collect()
    }

    fn tmp_dir(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("stoc-t15-{tag}-"))
            .tempdir()
            .expect("创建测试临时目录失败")
    }

    #[test]
    fn replace_out_of_bounds_is_rejected() {
        let err = chat_ops_apply(
            vec![r#"{"n":0}"#.into(), r#"{"n":1}"#.into()],
            vec![Replace {
                index: 2,
                json: r#"{"n":9}"#.into(),
            }],
        )
        .unwrap_err();
        assert!(err.contains("行号越界"), "{err}");
        assert!(err.contains("共 2 行"), "{err}");

        let err = chat_ops_apply(
            vec![r#"{"n":0}"#.into()],
            vec![Replace {
                index: 1,
                json: r#"{"n":9}"#.into(),
            }],
        )
        .unwrap_err();
        assert!(err.contains("行号越界"), "{err}");
    }

    #[test]
    fn insert_head_middle_tail() {
        let out = chat_ops_apply(
            vec![r#"{"a":1}"#.into(), r#"{"c":3}"#.into()],
            vec![
                Insert { index: 1, json: r#"{"b":2}"#.into() },
                Insert { index: 0, json: r#"{"h":0}"#.into() },
                Insert { index: 4, json: r#"{"t":4}"#.into() },
                Insert { index: 999, json: r#"{"x":5}"#.into() },
            ],
        )
        .unwrap();
        let keys: Vec<String> = out
            .iter()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).unwrap();
                v.as_object().unwrap().keys().next().unwrap().clone()
            })
            .collect();
        assert_eq!(
            keys,
            ["h", "a", "b", "c", "t", "x"].map(|s| s.to_string())
        );
    }

    #[test]
    fn delete_keeps_physical_line_numbers_matching_get_index() {
        let raw = "{\"h\":0}\n{\"a\":1}\n{\"b\":2}\n{\"c\":3}\n";
        let out = chat_ops_apply(lines_of(raw), vec![Delete { index: 1 }]).unwrap();
        assert_eq!(out.len(), 3, "物理行数应减 1");
        assert_eq!(out[0], "{\"h\":0}");
        assert_eq!(out[1], "{\"b\":2}");
        assert_eq!(out[2], "{\"c\":3}");
    }

    #[test]
    fn unknown_fields_survive_untouched_lines() {
        let raw = concat!(
            "{\"chat_metadata\":{}}\n",
            "{\"mes\":\"a\",\"swipes\":[\"x\",\"y\"],\"extra\":{\"k\":1}}\n",
            "{\"mes\":\"b\"}\n"
        );
        let out = chat_ops_apply(
            lines_of(raw),
            vec![Replace { index: 2, json: "{\"mes\":\"b2\"}".into() }],
        )
        .unwrap();
        assert_eq!(out[0], "{\"chat_metadata\":{}}");
        assert_eq!(
            out[1],
            "{\"mes\":\"a\",\"swipes\":[\"x\",\"y\"],\"extra\":{\"k\":1}}",
            "未触碰行的原始字节必须原样保留"
        );
        assert_eq!(out[2], "{\"mes\":\"b2\"}");
    }

    #[test]
    fn atomic_write_leaves_no_tmp_residue() {
        let tmp = tmp_dir("atomic");
        let dir = tmp.path().to_path_buf();
        let path = dir.join("chat.jsonl");
        chat_write_atomic(
            &path,
            &["{\"h\":0}".to_string(), "{\"a\":1}".to_string()],
        )
        .unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw, "{\"h\":0}\n{\"a\":1}\n");
        assert!(
            !path.with_extension("jsonl.tmp").exists(),
            "rename 后 .tmp 不许残留"
        );
    }

    #[test]
    fn chat_metadata_header_survives_line_ops() {
        let raw = "{\"chat_metadata\":{\"id\":\"01J\"}}\n{\"mes\":\"a\"}\n";
        let out = chat_ops_apply(
            lines_of(raw),
            vec![
                Replace { index: 1, json: "{\"mes\":\"b\"}".into() },
                Insert { index: 2, json: "{\"mes\":\"c\"}".into() },
            ],
        )
        .unwrap();
        assert_eq!(
            out[0],
            "{\"chat_metadata\":{\"id\":\"01J\"}}",
            "首行 header 不许被行级操作改写"
        );
        assert_eq!(out.last().unwrap(), "{\"mes\":\"c\"}");
    }

    #[test]
    fn aborted_batch_does_not_damage_jsonl() {
        let tmp = tmp_dir("abort");
        let dir = tmp.path().to_path_buf();
        let path = dir.join("chat.jsonl");
        let raw = "{\"chat_metadata\":{}}\n{\"mes\":\"行1\",\"指纹\":\"keep\"}\n";
        std::fs::write(&path, raw).unwrap();

        let err = chat_apply_ops_to_file(
            &path,
            vec![
                Replace { index: 0, json: "{\"chat_metadata\":{\"x\":1}}".into() },
                Replace { index: 1, json: "not-json{".into() },
            ],
        )
        .unwrap_err();
        assert!(err.contains("不是合法 JSON"), "{err}");

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, raw, "失败批不得损坏目标 JSONL");
        assert!(
            !path.with_extension("jsonl.tmp").exists(),
            "失败批不得留下 .tmp 半成品"
        );
    }
}