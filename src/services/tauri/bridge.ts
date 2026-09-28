/**
 * Tauri 桌面端桥接（生产环境适配）
 *
 * 桌面运行时需要两件事（浏览器开发环境下全部跳过，行为不变）：
 *  1. 从 Rust 中继拿基地址 + 鉴权 token → 写入 `localStorage['st.base'/'st.relayToken']`
 *  2. 订阅 `st-status` 事件，实时反映 SillyTavern sidecar 的启动状态
 *
 * 用动态 import 引 `@tauri-apps/api`：浏览器里没有 Tauri 全局对象时，
 * 连模块都不会被求值，避免开发环境报错或白白打包体积。
 */
import { reactive } from 'vue'
import { setStBase, setRelayToken } from '@/services/st/client'

export interface SidecarStatus {
  state: 'starting' | 'ready' | 'error' | 'stopped' | string
  message: string
  port: number
  pid?: number | null
  st_dir: string
  node: string
  /** true = 由本应用拉起（退出时会被清理）；false = 外部已在运行的实例 */
  managed: boolean
}

export const sidecar = reactive({
  /** 是否运行在 Tauri 桌面壳里 */
  isDesktop: false,
  /** SillyTavern 已就绪 */
  ready: false,
  /** 中继基地址（浏览器开发环境为空） */
  relayBase: '',
  /** 最近一次状态推送 */
  status: null as SidecarStatus | null,
  /** 桥接失败原因（仅调试用） */
  error: '',
})

function hasTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

/** 在 app.mount 之前调用 */
export async function initDesktopBridge(): Promise<void> {
  if (!hasTauri()) return
  sidecar.isDesktop = true
  try {
    const { invoke } = await import('@tauri-apps/api/core')

    // 中继端点 = 基地址 + 一次性鉴权 token（中继代持 ST 会话 Cookie，token 是准入凭证）
    const ep = await invoke<{ base: string; token: string }>('st_relay_base')
    if (ep?.base) {
      setStBase(ep.base)
      sidecar.relayBase = ep.base
    }
    if (ep?.token) setRelayToken(ep.token)

    const st = await invoke<SidecarStatus | null>('st_status')
    if (st) {
      sidecar.status = st
      sidecar.ready = st.state === 'ready'
    }

    const { listen } = await import('@tauri-apps/api/event')
    await listen<SidecarStatus>('st-status', (e) => {
      sidecar.status = e.payload
      sidecar.ready = e.payload.state === 'ready'
    })
  } catch (e) {
    sidecar.error = e instanceof Error ? e.message : String(e)
    console.warn('[bridge] Tauri 桥接初始化失败:', sidecar.error)
  }
}
