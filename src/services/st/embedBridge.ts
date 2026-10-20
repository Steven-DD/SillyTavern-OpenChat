/**
 * 前端界面 iframe 沙箱的宿主侧 API 桥（TavernHelper 兼容层的宿主半边）。
 *
 * iframe 内卡脚本调用 shim（window.TavernHelper / window.SillyTavern / 同名全局函数）→
 * postMessage 转到这里 → 映射到 App 现有会话引擎。只暴露白名单 API：
 * 沙箱无 allow-same-origin，卡脚本无法绕过桥直连 ST 后端。
 */
import { useChatStore } from '@/stores/chat'
import { broadcastHostEvent } from './eventBus'

/** 酒馆助手 getChatMessages 返回的楼层子集 */
interface ThChatMessage {
  message_id: number
  name: string
  role: 'user' | 'assistant' | 'system'
  is_user: boolean
  is_system: boolean
  message: string
}

/** 楼层区间解析：'all' | 'latest' | 数字 | 'N' | '-N'(最近N) | 'N-' | 'N-M' | [a,b]（0 起闭区间） */
function resolveRange(
  range: unknown,
  total: number,
): { start: number; end: number } {
  if (range === undefined || range === null || range === 'all') return { start: 0, end: total - 1 }
  if (range === 'latest') return { start: total - 1, end: total - 1 }
  if (typeof range === 'number') return { start: range, end: range }
  if (Array.isArray(range) && range.length === 2) {
    return { start: Number(range[0]) || 0, end: Number(range[1]) || total - 1 }
  }
  if (typeof range === 'string') {
    const s = range.trim()
    if (/^-?\d+$/.test(s)) {
      const n = Number(s)
      if (n < 0) return { start: Math.max(0, total + n), end: total - 1 } // 最近 N 条
      return { start: n, end: n }
    }
    const m = /^(-?\d*)\s*-\s*(-?\d*)$/.exec(s)
    if (m) {
      const start = m[1] === '' || m[1] === '-' ? 0 : Number(m[1])
      const end = m[2] === '' ? total - 1 : Number(m[2])
      return { start: start < 0 ? Math.max(0, total + start) : start, end }
    }
  }
  return { start: 0, end: total - 1 }
}

/** getChatMessages：楼层读出（酒馆助手字段子集） */
function getChatMessages(range: unknown): ThChatMessage[] {
  const chat = useChatStore()
  const msgs = chat.messages
  if (!msgs.length) return []
  const { start, end } = resolveRange(range, msgs.length)
  const out: ThChatMessage[] = []
  for (let i = Math.max(0, start); i <= Math.min(end, msgs.length - 1); i++) {
    const m = msgs[i]!
    if (m.error) continue
    out.push({
      message_id: i,
      name: m.name,
      role: m.role,
      is_user: m.role === 'user',
      is_system: m.role === 'assistant' && (m.extra?.is_system as boolean | undefined) === true,
      message: m.content,
    })
  }
  return out
}

/** getVariables：会话级变量（chat_metadata.variables，App 宏存储同源） */
function getVariables(): Record<string, unknown> {
  return { ...useChatStore().chatVariables }
}

/** getVariable：获取单个会话变量 */
function getVariable(args: unknown): unknown {
  const { key } = (args ?? {}) as { key?: unknown }
  if (typeof key !== 'string') return undefined
  return useChatStore().chatVariables[key]
}

/** setVariable：单键改写变量并即时排队落盘 */
function setVariable(args: unknown): true {
  const { key, value } = (args ?? {}) as { key?: unknown; value?: unknown }
  if (typeof key !== 'string') throw new Error('setVariable 需要字符串 key')
  const chat = useChatStore()
  chat.chatVariables[key] = value
  chat.scheduleChatVarFlush()
  return true
}

/** getContext：获取当前会话上下文（角色名、会话文件、用户名、全量变量等） */
function getContext(): Record<string, unknown> {
  const chat = useChatStore()
  return {
    characterId: chat.currentAvatar,
    chatId: chat.currentFile,
    name1: chat.userName,
    name2: chat.currentCharacter?.name ?? '',
    characterName: chat.currentCharacter?.name ?? '',
    groupId: chat.group?.id,
    variables: { ...chat.chatVariables },
  }
}

