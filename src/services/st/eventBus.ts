/**
 * SillyTavern-OpenChat 生命周期事件总线 (Lifecycle Event Bus)
 *
 * 1. 宿主事件注册表：脚本库/前端界面 iframe 中 eventOn 的订阅集中管理，由 scriptHost 分发
 * 2. 强类型应用生命周期总线：为 QuickReplies、STscript 触发器与组件提供事件与流水线拦截能力
 */

/* ================================================================== *
 * 1. iframe 宿主事件注册表（TavernHelper 兼容）
 * ================================================================== */

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

/* ================================================================== *
 * 2. 应用级生命周期事件总线 (Lifecycle Event Bus)
 * ================================================================== */

export interface EventPayloads {
  /** 发送用户消息前（可修改内容或设置 abort: true 拦截发送） */
  before_message_send: {
    text: string
    image?: string
    characterName?: string
    abort?: boolean
  }
  /** 接收到 AI 消息并完成落盘后 */
  after_message_receive: {
    text: string
    characterName?: string
    isStreaming?: boolean
  }
  /** 切换/加载角色卡完成 */
  on_character_loaded: {
    avatar: string
    name: string
  }
  /** 组装 Prompt 完毕、即将向后端提交生成前 */
  before_generation: {
    characterName?: string
    promptLength: number
  }
  /** 会话切换 */
  on_session_changed: {
    avatar: string
    file: string
  }
}

export type HookEventName = keyof EventPayloads
export type EventHandler<T = unknown> = (payload: T) => Promise<T | void> | T | void

class EventBus {
  private listeners: Map<HookEventName, Set<EventHandler<any>>> = new Map()

  /** 订阅事件 */
  on<E extends HookEventName>(event: E, handler: EventHandler<EventPayloads[E]>): () => void {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set())
    }
    this.listeners.get(event)!.add(handler)
    return () => this.off(event, handler)
  }

  /** 取消订阅 */
  off<E extends HookEventName>(event: E, handler: EventHandler<EventPayloads[E]>): void {
    const set = this.listeners.get(event)
    if (set) {
      set.delete(handler)
    }
  }

  /** 并发广播通知 */
  async emit<E extends HookEventName>(event: E, payload: EventPayloads[E]): Promise<void> {
    const set = this.listeners.get(event)
    if (!set || set.size === 0) return
    const promises: Promise<unknown>[] = []
    for (const h of set) {
      try {
        const res = h(payload)
        if (res instanceof Promise) promises.push(res)
      } catch (err) {
        console.error(`[EventBus] Error in handler for event "${event}":`, err)
      }
    }
    if (promises.length) {
      await Promise.allSettled(promises)
    }
  }

  /** 串行流水线调度（后续处理函数接收前置函数处理后的 payload，支持就地修改或拦截） */
  async pipeline<E extends HookEventName>(event: E, initialPayload: EventPayloads[E]): Promise<EventPayloads[E]> {
    const set = this.listeners.get(event)
    if (!set || set.size === 0) return initialPayload
    let current = { ...initialPayload }
    for (const h of set) {
      try {
        const res = await h(current)
        if (res && typeof res === 'object') {
          current = { ...current, ...res }
        }
      } catch (err) {
        console.error(`[EventBus] Error in pipeline handler for event "${event}":`, err)
      }
    }
    return current
  }

  /** 清空指定或全部监听器 */
  clear(event?: HookEventName): void {
    if (event) {
      this.listeners.delete(event)
    } else {
      this.listeners.clear()
    }
  }
}

export const eventBus = new EventBus()
