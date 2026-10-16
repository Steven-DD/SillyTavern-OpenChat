/**
 * 脚本库运行时（酒馆助手 TavernHelper_scripts / tavern_helper.scripts 语义）：
 *
 * 角色卡内启用的脚本在会话打开时执行——每个脚本一个隐藏 sandbox iframe
 * （与消息前端界面共用 SANDBOX_BOOTSTRAP shim），脚本经 postMessage 桥
 * 调用宿主 API（embedBridge.handleEmbedCall）并可订阅宿主事件
 * （MESSAGE_SENT / MESSAGE_RECEIVED / MESSAGE_EDITED / CHAT_CHANGED /
 * GENERATION_ENDED，由 chat store 在对应时机 broadcastHostEvent 广播）。
 *
 * 生命周期：openSession 时 stop→start（切卡即换脚本集）。
 */
import { SANDBOX_BOOTSTRAP, buildEmbedSrcdoc } from './embed'
import { handleEmbedCall } from './embedBridge'
import {
  broadcastHostEvent,
  pruneEventTargets,
  registerEventTarget,
  unregisterEventTarget,
} from './eventBus'
import { log } from '@/services/log'

interface ScriptRuntime {
  avatar: string
  frames: HTMLIFrameElement[]
}

let runtime: ScriptRuntime | null = null
let hostListenerInstalled = false

/** 共享宿主消息监听：按 ev.source 分发（API 调用/事件订阅/错误上报） */
function onHostMessage(ev: MessageEvent): void {
  const d = ev.data as { source?: string; type?: string } | null
  if (!d || d.source !== 'tavern-embed') return

  if (d.type === 'th-event-on') {
    const { id, name } = d as { id: number; name: string }
    if (typeof id === 'number' && ev.source) registerEventTarget(ev.source as Window, id, name)
    return
  }
  if (d.type === 'th-event-off') {
    const { id } = d as { id: number }
    if (typeof id === 'number') unregisterEventTarget(id)
    return
  }
  if (d.type === 'th-call') {
    const { id, api, args } = d as { id: number; api: string; args: unknown }
    const respond = (payload: Record<string, unknown>): void => {
      try {
        ;(ev.source as Window).postMessage(
          { source: 'tavern-host', type: 'th-result', id, ...payload },
          '*',
        )
      } catch {
        /* iframe 销毁竞态 */
      }
    }
    Promise.resolve()
      .then(() => handleEmbedCall(api, args))
      .then((result) => respond({ result }))
      .catch((e: unknown) => respond({ error: e instanceof Error ? e.message : String(e) }))
    return
  }
  if (d.type === 'th-error') {
    const { msg, where } = d as { msg?: string; where?: string }
    log.warn('scriptHost', '脚本错误：' + (msg ?? '未知') + '（' + (where ?? '') + '）')
  }
}

function ensureHostListener(): void {
  if (hostListenerInstalled) return
  window.addEventListener('message', onHostMessage)
  hostListenerInstalled = true
}

/** 从角色卡扩展字段提取脚本清单（新旧两种存储键），只保留启用项 */
export function extractCardScripts(ext: unknown): { name: string; content: string }[] {
  const e = ext as
    | {
        TavernHelper_scripts?: unknown
        tavern_helper?: { scripts?: unknown }
      }
    | undefined
  const read = (raw: unknown): { name: string; content: string }[] => {
    if (!Array.isArray(raw)) return []
    return raw
      .filter((x): x is { name?: unknown; content?: unknown; enabled?: unknown } => !!x && typeof x === 'object')
      .filter((x) => x.enabled !== false && typeof x.content === 'string' && x.content.trim())
      .map((x) => ({ name: String(x.name ?? '未命名脚本'), content: x.content as string }))
  }
  return [...read(e?.TavernHelper_scripts), ...read(e?.tavern_helper?.scripts)]
}

/** 启动/替换脚本运行时（openSession 时调用；无启用脚本则清空） */
export function startScriptRuntime(avatar: string, scripts: { name: string; content: string }[]): void {
  stopScriptRuntime()
  if (!scripts.length) return
  ensureHostListener()

  const frames: HTMLIFrameElement[] = []
  for (const sc of scripts) {
    const iframe = document.createElement('iframe')
    iframe.className = 'st-script-runtime'
    iframe.setAttribute('sandbox', 'allow-scripts')
    iframe.style.display = 'none'
    iframe.dataset.scriptName = sc.name
    // 脚本内容是裸 JS：包进 <script>，闭合标签转义防提前截断
    const safe = sc.content.replace(/<\/script/gi, '<\\/script')
    const boot =
      SANDBOX_BOOTSTRAP +
      '\n;(async function () {\n  try {\n' +
      safe +
      '\n  } catch (e) { parent.postMessage({ source: "tavern-embed", type: "th-error", msg: String(e && e.message ? e.message : e), where: "script:' +
      sc.name +
      '" }, "*") }\n})()'
    iframe.srcdoc = buildEmbedSrcdoc('<!DOCTYPE html><html><head><meta charset="utf-8"></head><body></body></html>', boot)
    document.body.appendChild(iframe)
    frames.push(iframe)
  }
  runtime = { avatar, frames }
  log.op('scriptHost.start', '脚本库运行时已启动', { avatar, scripts: scripts.map((s) => s.name) })
}

/** 停止运行时（会话切换/关闭） */
export function stopScriptRuntime(): void {
  if (!runtime) return
  for (const f of runtime.frames) f.remove()
  const alive = new Set(runtime.frames.map((f) => f.contentWindow).filter(Boolean)) as Set<Window>
  pruneEventTargets(alive)
  log.op('scriptHost.stop', '脚本库运行时已停止', { avatar: runtime.avatar })
  runtime = null
}

/** 当前会话切换/消息变化时的宿主事件广播入口 */
export function emitScriptHostEvent(name: string, payload?: unknown): void {
  broadcastHostEvent(name, payload)
}

// dev 构建暴露脚本测试钩子（验证脚本库运行时，不写入任何卡数据）
if (import.meta.env.DEV) {
  ;(window as unknown as Record<string, unknown>).__stStartScriptDev = (code: string): void => {
    startScriptRuntime('__dev__', [{ name: '__dev_test__', content: code }])
  }
  ;(window as unknown as Record<string, unknown>).__stStopScriptDev = (): void => {
    stopScriptRuntime()
  }
}
