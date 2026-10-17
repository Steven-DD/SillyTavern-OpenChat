//! backend/vault —— T12 secrets vault（Phase 3 头项，方向已裁定：
//! **密钥类 → OS keychain 权威；配置类（url×4 + vertexai 服务账号）→ settings**）。
//!
//! 存储面（回退连续性，清单 T12 验收口径）：
//! - `secrets.json` **镜像保留**（ST 完整多值结构 id/value/label/active）——
//!   用户切回 st 模式时 ST 直接读它，密钥仍有效（Phase 6 去 ST 时清理，
//!   本期不清）；native 不在此之外产生新的明文密钥文件。
//! - keychain 权威：service=`SillyTavern-OpenChat`，account=`STOC:{key}:{id}`；
//!   读 = keychain 优先（镜像 active 条目定 id）→ 镜像 value 回落
//!   （keyring 不可用/headless 降级 —— 存档一次性 log，行为=ST 字面）。
//! - 配置类：settings `extension_settings.stchat_native.{key}` 权威 → 镜像回落；
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
                eprintln!("[vault] OS keychain 不可用（{e}）→ 降级为 secrets.json 镜像权威（行为=ST 字面）");
                Self {
                    store: Box::new(KeyringStore),
                    degraded: AtomicBool::new(true),
                }
            }
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

    /// 读 active 条目值（keychain 权威 + 镜像回落）
    pub fn read_active(&self, st: &BackendState, key: &str) -> Option<String> {
        if classify(key) == SecretClass::Settings {
            // 配置类：settings 优先
            if let Some(v) = super::settings::read_native_config(&st.data_root, key) {
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
        let entries = Self::mirror_entries(st, key);
        let active = entries.into_iter().find(|s| {
            s.get("active").and_then(|b| b.as_bool()).unwrap_or(false)
        })?;
        let id = active.get("id").and_then(|i| i.as_str()).unwrap_or("");
        let mirror_value = active.get("value").and_then(|v| v.as_str()).map(String::from);
        if classify(key) == SecretClass::KeyChain {
            if !self.is_degraded() {
                match self.store.get(&Self::account_for(key, id)) {
                    Ok(Some(v)) => return Some(v),
                    Ok(None) => {} // keychain 无条目（迁移前启动）→ 镜像
                    Err(_) => {
                        // keychain 故障 → 一次性降级标记 + 镜像
                        self.degraded.store(true, Ordering::Relaxed);
                        eprintln!("[vault] keychain 读取故障 → 降级为镜像权威（secrets.json）");
                    }
                }
            }
            mirror_value
        } else if classify(key) == SecretClass::Settings {
            mirror_value // settings 读空时回落镜像
        } else {
            mirror_value
        }
    }

    /// 写同步（secrets_write 调用）：镜像已写 id+value → vault 就位
    pub fn store_sync(&self, st: &BackendState, key: &str, id: &str, value: &str) {
        match classify(key) {
            SecretClass::KeyChain => {
                if self.is_degraded() {
                    // 降级模式：镜像即权威，无 keychain 动作
                    return;
                }
                if value.len() > 2500 {
                    // Windows CredMan 上限内（2560B 全条目）；超限 → 显式降级（镜像兜底）
                    eprintln!(
                        "[vault] keychain 条目超 2500B（key={key}）→ 该密钥仅落镜像（secrets.json），行为=ST 字面"
                    );
                    return;
                }
                if let Err(e) = self.store.set(&Self::account_for(key, id), value) {
                    eprintln!("[vault] keychain 写入失败（key={key}）：{e} → 降级为镜像权威");
                    self.degraded.store(true, Ordering::Relaxed);
                }
            }
            SecretClass::Settings => {
                super::settings::patch_native_config(&st.data_root, key, value);
            }
        }
    }

    /// 启动迁移（幂等）：镜像 → keychain / settings
    pub fn migrate_existing(&self, st: &BackendState) {
        let path = Self::mirror_path(st);
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { return };
        let Some(obj) = v.as_object() else { return };
        let entries: Vec<(String, Vec<Value>)> = obj
            .iter()
            .filter_map(|(k, a)| a.as_array().map(|a| (k.clone(), a.clone())))
            .collect();
        let (mut chain, mut cfg) = (0usize, 0usize);
        for (key, list) in entries {
            for entry in list {
                let Some(value) = entry.get("value").and_then(|x| x.as_str()) else { continue };
                let Some(id) = entry.get("id").and_then(|x| x.as_str()) else { continue };
                match classify(&key) {
                    SecretClass::KeyChain => {
                        if !self.is_degraded() && value.len() <= 2500 {
                            if let Ok(None) = self.store.get(&Self::account_for(&key, id)) {
                                let _ = self.store.set(&Self::account_for(&key, id), value);
                                chain += 1;
                            }
                        }
                    }
                    SecretClass::Settings => {
                        // 仅当 settings 面还没有该键才回填（以 settings 为权威后不往回写）
                        if super::settings::read_native_config(&st.data_root, &key).is_none() {
                            super::settings::patch_native_config(&st.data_root, &key, value);
                            cfg += 1;
                        }
                    }
                }
            }
        }
        if chain + cfg > 0 {
            println!("[vault] 原有密钥迁移：keychain {chain} 条、settings 配置 {cfg} 项（镜像 secrets.json 按清单保留）");
        }
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

    fn st_with(tag: &str, secrets: &Value) -> BackendState {
        let root = std::env::temp_dir().join(format!("stchat-vault-{tag}-{}", std::process::id()));
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
            vault: std::sync::Arc::new(Vault::new_memory()),
        }
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

    #[test]
    fn uuid_shape() {
        let id = uuid_v4();
        assert_eq!(id.len(), 36);
        assert_eq!(id.split('-').count(), 5);
    }
}
