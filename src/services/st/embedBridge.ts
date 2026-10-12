/**
 * 前端界面 iframe 沙箱的宿主侧 API 桥（TavernHelper 兼容层的宿主半边）。
 *
 * iframe 内卡脚本调用 shim（window.TavernHelper / 同名全局函数）→
 * postMessage 转到这里 → 映射到 App 现有会话引擎。只暴露白名单 API：
 * 沙箱无 allow-same-origin，卡脚本无法绕过桥直连 ST 后端。
 */
import { useChatStore } from '@/stores/chat'

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
  getVariables: () => getVariables(),
  replaceVariables: (args) =>
    replaceVariables((args as { value?: unknown })?.value ?? (args as unknown)),
  insertVariables: (args) =>
    insertVariables((args as { value?: unknown })?.value ?? (args as unknown)),
  triggerSlash: (args) => triggerSlash((args as { cmds?: unknown })?.cmds ?? (args as unknown)),
}

/** iframe → 宿主的调用分发（embedBridge 宿主半边入口） */
export async function handleEmbedCall(api: string, rawArgs: unknown): Promise<unknown> {
  const fn = API_TABLE[api]
  if (!fn) throw new Error(`未支持的 API：${api}`)
  return await fn(rawArgs)
}
