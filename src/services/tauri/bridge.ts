/**
 * Tauri 桌面端桥接（生产环境适配）
 *
 * 桌面运行时需要从 Rust native backend 拿基地址 + 鉴权 token → 写入
 * `localStorage['st.base'/'st.relayToken']`。浏览器开发环境全部跳过。
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

/** 在 app.mount 之前调用 */
export async function initDesktopBridge(): Promise<void> {
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