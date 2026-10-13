/**
 * 统一日志门面：操作审计 + 调试分级。
 *
 * - Tauri 壳内：走 @tauri-apps/plugin-log（console + 滚动文件 appLogDir，
 *   Rust 侧 tauri_plugin_log Builder 已注册，单文件 2MB KeepOne 轮转）
 * - 浏览器 dev（无 Tauri）：console + 内存环形缓冲（最近 800 条，
 *   window.__stExportLogs() 可导出文本，调试用）
 *
 * 使用：`logOp('chat.send', '开始生成', { model })` 记录操作审计；
 * `log.error(...)` 记录错误。埋点原则：会话生命周期、生成生命周期、
 * 数据删除/导入导出、巡检清理、全局错误。
 */
type Level = 'debug' | 'info' | 'warn' | 'error'

interface LogEntry {
  t: number
  level: Level
  scope: string
  msg: string
  detail?: unknown
}

const RING_MAX = 800
const ring: LogEntry[] = []

/** 是否 Tauri 壳（与 plugins.ts 同款判定：v2 注入 __TAURI_INTERNALS__） */
function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

/** 插件模块懒加载（浏览器 dev 下动态 import 也不应触发 IPC） */
async function pluginLog() {
  return await import('@tauri-apps/plugin-log')
}

function fmt(detail: unknown): string {
  if (detail === undefined) return ''
  if (detail instanceof Error) return ` :: ${detail.name}: ${detail.message}`
  if (typeof detail === 'string') return ` :: ${detail}`
  try {
    return ` :: ${JSON.stringify(detail)}`
  } catch {
    return ' :: [unserializable]'
  }
}

function emit(level: Level, scope: string, msg: string, detail?: unknown): void {
  const line = `[${scope}] ${msg}${fmt(detail)}`
  ring.push({ t: Date.now(), level, scope, msg, detail })
  if (ring.length > RING_MAX) ring.shift()

  if (inTauri()) {
    void pluginLog()
      .then((m) => {
        const fn =
          level === 'debug' ? m.debug : level === 'warn' ? m.warn : level === 'error' ? m.error : m.info
        void fn(line)
      })
      .catch(() => {
        console[level === 'debug' ? 'debug' : level === 'warn' ? 'warn' : level === 'error' ? 'error' : 'info'](line)
      })
  } else {
    const fn =
      level === 'debug' ? console.debug : level === 'warn' ? console.warn : level === 'error' ? console.error : console.info
    fn(line)
  }
}

export const log = {
  debug: (scope: string, msg: string, detail?: unknown) => emit('debug', scope, msg, detail),
  info: (scope: string, msg: string, detail?: unknown) => emit('info', scope, msg, detail),
  warn: (scope: string, msg: string, detail?: unknown) => emit('warn', scope, msg, detail),
  error: (scope: string, msg: string, detail?: unknown) => emit('error', scope, msg, detail),
  /** 操作审计（info 级，语义化埋点） */
  op: (action: string, msg: string, detail?: unknown) => emit('info', action, msg, detail),
  /** 导出内存缓冲（浏览器 dev 调试用） */
  exportText(): string {
    return ring
      .map((e) => `${new Date(e.t).toISOString()} [${e.level.toUpperCase()}] [${e.scope}] ${e.msg}${fmt(e.detail)}`)
      .join('\n')
  },
}

/** 安装全局错误捕获：Vue errorHandler + window error/unhandledrejection（在 main.ts 调用） */
export function installGlobalErrorHook(app: import('vue').App): void {
  app.config.errorHandler = (err, _instance, info) => {
    log.error('vue', `渲染错误（${info}）`, err)
  }
  window.addEventListener('error', (e) => {
    log.error('window', e.message, `${e.filename}:${e.lineno}`)
  })
  window.addEventListener('unhandledrejection', (e) => {
    log.error('promise', e.reason instanceof Error ? e.reason.message : String(e.reason))
  })
  log.info('log', '日志系统就绪', inTauri() ? 'tauri-plugin-log（文件+console）' : '浏览器 dev（console+内存）')
}
