//! 测试专用：带临时目录守卫的后端状态（`#[cfg(test)]`）
//!
//! 以前每个用例都把数据根建在 `%TEMP%/stoc-<case>-<pid>`，跑完谁也不删 —— 一轮
//! cargo test 就留约 80 个目录（现由 scripts/clean-test-artifacts.mjs 兜底）。
//! 这里把 `tempfile::TempDir` 守卫挂到返回值上：用例结束（含 panic unwind）自动删树，
//! 不再产生残骸。
//!
//! 用法（backend/*.rs 的 `mod tests` 内）：
//! ```ignore
//! let tmp = TestBackend::temp("stoc-chars-roundtrip-");   // 可识别前缀 + 随机后缀
//! let root = tmp.path().to_path_buf();
//! TestBackend::new(tmp, BackendState { data_root: root, .. })
//! ```
//! 返回值 `Deref` 到 `BackendState`，用例里 `&st` / `st.data_root` 的写法不用改。

use std::ops::Deref;

use super::BackendState;

pub(crate) struct TestBackend {
    /// 注意声明顺序：结构体按声明顺序 drop —— st 先释放，目录守卫后删树
    st: BackendState,
    /// 仅靠 Drop 生效（从不读取），故放行 dead_code
    #[allow(dead_code)]
    tmp: tempfile::TempDir,
}

impl TestBackend {
    /// 建临时目录：`prefix` 只用于排障识别（如 `stoc-vault-sync-`），随机后缀保证唯一
    pub fn temp(prefix: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(prefix)
            .tempdir()
            .expect("创建测试临时目录失败")
    }

    /// 把目录守卫与后端状态绑在一起，随返回值一起销毁
    pub fn new(tmp: tempfile::TempDir, st: BackendState) -> Self {
        Self { st, tmp }
    }
}

impl Deref for TestBackend {
    type Target = BackendState;

    fn deref(&self) -> &BackendState {
        &self.st
    }
}
