//! backend/vault —— T12 secrets vault（Phase 3 头项，方向已裁定：
//! **密钥类 → OS keychain 权威；配置类（url×4 + vertexai 服务账号）→ settings**）。
//!
//! ── 密钥落盘策略（2026-10-19 裁定：keychain 为主、镜像兜底，明文只留降级面）──
//! - 正常路径（KeyChain 类且 keyring 健康且 value<=2500B；或 Settings 类）：
//!   明文**只进** keychain / settings；`secrets.json` 仍写并保持 ST 结构形状
//!   （id/value/label/active 四字段都在），但该条目 value 置空串，另加
//!   `backend="keychain"|"settings"` 标记 —— 镜像不再是第二份明文。
//! - 降级路径（keyring 不可用 / 写失败 / value>2500B）：镜像保留明文 value，
//!   标 `backend="mirror"`，沿用 degraded 标记 + 显式 log（不得静默丢密钥）。
//! - 读取顺序不变：keychain/settings 权威优先 → 镜像 value 兜底（仅在权威
//!   缺失/故障时）；**空串按无值处理**（返回 None）。
//! - 启动迁移（migrate_existing）：回填 + **洗白** —— 带明文的旧镜像条目回填
//!   权威副本成功后，该条目 value 置空并标 backend，重写 `secrets.json`；
//!   历史明文在启动后从磁盘消失（任务 A 验收核心）。
//! - 写入顺序铁律：**权威副本先写、镜像后写**（见 tokenizers_secrets::secrets_write）。
//! - keychain 权威：service=`SillyTavern-OpenChat`，account=`STOC:{key}:{id}`；
//!   镜像 active 条目定 id。
//! - 配置类：settings `extension_settings.stoc_native.{key}` 权威 → 镜像回落；
//!   写入时同步（服务账号大 JSON 避开 Windows CredMan 2560 字节条目上限）。
//!
//! 多值结构（secrets.js:204-223/588-640）：push + `_deactivateAllSecrets`
//! （写端在 tokenizers_secrets::secrets_write），镜像负责 ST 兼容形态；
//! App 契约只消费 write/read（ST 的 /rotate//rename//delete 无前端调用面 →
//! 不在本后端开放，S20 未登记项）。
//!
//! Windows CredMan 单条 2560 字节上限：超限 set 失败 → 降级镜像（显式可判定，
//! 不得静默丢密钥）。

use super::BackendState;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) const KEYCHAIN_SERVICE: &str = "SillyTavern-OpenChat";
const ACCOUNT_PREFIX: &str = "STOC";

/// 镜像条目 backend 字段取值（ST 四字段结构上的外加标记字段）
pub(crate) const BACKEND_KEYCHAIN: &str = "keychain";
pub(crate) const BACKEND_SETTINGS: &str = "settings";
pub(crate) const BACKEND_MIRROR: &str = "mirror";

/// keychain 单条上限（Windows CredMan 2560B 全条目，留余量取 2500）
pub(crate) const KEYCHAIN_VALUE_MAX: usize = 2500;

/* ---------------- 分类（SECRET_KEYS 全量映射，secrets.js:9-73） ---------------- */

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecretClass {
    /// OS keychain 权威
    KeyChain,
    /// settings 权威（含 vertexai 服务账号 —— 大 JSON 避开 CredMan 上限）
    Settings,
}

/// 配置类四 URL + 服务账号（清单 T12 字面）
pub const SETTINGS_KEYS: [&str; 5] = [
    "libre_url",
    "lingva_url",
    "oneringtranslator_url",
    "deeplx_url",
    "vertexai_service_account_json",
];

pub fn classify(key: &str) -> SecretClass {
    if SETTINGS_KEYS.contains(&key) {
        SecretClass::Settings
    } else {
        SecretClass::KeyChain
    }
}

/* ---------------- store 抽象 ---------------- */

pub trait SecretStore: Send + Sync {
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn del(&self, account: &str) -> Result<(), String>;
}

