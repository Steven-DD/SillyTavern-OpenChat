/**
 * Tauri 桌面端桥接（生产环境适配）
 *
 * 桌面运行时需要从 Rust native backend 拿基地址 + 鉴权 token → 写入
 * `localStorage['stoc.base']` / `sessionStorage['stoc.apiToken']`。
 * 浏览器开发环境跳过桥接本身；旧键 'st.*' → 'stoc.*' 的一次性迁移仍会执行。
 */
import { reactive } from 'vue'
import { setStBase, setApiToken } from '@/services/st/client'

export interface BackendStatus {
  state: 'ready' | string
  message: string
  port: number
}

export const desktop = reactive({
  /** 是否运行在 Tauri 桌面壳里 */
  isDesktop: false,
  /** native backend 已就绪 */
  ready: true,
  /** 中继/后端基地址（浏览器开发环境为空） */
  relayBase: '',
  /** 最近一次状态推送 */
  status: null as BackendStatus | null,
  /** 桥接失败原因（仅调试用） */
  error: '',
})

function hasTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

/** 旧存储键 → 新键（一次性迁移；'st.relayToken' 已废弃，不读取也不迁移，仅清理残留） */
const LEGACY_STORAGE_KEYS: [string, string][] = [
  ['st.base', 'stoc.base'],
  ['st.apiToken', 'stoc.apiToken'],
  ['st.source', 'stoc.source'],
  ['st.model', 'stoc.model'],
]
const DROPPED_STORAGE_KEYS = ['st.relayToken']

/**
 * 把旧键值搬到新键（桥接写入前执行，浏览器/桌面都会跑一次）。
 * 新键已存在（用户已在新版本写过）→ 以新键为准，只清旧键；两处存储各自独立处理。
 */
function migrateLegacyStorageKeys(): void {
  for (const [oldKey, newKey] of LEGACY_STORAGE_KEYS) {
    for (const store of [localStorage, sessionStorage]) {
      const v = store.getItem(oldKey)
      if (v === null) continue
      if (store.getItem(newKey) === null) store.setItem(newKey, v)
      store.removeItem(oldKey)
    }
  }
  for (const key of DROPPED_STORAGE_KEYS) {
    localStorage.removeItem(key)
    sessionStorage.removeItem(key)
  }
}

/** 在 app.mount 之前调用 */
export async function initDesktopBridge(): Promise<void> {
  migrateLegacyStorageKeys()
  if (!hasTauri()) return
  desktop.isDesktop = true
  try {
    const { invoke } = await import('@tauri-apps/api/core')

    const ep = await invoke<{ base: string; token: string }>('backend_base')
    if (ep?.base) {
      setStBase(ep.base)
      desktop.relayBase = ep.base
    }
    if (ep?.token) setApiToken(ep.token)
    desktop.ready = true
    desktop.status = { state: 'ready', message: 'native backend 已就绪', port: 0 }
  } catch (e) {
    desktop.error = e instanceof Error ? e.message : String(e)
    console.warn('[bridge] Tauri 桥接初始化失败:', desktop.error)
  }
}