/** getStorage：读取持久化的卡内 localStorage 字典 */
function getStorage(): Record<string, string> {
  const chat = useChatStore()
  const raw = chat.chatVariables.__local_storage__
  if (raw && typeof raw === 'object') return { ...(raw as Record<string, string>) }
  return {}
}

/** setStorage：卡内 localStorage 变动持久化同步 */
function setStorage(args: unknown): true {
  const { key, value } = (args ?? {}) as { key?: unknown; value?: unknown }
  if (typeof key !== 'string') throw new Error('setStorage 需要 key')
  const chat = useChatStore()
  const store = ((chat.chatVariables.__local_storage__ as Record<string, string>) ?? {})
  if (value === null || value === undefined) {
    delete store[key]
  } else {
    store[key] = String(value)
  }
  chat.chatVariables.__local_storage__ = store
  chat.scheduleChatVarFlush()
  return true
}

/** setChatMessages：按楼层改写消息（含落盘），供脚本更新状态栏/正文 */
function setChatMessages(args: unknown): true {
  const { message_id, message } = (args ?? {}) as { message_id?: unknown; message?: unknown }
  if (typeof message_id !== 'number' || typeof message !== 'string') {
    throw new Error('setChatMessages 需要 { message_id: number, message: string }')
  }
  void useChatStore().editMessage(message_id, message)
  return true
}

/** eventEmit：脚本间通信（广播回所有订阅帧） */
function eventEmit(args: unknown): true {
  const { name, payload } = (args ?? {}) as { name?: unknown; payload?: unknown }
  if (typeof name !== 'string') throw new Error('eventEmit 需要 name')
  broadcastHostEvent(name, payload)
  return true
}

/** replaceVariables：整体替换会话变量 */
function replaceVariables(value: unknown): true {
  const chat = useChatStore()
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('replaceVariables 需要对象参数')
  }
  chat.replaceChatVariables(value as Record<string, unknown>)
  return true
}

/** insertVariables：浅合并进会话变量 */
function insertVariables(value: unknown): true {
  const chat = useChatStore()
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('insertVariables 需要对象参数')
  }
  chat.insertChatVariables(value as Record<string, unknown>)
  return true
}

/**
 * triggerSlash：执行 STscript。
 * 卡里典型用法 `/send xxx|/trigger`（发送并生成）→ 直接走 chat.send
 * （发送 + 生成为一体）；其余命令串交给脚本引擎。
 */
async function triggerSlash(cmds: unknown): Promise<void> {
  const chat = useChatStore()
  const script = typeof cmds === 'string' ? cmds : Array.isArray(cmds) ? cmds.join('\n') : ''
  if (!script.trim()) return
  // /send … | /trigger 组合：等价于发送一条用户消息（send 内含生成）
  const sendTrigger = /^\/send\s+([\s\S]*?)\s*\|\s*\/trigger\s*$/i.exec(script.trim())
  if (sendTrigger) {
    await chat.send(sendTrigger[1]!)
    return
  }
  await chat.runUserScript(script)
}

/** 桥 API 分发表（白名单：不在表内的调用一律拒绝） */
const API_TABLE: Record<string, (args: unknown) => unknown> = {
  getChatMessages: (args) => getChatMessages((args as { range?: unknown })?.range),
  setChatMessages: (args) => setChatMessages(args),
  getVariables: () => getVariables(),
  getVariable: (args) => getVariable(args),
  setVariable: (args) => setVariable(args),
  getContext: () => getContext(),
  getStorage: () => getStorage(),
  setStorage: (args) => setStorage(args),
  replaceVariables: (args) =>
    replaceVariables((args as { value?: unknown })?.value ?? (args as unknown)),
  insertVariables: (args) =>
    insertVariables((args as { value?: unknown })?.value ?? (args as unknown)),
  triggerSlash: (args) => triggerSlash((args as { cmds?: unknown })?.cmds ?? (args as unknown)),
  eventEmit: (args) => eventEmit(args),
}

/** iframe → 宿主的调用分发（embedBridge 宿主半边入口） */
export async function handleEmbedCall(api: string, rawArgs: unknown): Promise<unknown> {
  const fn = API_TABLE[api]
  if (!fn) throw new Error(`未支持的 API：${api}`)
  return await fn(rawArgs)
}