/// 生产实现：OS keychain（keyring crate：Windows CredMan / macOS Keychain /
/// Linux secret service）
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account).map_err(|e| e.to_string())?;
        entry.set_password(value).map_err(|e| e.to_string())
    }
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn del(&self, account: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

impl KeyringStore {
    /// 可用性探针（headless/CI 上 keychain 可能不存在 → 降级）
    pub fn probe() -> Result<(), String> {
        let s = KeyringStore;
        let probe_acct = format!("{ACCOUNT_PREFIX}:probe");
        s.set(&probe_acct, "probe")?;
        let got = s.get(&probe_acct)?;
        s.del(&probe_acct)?;
        match got.as_deref() {
            Some("probe") => Ok(()),
            other => Err(format!("keychain 探针读回不符：{other:?}")),
        }
    }
}

/// 测试实现：内存表
pub struct MemoryStore(std::sync::Mutex<HashMap<String, String>>);

impl MemoryStore {
    pub fn new() -> Self {
        Self(std::sync::Mutex::new(HashMap::new()))
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for MemoryStore {
    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(account.into(), value.into());
        Ok(())
    }
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn del(&self, account: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}

/// 权威写入结果（决定镜像条目是否保留明文）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StoreOutcome {
    /// 权威副本已就位；镜像 value 置空，backend 写此标签
    Authoritative(&'static str),
    /// 降级 / 超限：只有镜像持明文（backend="mirror"），不得静默丢密钥
    MirrorOnly,
}

impl StoreOutcome {
    /// 镜像条目 backend 字段值
    pub fn backend_label(self) -> &'static str {
        match self {
            StoreOutcome::Authoritative(label) => label,
            StoreOutcome::MirrorOnly => BACKEND_MIRROR,
        }
    }

    pub fn is_authoritative(self) -> bool {
        matches!(self, StoreOutcome::Authoritative(_))
    }
}

/// 测试用：set/get/del 全失败（注入必失败 store 的降级路径）
#[cfg(test)]
pub struct FailingStore;

#[cfg(test)]
impl SecretStore for FailingStore {
    fn set(&self, _account: &str, _value: &str) -> Result<(), String> {
        Err("injected store failure（测试注入）".into())
    }
    fn get(&self, _account: &str) -> Result<Option<String>, String> {
        Err("injected store failure（测试注入）".into())
    }
    fn del(&self, _account: &str) -> Result<(), String> {
        Err("injected store failure（测试注入）".into())
    }
}

/* ---------------- Vault 门面 ---------------- */

pub struct Vault {
    store: Box<dyn SecretStore>,
    /// keychain 可用性（降级时=false：读全走镜像，显式闭环）
    degraded: AtomicBool,
}

impl Vault {
    /// 生产：keychain 探针通过才启用；失败 → 降级镜像模式（log 一次）
    pub fn new_keyring() -> Self {
        match KeyringStore::probe() {
            Ok(()) => Self {
                store: Box::new(KeyringStore),
                degraded: AtomicBool::new(false),
            },
            Err(e) => {
                log::warn!(target: super::logx::VAULT, "OS keychain 不可用（{e}）→ 降级为 secrets.json 镜像权威（密钥仍可用，仅安全性降级）");
                Self {
                    store: Box::new(KeyringStore),
                    degraded: AtomicBool::new(true),
                }
            }
        }
    }

    /// 测试：注入必失败 store（模拟 keyring 不可用 / 写入失败 → 降级路径）
    #[cfg(test)]
    pub fn new_failing() -> Self {
        Self {
            store: Box::new(FailingStore),
            degraded: AtomicBool::new(false),
        }
    }

    /// 测试：内存制品
    pub fn new_memory() -> Self {
        Self {
            store: Box::new(MemoryStore::new()),
            degraded: AtomicBool::new(false),
        }
    }

    pub fn is_degraded(&self) -> bool {
        self.degraded.load(Ordering::Relaxed)
    }

    fn account_for(key: &str, id: &str) -> String {
        format!("{ACCOUNT_PREFIX}:{key}:{id}")
    }

    /// 镜像 secrets.json 路径（ST 布局）
    fn mirror_path(st: &BackendState) -> std::path::PathBuf {
        st.data_root.join("default-user").join("secrets.json")
    }

    /// 读镜像 key 的全部条目
    fn mirror_entries(st: &BackendState, key: &str) -> Vec<Value> {
        let Ok(text) = std::fs::read_to_string(Self::mirror_path(st)) else {
            return Vec::new();
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            return Vec::new();
        };
        v.get(key).and_then(|a| a.as_array()).cloned().unwrap_or_default()
    }

    /// 读 active 条目值（keychain/settings 权威优先 + 镜像回落；空串按无值）
    pub fn read_active(&self, st: &BackendState, key: &str) -> Option<String> {
        if classify(key) == SecretClass::Settings {
            // 配置类：settings 优先（read_native_config 已把空串当未配置）
            if let Some(v) = super::settings::read_native_config(&st.data_root, key) {
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
        let entries = Self::mirror_entries(st, key);
        let active = entries
            .into_iter()
            .find(|s| s.get("active").and_then(|b| b.as_bool()).unwrap_or(false))?;
        let id = active.get("id").and_then(|i| i.as_str()).unwrap_or("");
        let mirror_value = active
            .get("value")
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty()) // 洗白后的空串 = 无值，不得当密钥返回
            .map(String::from);
        if classify(key) == SecretClass::KeyChain && !self.is_degraded() {
            match self.store.get(&Self::account_for(key, id)) {
                Ok(Some(v)) if !v.is_empty() => return Some(v),
                Ok(Some(_)) => {} // 权威副本是空串 → 按无值，继续镜像兜底
                Ok(None) => {} // keychain 无条目（迁移前启动）→ 镜像
                Err(_) => {
                    // keychain 故障 → 一次性降级标记 + 镜像
                    self.degraded.store(true, Ordering::Relaxed);
                    log::warn!(target: super::logx::VAULT, "keychain 读取故障（key={key}）→ 降级为镜像兜底（secrets.json）");
                }
            }
        }
        // Settings/未知类读空回落镜像；KeyChain 读空/无条目/故障同样回落镜像
        mirror_value
    }

    /// 写权威副本（secrets_write 调用；**先于镜像写** —— 写入顺序铁律）。
    /// 返回结果决定镜像条目是否保留明文：Authoritative → 镜像 value 置空；
    /// MirrorOnly → 镜像保留明文（降级/超限，显式 log，不得静默丢密钥）。
    pub fn store_sync(&self, st: &BackendState, key: &str, id: &str, value: &str) -> StoreOutcome {
        match classify(key) {
            SecretClass::KeyChain => {
                if self.is_degraded() {
                    // 降级模式：镜像即权威，无 keychain 动作（明文留镜像）
                    return StoreOutcome::MirrorOnly;
                }
                if value.is_empty() {
                    // 空串按无值：无需写 keychain（镜像 value 同样为空）
                    return StoreOutcome::Authoritative(BACKEND_KEYCHAIN);
                }
                if value.len() > KEYCHAIN_VALUE_MAX {
                    // Windows CredMan 上限内（2560B 全条目）；超限 → 该条仅镜像（明文）
                    log::warn!(target: super::logx::VAULT, "keychain 条目超 {KEYCHAIN_VALUE_MAX}B（key={key}）→ 该密钥仅落镜像（secrets.json，backend=mirror）");
                    return StoreOutcome::MirrorOnly;
                }
                match self.store.set(&Self::account_for(key, id), value) {
                    Ok(()) => StoreOutcome::Authoritative(BACKEND_KEYCHAIN),
                    Err(e) => {
                        log::error!(target: super::logx::VAULT, "keychain 写入失败（key={key}）：{e} → 降级为镜像兜底（明文留 secrets.json）");
                        self.degraded.store(true, Ordering::Relaxed);
                        StoreOutcome::MirrorOnly
                    }
                }
            }
            SecretClass::Settings => {
                if super::settings::patch_native_config(&st.data_root, key, value) {
                    StoreOutcome::Authoritative(BACKEND_SETTINGS)
                } else {
                    log::error!(target: super::logx::VAULT, "settings 写入失败（key={key}）→ 降级为镜像兜底（明文留 secrets.json）");
                    StoreOutcome::MirrorOnly
                }
            }
        }
    }

    /// 启动迁移（幂等）：回填 + **洗白** —— 带明文的旧镜像条目回填
    /// keychain/settings 成功后，该条目 value 置空并标 backend，重写 secrets.json
    /// （历史明文在启动后从磁盘消失）。降级/超限条目保留明文并标
    /// backend="mirror"（显式 log，不静默丢）。
    pub fn migrate_existing(&self, st: &BackendState) {
        // 任务 B：extension_settings 下旧命名空间 stchat_* → stoc_*（一次性幂等）
        super::settings::migrate_stchat_namespace(&st.data_root);

        let path = Self::mirror_path(st);
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let Ok(mut v) = serde_json::from_str::<Value>(&text) else { return };
        let Some(obj) = v.as_object_mut() else { return };

        let (mut chain, mut cfg, mut scrubbed, mut mirror_kept) = (0usize, 0usize, 0usize, 0usize);
        let mut changed = false;
        for (key, arr) in obj.iter_mut() {
            let Some(list) = arr.as_array_mut() else { continue };
            let class = classify(key);
            for entry in list.iter_mut() {
                let Some(o) = entry.as_object_mut() else { continue };
                let value = o
                    .get("value")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if value.is_empty() {
                    continue; // 已洗白 / 本就无明文：不动
                }
                let Some(id) = o.get("id").and_then(|x| x.as_str()).map(String::from) else {
                    continue;
                };
                let outcome = match class {
                    SecretClass::KeyChain => {
                        if self.is_degraded() {
                            log::warn!(target: super::logx::VAULT, "迁移遇降级模式（key={key}）→ 明文保留在镜像（backend=mirror）");
                            StoreOutcome::MirrorOnly
                        } else if value.len() > KEYCHAIN_VALUE_MAX {
                            log::warn!(target: super::logx::VAULT, "迁移条目超 {KEYCHAIN_VALUE_MAX}B（key={key}）→ 明文保留在镜像（backend=mirror）");
                            StoreOutcome::MirrorOnly
                        } else {
                            let acct = Self::account_for(key, &id);
                            match self.store.get(&acct) {
                                // 权威已有非空值：直接洗白镜像（keychain 优先，不回写覆盖）
                                Ok(Some(existing)) if !existing.is_empty() => {
                                    StoreOutcome::Authoritative(BACKEND_KEYCHAIN)
                                }
                                _ => match self.store.set(&acct, &value) {
                                    Ok(()) => {
                                        chain += 1;
                                        StoreOutcome::Authoritative(BACKEND_KEYCHAIN)
                                    }
                                    Err(e) => {
                                        log::error!(target: super::logx::VAULT, "迁移 keychain 写入失败（key={key}）：{e} → 明文保留在镜像");
                                        self.degraded.store(true, Ordering::Relaxed);
                                        StoreOutcome::MirrorOnly
                                    }
                                },
                            }
                        }
                    }
                    SecretClass::Settings => {
                        // settings 已有该键 = 已是权威 → 直接洗白；否则回填
                        let already =
                            super::settings::read_native_config(&st.data_root, key).is_some();
                        if already
                            || super::settings::patch_native_config(&st.data_root, key, &value)
                        {
                            if !already {
                                cfg += 1;
                            }
                            StoreOutcome::Authoritative(BACKEND_SETTINGS)
                        } else {
                            log::error!(target: super::logx::VAULT, "迁移 settings 写入失败（key={key}）→ 明文保留在镜像");
                            StoreOutcome::MirrorOnly
                        }
                    }
                };

                match outcome {
                    StoreOutcome::Authoritative(label) => {
                        o.insert("value".into(), serde_json::json!("")); // 洗白
                        o.insert("backend".into(), serde_json::json!(label));
                        scrubbed += 1;
                        changed = true;
                    }
                    StoreOutcome::MirrorOnly => {
                        mirror_kept += 1;
                        if o.get("backend").and_then(|b| b.as_str()) != Some(BACKEND_MIRROR) {
                            o.insert("backend".into(), serde_json::json!(BACKEND_MIRROR));
                            changed = true;
                        }
                    }
                }
            }
        }

        if !changed {
            return;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let pretty = serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string());
        let tmp = path.with_extension("json.tmp");
        if let Err(e) = std::fs::write(&tmp, pretty.as_bytes()).and_then(|_| std::fs::rename(&tmp, &path)) {
            let _ = std::fs::remove_file(&tmp);
            log::error!(target: super::logx::VAULT, "镜像洗白写回失败：{e}（明文仍在 secrets.json，下次启动重试）");
            return;
        }
        log::info!(
            target: super::logx::VAULT,
            "原有密钥迁移：keychain {chain} 条、settings 配置 {cfg} 项；镜像洗白 {scrubbed} 条（backend=keychain/settings），明文保留 {mirror_kept} 条（backend=mirror）"
        );
    }
}

/* ---------------- helpers ---------------- */

/// uuid v4（tokenizers_secrets 共用；getrandom 拼 8-4-4-4-12）
pub(crate) fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("OS CSPRNG 失败");
    b[6] = (b[6] & 0x0F) | 0x40;
    b[8] = (b[8] & 0x3F) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn st_with_vault(tag: &str, secrets: &Value, vault: Vault) -> BackendState {
        let root = std::env::temp_dir().join(format!("stoc-vault-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("default-user");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("secrets.json"),
            serde_json::to_string_pretty(secrets).unwrap(),
        )
        .unwrap();
        BackendState {
            token: std::sync::Arc::new("t".into()),
            data_root: root,
            max_body_bytes: 1024,
            settings_svc: std::sync::Arc::new(super::super::settings::SettingsService::new()),
            chats_svc: std::sync::Arc::new(super::super::chats::ChatsService::new()),
            maid_tokens: std::sync::Arc::new(super::super::extras::MaidTokens::new()),
            vault: std::sync::Arc::new(vault),
        }
    }

    fn st_with(tag: &str, secrets: &Value) -> BackendState {
        st_with_vault(tag, secrets, Vault::new_memory())
    }

    fn seal_path(st: &BackendState) -> std::path::PathBuf {
        st.data_root.join("default-user").join("secrets.json")
    }

    #[test]
    fn classification_matches_checklist() {
        // 密钥类 → keychain
        assert_eq!(classify("api_key_claude"), SecretClass::KeyChain);
        assert_eq!(classify("api_key_openai"), SecretClass::KeyChain);
        // 配置类四 URL + 服务账号 → settings
        assert_eq!(classify("libre_url"), SecretClass::Settings);
        assert_eq!(classify("lingva_url"), SecretClass::Settings);
        assert_eq!(classify("oneringtranslator_url"), SecretClass::Settings);
        assert_eq!(classify("deeplx_url"), SecretClass::Settings);
        assert_eq!(classify("vertexai_service_account_json"), SecretClass::Settings);
    }

    #[test]
    fn chain_read_prefers_store_and_falls_back_to_mirror() {
        let st = st_with(
            "chainread",
            &json!({ "api_key_openai": [
                { "id": "a1", "value": "mirror-old", "label": "L", "active": false },
                { "id": "a2", "value": "mirror-active", "label": "L", "active": true },
            ] }),
        );
        // 未镜像进 keychain → 镜像值
        assert_eq!(st.vault.read_active(&st, "api_key_openai").as_deref(), Some("mirror-active"));
        // keychain 有权威值 → 覆盖镜像
        st.vault
            .store
            .set(&Vault::account_for("api_key_openai", "a2"), "chain-authoritative")
            .unwrap();
        assert_eq!(
            st.vault.read_active(&st, "api_key_openai").as_deref(),
            Some("chain-authoritative")
        );
    }

    #[test]
    fn settings_class_reads_settings_first() {
        let st = st_with(
            "cfgread",
            &json!({ "libre_url": [{ "id": "i1", "value": "http://127.0.0.1:7550", "label": "L", "active": true }] }),
        );
        // 未迁移 → 镜像
        assert_eq!(st.vault.read_active(&st, "libre_url").as_deref(), Some("http://127.0.0.1:7550"));
        // 同步进 settings → settings 权威
        super::super::settings::patch_native_config(&st.data_root, "libre_url", "http://127.0.0.1:9999");
        assert_eq!(
            st.vault.read_active(&st, "libre_url").as_deref(),
            Some("http://127.0.0.1:9999")
        );
    }

    #[test]
    fn migrate_imports_chain_and_settings_items() {
        let st = st_with(
            "migrate",
            &json!({
                "api_key_openai": [
                    { "id": "a1", "value": "sk-secret", "label": "L", "active": true },
                    { "id": "a2", "value": "old", "label": "L", "active": false },
                ],
                "deeplx_url": [
                    { "id": "u1", "value": "http://127.0.0.1:1188/translate", "label": "L", "active": true },
                ],
                "_migrated": [{ "id": "z", "value": "m", "label": "L", "active": true }],
            }),
        );
        st.vault.migrate_existing(&st);
        // keychain 面有 a1/a2 两个条目（active 与否都迁）
        assert_eq!(
            st.vault.store.get(&Vault::account_for("api_key_openai", "a1")).unwrap().as_deref(),
            Some("sk-secret")
        );
        assert_eq!(
            st.vault.store.get(&Vault::account_for("api_key_openai", "a2")).unwrap().as_deref(),
            Some("old")
        );
        // settings 面有 deeplx_url
        assert_eq!(
            super::super::settings::read_native_config(&st.data_root, "deeplx_url").as_deref(),
            Some("http://127.0.0.1:1188/translate")
        );
        // 幂等：重迁不重复（无崩溃 即通过）
        st.vault.migrate_existing(&st);
    }

    #[test]
    fn store_sync_routes_by_class() {
        let st = st_with("sync", &json!({}));
        st.vault.store_sync(&st, "api_key_custom", "x1", "sk-custom");
        assert_eq!(
            st.vault.store.get(&Vault::account_for("api_key_custom", "x1")).unwrap().as_deref(),
            Some("sk-custom")
        );
        st.vault.store_sync(&st, "lingva_url", "u1", "https://lingva.example");
        assert_eq!(
            super::super::settings::read_native_config(&st.data_root, "lingva_url").as_deref(),
            Some("https://lingva.example")
        );
    }

    /* ---------- 落盘策略：权威先写、镜像洗白 / 兜底 ---------- */

    #[test]
    fn store_sync_outcome_decides_mirror_plaintext() {
        let st = st_with("syncout", &json!({}));
        assert_eq!(
            st.vault.store_sync(&st, "api_key_custom", "x1", "sk-custom"),
            StoreOutcome::Authoritative(BACKEND_KEYCHAIN)
        );
        assert!(!st.vault.is_degraded(), "内存 store 健康 → 不降级");
        assert_eq!(
            st.vault.store_sync(&st, "lingva_url", "u1", "https://lingva.example"),
            StoreOutcome::Authoritative(BACKEND_SETTINGS)
        );
        // 空串按无值：视为权威无值，不写 keychain
        assert_eq!(
            st.vault.store_sync(&st, "api_key_openai", "e1", ""),
            StoreOutcome::Authoritative(BACKEND_KEYCHAIN)
        );
        // 超限：权威未就位 → 仅镜像（明文保留）
        let big = "k".repeat(KEYCHAIN_VALUE_MAX + 1);
        assert_eq!(
            st.vault.store_sync(&st, "api_key_big", "b1", &big),
            StoreOutcome::MirrorOnly
        );
        assert_eq!(StoreOutcome::MirrorOnly.backend_label(), BACKEND_MIRROR);
        assert_eq!(StoreOutcome::Authoritative(BACKEND_KEYCHAIN).backend_label(), "keychain");
    }

    #[test]
    fn empty_mirror_value_is_treated_as_absent() {
        let st = st_with(
            "emptyval",
            &json!({ "api_key_openai": [
                { "id": "e1", "value": "", "label": "L", "active": true, "backend": "keychain" }
            ] }),
        );
        assert_eq!(
            st.vault.read_active(&st, "api_key_openai"),
            None,
            "洗白后的空串不是密钥"
        );
    }

    #[test]
    fn oversize_mirror_value_is_readable() {
        let big = "k".repeat(KEYCHAIN_VALUE_MAX + 1);
        let st = st_with(
            "oversize",
            &json!({ "api_key_big": [
                { "id": "b1", "value": big, "label": "L", "active": true, "backend": "mirror" }
            ] }),
        );
        assert_eq!(
            st.vault.read_active(&st, "api_key_big").as_deref(),
            Some(big.as_str())
        );
    }

    /// ④ 洗白：预置带明文旧 secrets.json → migrate 后文件内不再出现该明文串
    #[test]
    fn migrate_backfills_and_scrubs_plaintext_from_disk() {
        let st = st_with(
            "scrub",
            &json!({
                "api_key_openai": [
                    { "id": "a1", "value": "sk-plain-1", "label": "L", "active": true },
                ],
                "deeplx_url": [
                    { "id": "u1", "value": "http://127.0.0.1:1188/translate", "label": "L", "active": true },
                ],
            }),
        );
        let path = seal_path(&st);
        st.vault.migrate_existing(&st);

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("sk-plain-1"), "历史明文必须从磁盘消失：{raw}");
        assert!(
            !raw.contains("http://127.0.0.1:1188/translate"),
            "settings 类明文同样洗白：{raw}"
        );
        let disk: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(disk["api_key_openai"][0]["value"], "");
        assert_eq!(disk["api_key_openai"][0]["backend"], "keychain");
        assert_eq!(disk["deeplx_url"][0]["value"], "");
        assert_eq!(disk["deeplx_url"][0]["backend"], "settings");
        // 权威副本就位（读得回）
        assert_eq!(
            st.vault.read_active(&st, "api_key_openai").as_deref(),
            Some("sk-plain-1")
        );
        assert_eq!(
            st.vault.read_active(&st, "deeplx_url").as_deref(),
            Some("http://127.0.0.1:1188/translate")
        );
        // 幂等：第二次迁移零写入
        st.vault.migrate_existing(&st);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            raw,
            "洗白后重迁不得再改文件"
        );
    }

    #[test]
    fn migrate_keeps_plaintext_when_store_fails() {
        let st = st_with_vault(
            "scrub-degraded",
            &json!({ "api_key_openai": [
                { "id": "a1", "value": "sk-keep", "label": "L", "active": true }
            ] }),
            Vault::new_failing(),
        );
        st.vault.migrate_existing(&st);
        let raw = std::fs::read_to_string(seal_path(&st)).unwrap();
        assert!(raw.contains("sk-keep"), "降级迁移不得丢明文：{raw}");
        let disk: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(disk["api_key_openai"][0]["backend"], "mirror");
        assert_eq!(
            st.vault.read_active(&st, "api_key_openai").as_deref(),
            Some("sk-keep")
        );
    }

    #[test]
    fn uuid_shape() {
        let id = uuid_v4();
        assert_eq!(id.len(), 36);
        assert_eq!(id.split('-').count(), 5);
    }
}
