/**
 * 宿主事件注册表：脚本库/前端界面 iframe 中 eventOn 的订阅集中管理，
 * 由 scriptHost 在消息桥上分发订阅/退订，chat store 在生命周期时机广播。
 * （独立模块：避免 embedBridge ↔ scriptHost 循环依赖）
 */

const eventTargets = new Map<number, { win: Window; name: string }>()

/** iframe 内脚本订阅宿主事件（th-event-on，由 scriptHost 分发） */
export function registerEventTarget(win: Window, id: number, name: string): void {
  eventTargets.set(id, { win, name })
}

/** iframe 内脚本退订（th-event-off） */
export function unregisterEventTarget(id: number): void {
  eventTargets.delete(id)
}

/** 清理已销毁 iframe 的订阅（stopScriptRuntime 时按存活帧过滤） */
export function pruneEventTargets(alive: Set<Window>): void {
  for (const [id, t] of [...eventTargets.entries()]) {
    if (!alive.has(t.win)) eventTargets.delete(id)
  }
}

/** 广播宿主事件给所有订阅了该事件的 iframe 监听器 */
export function broadcastHostEvent(name: string, payload?: unknown): void {
  for (const { win, name: sub } of eventTargets.values()) {
    if (sub !== name) continue
    try {
      win.postMessage({ source: 'tavern-host', type: 'th-event', name, payload }, '*')
    } catch {
      /* iframe 已销毁（会话切换竞态） */
    }
  }
}
