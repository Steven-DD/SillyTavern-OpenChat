/**
 * 宏引擎 v2（约 55 个宏）
 *
 * ── 语法 ──
 * {{name}}、{{name::arg1::arg2}}、{{name:arg}}（random/pick/roll/datetimeformat 兼容冒号）
 * {{// 注释}}、块级 {{if cond}}…{{else}}…{{/if}}。大小写不敏感。
 * 支持嵌套（内层宏先展开，最多迭代 8 轮防死循环）。
 *
 * ── 有意为之的差异 ──
 * - 变量存储：chat 级直接读写 chat_metadata.variables
 * - {{trim}} 非作用域用法返回空串（ST 返回标记做后处理正则，效果近似）
 * - {{pick}} 种子中的 offset 用「文本内出现序号」而非字符偏移（其余种子成分与网页端一致，
 *   PRNG 同为 seedrandom，同会话同文本结果稳定）
 * - {{outlet}}/{{hasExtension}}/{{group}} 系列、instruct 序列宏在 App 无对应概念，返回空串
 */

import seedrandom from 'seedrandom'

/**
 * ST utils.js 的 getStringHash（逐位一致移植）——{{pick}} 种子与网页端同值的前提。
 */
export function getStringHash(str: string, seed = 0): number {
  if (typeof str !== 'string') return 0
  let h1 = 0xdeadbeef ^ seed
  let h2 = 0x41c6ce57 ^ seed
  for (let i = 0, ch; i < str.length; i++) {
    ch = str.charCodeAt(i)
    h1 = Math.imul(h1 ^ ch, 2654435761)
    h2 = Math.imul(h2 ^ ch, 1597334677)
  }
  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507) ^ Math.imul(h2 ^ (h2 >>> 13), 3266489909)
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507) ^ Math.imul(h1 ^ (h1 >>> 13), 3266489909)
  return 4294967296 * (2097151 & h2) + (h1 >>> 0)
}

/** 每次 runMacros 调用的 pick 求值环境（同步展开，模块级暂存） */
let pickRunEnv: { contentHash: number; ordinal: number } | null = null

/* ------------------------------------------------------------------ *
 * 上下文与变量存储
 * ------------------------------------------------------------------ */

export interface MacroVariables {
  get(name: string): unknown
  set(name: string, value: unknown): void
  has(name: string): boolean
  delete(name: string): void
}

export interface MacroContext {
  charName: string
  userName: string
  /** 生效人设描述（锁定或默认人设） */
  persona: string
  charDescription: string
  charPersonality: string
  charScenario: string
  charPrompt: string
  charInstruction: string
  charDepthPrompt: string
  charCreatorNotes: string
  charFirstMessage: string
  charVersion: string
  mesExamples: string
  model: string
  maxContext: number
  maxResponse: number
  /** 聊天历史（旧→新，仅正文本消息） */
  history: { role: 'user' | 'assistant'; content: string }[]
  /** 最后一条用户消息的发送时间戳（{{idleDuration}} 用） */
  lastUserSendDate?: number
  swipeId: number
  lastGenerationType: string
  /** {{banned}} 收集（供 Text Completion 透传，当前仅记录） */
  bannedWords: string[]
  chatVars: MacroVariables
  globalVars: MacroVariables
  /** 会话哈希（getStringHash(chatId)，{{pick}} 种子之一；来自 chat_metadata.chat_id_hash 或会话键） */
  chatIdHash?: number
  /** /reroll-pick 重置种子（chat_metadata.pick_reroll_seed） */
  pickRerollSeed?: number | null
}

/** localStorage JSON 变量存储 */
function localVarStore(storageKey: string): MacroVariables {
  const load = (): Record<string, unknown> => {
    try {
      const v = JSON.parse(localStorage.getItem(storageKey) ?? '{}')
      return v && typeof v === 'object' ? (v as Record<string, unknown>) : {}
    } catch {
      return {}
    }
  }
  return {
    get(name) {
      return load()[name]
    },
    set(name, value) {
      const all = load()
      all[name] = value
      localStorage.setItem(storageKey, JSON.stringify(all))
    },
    has(name) {
      return name in load()
    },
    delete(name) {
      const all = load()
      delete all[name]
      localStorage.setItem(storageKey, JSON.stringify(all))
    },
  }
}

/** 会话级变量（localStorage 存储）。
 *  ⚠ 生产主链路已不再使用：prompt 组装经 `BuildPromptOptions.macro.chatVars` 注入
 *  chat_metadata.variables 的 store。本函数保留供自检脚本使用。 */
export function getChatVarStore(sessionKey: string): MacroVariables {
  return localVarStore(`app.chat.vars::${sessionKey}`)
}
/** 全局变量（App 侧 localStorage） */
export function getGlobalVarStore(): MacroVariables {
  return localVarStore('app.vars.global')
}

/** 内存变量存储（无 localStorage 环境 / 无会话键时的瞬态存储） */
export function memoryVarStore(): MacroVariables {
  const m = new Map<string, unknown>()
  return {
    get: (name) => m.get(name),
    set: (name, value) => void m.set(name, value),
    has: (name) => m.has(name),
    delete: (name) => void m.delete(name),
  }
}
/** 应用级瞬态共享存储（无会话键时 setvar 不丢失） */
const transientChatVars = memoryVarStore()
const transientGlobalVars = memoryVarStore()

/** 构造上下文（缺省值兜底） */
export function emptyContext(partial: Partial<MacroContext> = {}): MacroContext {
  return {
    charName: '',
    userName: 'User',
    persona: '',
    charDescription: '',
    charPersonality: '',
    charScenario: '',
    charPrompt: '',
    charInstruction: '',
    charDepthPrompt: '',
    charCreatorNotes: '',
    charFirstMessage: '',
    charVersion: '',
    mesExamples: '',
    model: '',
    maxContext: 8192,
    maxResponse: 2048,
    history: [],
    swipeId: 0,
    lastGenerationType: 'normal',
    bannedWords: [],
    chatVars: transientChatVars,
    globalVars: transientGlobalVars,
    ...partial,
  }
}

/* ------------------------------------------------------------------ *
 * Handler 注册表
 * ------------------------------------------------------------------ */

interface MacroArgs {
  /** :: 分隔的未命名参数（内层宏已展开） */
  unnamed: string[]
  /** 原始参数串（random/pick 需要按逗号切分） */
  raw: string
  ctx: MacroContext
}

type MacroHandler = (args: MacroArgs) => string

const registry = new Map<string, MacroHandler>()

function reg(name: string, handler: MacroHandler, aliases: string[] = []): void {
  registry.set(name.toLowerCase(), handler)
  for (const a of aliases) registry.set(a.toLowerCase(), handler)
}

const s = (v: unknown): string => (v === undefined || v === null ? '' : String(v))

/* ---- core ---- */
reg('space', ({ unnamed: [c] }) => ' '.repeat(Math.max(0, Number(c) || 1)))
reg('newline', ({ unnamed: [c] }) => '\n'.repeat(Math.max(0, Number(c) || 1)))
reg('noop', () => '')
reg('trim', ({ unnamed: [c] }) => s(c).trim())
reg('reverse', ({ unnamed: [v] }) => Array.from(s(v)).reverse().join(''))
reg('//', () => '')
reg('comment', () => '')
reg('input', ({ ctx }) => lastOf(ctx, 'user'))
reg('maxPrompt', ({ ctx }) => String(ctx.maxContext), ['maxprompttokens'])
reg('maxContext', ({ ctx }) => String(ctx.maxContext), ['maxcontexttokens'])
reg('maxResponse', ({ ctx }) => String(ctx.maxResponse), ['maxresponsetokens'])
reg('banned', ({ unnamed: [w], ctx }) => {
  ctx.bannedWords.push(s(w).replace(/^"|"$/g, ''))
  return ''
})
reg('outlet', () => '')
reg('hasExtension', () => '')
reg('lastGenerationType', ({ ctx }) => ctx.lastGenerationType)

/* ---- env ---- */
reg('user', ({ ctx }) => ctx.userName)
reg('char', ({ ctx }) => ctx.charName)
reg('group', () => '')
reg('groupNotMuted', () => '')
reg('notChar', () => '')
reg('charIfNotGroup', () => '')
reg('charPrompt', ({ ctx }) => ctx.charPrompt)
reg('charInstruction', ({ ctx }) => ctx.charInstruction)
reg('charDescription', ({ ctx }) => ctx.charDescription, ['description'])
reg('charPersonality', ({ ctx }) => ctx.charPersonality, ['personality'])
reg('charScenario', ({ ctx }) => ctx.charScenario, ['scenario'])
reg('persona', ({ ctx }) => ctx.persona)
reg('mesExamplesRaw', ({ ctx }) => ctx.mesExamples)
reg('mesExamples', ({ ctx }) => ctx.mesExamples)
reg('charDepthPrompt', ({ ctx }) => ctx.charDepthPrompt)
reg('charCreatorNotes', ({ ctx }) => ctx.charCreatorNotes, ['creatornotes'])
reg('charFirstMessage', ({ ctx }) => ctx.charFirstMessage, ['greeting'])
reg('charVersion', ({ ctx }) => ctx.charVersion)
reg('model', ({ ctx }) => ctx.model)
reg('original', () => '')
reg('isMobile', () => 'false')

/* ---- chat ---- */
const lastOf = (ctx: MacroContext, role?: 'user' | 'assistant'): string => {
  const h = ctx.history
  if (!h.length) return ''
  if (!role) return h[h.length - 1]!.content
  for (let i = h.length - 1; i >= 0; i--) {
    const m = h[i]!
    if (m.role === role) return m.content
  }
  return ''
}
reg('lastMessage', ({ ctx }) => lastOf(ctx))
reg('lastMessageId', ({ ctx }) => String(Math.max(0, ctx.history.length - 1)))
reg('lastUserMessage', ({ ctx }) => lastOf(ctx, 'user'))
reg('lastCharMessage', ({ ctx }) => lastOf(ctx, 'assistant'))
reg('firstIncludedMessageId', () => '0')
reg('firstDisplayedMessageId', () => '0')
reg('lastSwipeId', ({ ctx }) => String(ctx.swipeId + 1))
reg('currentSwipeId', ({ ctx }) => String(ctx.swipeId + 1))
reg('allChatRange', () => '')

/* ---- time（moment 子集，中文区域语义） ---- */
const p2 = (n: number): string => String(n).padStart(2, '0')
const WEEK_ZH = ['星期日', '星期一', '星期二', '星期三', '星期四', '星期五', '星期六']

function applyOffset(d: Date, offset?: string): Date {
  const m = /^([+-]?\d+)([smhd])$/i.exec(s(offset).trim())
  if (!m) return d
  const n = Number(m[1])
  const ms = { s: 1000, m: 60_000, h: 3_600_000, d: 86_400_000 }[m[2]!.toLowerCase()] ?? 0
  return new Date(d.getTime() + n * ms)
}

function momentLike(d: Date, fmt: string): string {
  // moment 常用 token 子集：YYYY MM DD HH mm ss SSS d
  return fmt.replace(/YYYY|MM|DD|HH|mm|ss|SSS|d/g, (t) => {
    switch (t) {
      case 'YYYY': return String(d.getFullYear())
      case 'MM': return p2(d.getMonth() + 1)
      case 'DD': return p2(d.getDate())
      case 'HH': return p2(d.getHours())
      case 'mm': return p2(d.getMinutes())
      case 'ss': return p2(d.getSeconds())
      case 'SSS': return String(d.getMilliseconds()).padStart(3, '0')
      case 'd': return String(d.getDay())
      default: return t
    }
  })
}

reg('time', ({ unnamed: [o] }) => {
  const d = applyOffset(new Date(), o)
  return `${p2(d.getHours())}:${p2(d.getMinutes())}`
})
reg('date', ({ unnamed: [o] }) => {
  const d = applyOffset(new Date(), o)
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日`
})
reg('weekday', ({ unnamed: [o] }) => WEEK_ZH[applyOffset(new Date(), o).getDay()] ?? '')
reg('isotime', ({ unnamed: [o] }) => {
  const d = applyOffset(new Date(), o)
  return `${p2(d.getHours())}:${p2(d.getMinutes())}`
})
reg('isodate', ({ unnamed: [o] }) => {
  const d = applyOffset(new Date(), o)
  return `${d.getFullYear()}-${p2(d.getMonth() + 1)}-${p2(d.getDate())}`
})
reg('datetimeformat', ({ unnamed: [f] }) => momentLike(new Date(), s(f)))
/** 毫秒差 → moment 风格英文 humanize */
function humanizeDuration(ms: number): string {
  const abs = Math.abs(ms)
  const table: [number, number, string, string][] = [
    [45_000, 1000, 'second', 'seconds'],
    [90_000, 60_000, 'a minute', 'minutes'],
    [45 * 60_000, 60_000, 'minute', 'minutes'],
    [90 * 60_000, 3_600_000, 'an hour', 'hours'],
    [22 * 3_600_000, 3_600_000, 'hour', 'hours'],
    [36 * 3_600_000, 86_400_000, 'a day', 'days'],
    [25 * 86_400_000, 86_400_000, 'day', 'days'],
    [45 * 86_400_000, 2_592_000_000, 'a month', 'months'],
    [320 * 86_400_000, 2_592_000_000, 'month', 'months'],
    [548 * 86_400_000, 31_536_000_000, 'a year', 'years'],
  ]
  for (const [limit, div, one, many] of table) {
    if (abs <= limit) {
      const n = Math.max(1, Math.round(abs / div))
      return n === 1 ? one : `${n} ${many}`
    }
  }
  return `${Math.max(1, Math.round(abs / 31_536_000_000))} years`
}

// idleDuration：距上一次用户发言的时长（ST 取倒数第二条用户消息的 send_date）
reg('idleDuration', ({ ctx }) => {
  return ctx.lastUserSendDate ? humanizeDuration(Date.now() - ctx.lastUserSendDate) : ''
})
reg('idle_duration', ({ ctx }) => {
  return ctx.lastUserSendDate ? humanizeDuration(Date.now() - ctx.lastUserSendDate) : ''
})
// timeDiff：两个时间值的绝对差（ST moment.duration(...).humanize(true)）
reg('timeDiff', ({ unnamed: [left, right] }) => {
  const l = Date.parse(s(left))
  const r = Date.parse(s(right))
  if (!Number.isFinite(l) || !Number.isFinite(r)) return ''
  return humanizeDuration(l - r)
})

/* ---- random / pick / roll ---- */
/** 参数切分：支持 :: 与逗号（{{random::a::b}} 或 {{random a,b}} / {{random:a,b}}） */
function listArgs(raw: string, unnamed: string[]): string[] {
  const parts = unnamed.length > 1 ? unnamed : [raw]
  return parts
    .flatMap((x) => x.split('::'))
    .flatMap((x) => x.split(','))
    .map((x) => x.trim())
    .filter(Boolean)
}


reg('random', ({ unnamed, raw }) => {
  const items = listArgs(raw, unnamed)
  return items.length ? (items[Math.floor(Math.random() * items.length)] ?? '') : ''
})
reg('pick', ({ unnamed, raw, ctx }) => {
  const items = listArgs(raw, unnamed)
  if (!items.length) return ''
  // 种子与网页端同构：getStringHash([chatIdHash, contentHash, offset, rerollSeed].join('-'))
  // PRNG 同为 seedrandom → 同会话同文本同位置的选取结果跨端一致
  const offset = pickRunEnv ? pickRunEnv.ordinal++ : 0
  const contentHash = pickRunEnv?.contentHash ?? getStringHash(raw)
  const parts = [ctx.chatIdHash, contentHash, offset, ctx.pickRerollSeed ?? null].filter(
    (x) => x !== null && x !== undefined,
  )
  const rng = seedrandom(String(getStringHash(parts.join('-'))))
  return items[Math.floor(rng() * items.length)] ?? ''
})

/** droll 子集：[N]dM[+K/-K]、裸数字 = 1dX（{{roll::20}} = 1d20） */
function rollDice(formula: string): number | null {
  const f = s(formula).trim().toLowerCase()
  const norm = /^\d+$/.test(f) ? `1d${f}` : f
  const m = /^(\d*)d(\d+)(?:([+-])(\d+))?$/.exec(norm)
  if (!m) return null
  const count = Math.max(1, Math.min(100, Number(m[1]) || 1))
  const sides = Math.max(1, Math.min(1000, Number(m[2])))
  const mod = m[3] ? (m[3] === '+' ? 1 : -1) * (Number(m[4]) || 0) : 0
  let total = mod
  for (let i = 0; i < count; i++) total += 1 + Math.floor(Math.random() * sides)
  return total
}

reg('roll', ({ unnamed, raw }) => {
  const result = rollDice(unnamed[0] ?? raw)
  return result === null ? '' : String(result)
})

/* ---- variables（chat / global 双存储） ---- */
function registerVarFamily(kind: 'var' | 'globalvar'): void {
  const pick = (c: MacroContext): MacroVariables => (kind === 'var' ? c.chatVars : c.globalVars)
  const p = kind === 'var' ? '' : 'global'
  reg(`set${p}var`, ({ unnamed, ctx }) => {
    const [name, value] = unnamed
    if (name) pick(ctx).set(name, value ?? '')
    return ''
  })
  reg(`add${p}var`, ({ unnamed, ctx }) => {
    const [name, value] = unnamed
    if (name) pick(ctx).set(name, (Number(pick(ctx).get(name)) || 0) + (Number(value) || 0))
    return ''
  })
  reg(`inc${p}var`, ({ unnamed, ctx }) => {
    if (unnamed[0]) pick(ctx).set(unnamed[0], (Number(pick(ctx).get(unnamed[0])) || 0) + 1)
    return ''
  })
  reg(`dec${p}var`, ({ unnamed, ctx }) => {
    if (unnamed[0]) pick(ctx).set(unnamed[0], (Number(pick(ctx).get(unnamed[0])) || 0) - 1)
    return ''
  })
  reg(`get${p}var`, ({ unnamed, ctx }) => s(pick(ctx).get(unnamed[0] ?? '')))
  reg(`has${p}var`, ({ unnamed, ctx }) => (pick(ctx).has(unnamed[0] ?? '') ? 'true' : ''))
  reg(`delete${p}var`, ({ unnamed, ctx }) => {
    pick(ctx).delete(unnamed[0] ?? '')
    return ''
  })
  reg(`set${p}varkey`, ({ unnamed, ctx }) => {
    const [name, key, value] = unnamed
    if (name && key) {
      const obj = pick(ctx).get(name)
      const o = obj && typeof obj === 'object' ? (obj as Record<string, unknown>) : {}
      o[key] = value ?? ''
      pick(ctx).set(name, o)
    }
    return ''
  })
  reg(`get${p}varkey`, ({ unnamed, ctx }) => {
    const [name, key] = unnamed
    const obj = pick(ctx).get(name)
    return obj && typeof obj === 'object' ? s((obj as Record<string, unknown>)[key ?? '']) : ''
  })
  reg(`varexists`, ({ unnamed, ctx }) => (ctx.chatVars.has(unnamed[0] ?? '') ? 'true' : ''))
  reg(`globalvarexists`, ({ unnamed, ctx }) =>
    ctx.globalVars.has(unnamed[0] ?? '') ? 'true' : '')
  reg(`flush${p}var`, () => '')
  reg(`flush${p}globalvar`, () => '')
  reg(`set${p}varindex`, () => '')
  reg(`get${p}varindex`, () => '')
}
registerVarFamily('var')
registerVarFamily('globalvar')

/* ---- if / else ---- */
export function evalCondition(cond: string, ctx: MacroContext): boolean {
  let c = cond.trim()
  const invert = c.startsWith('!')
  if (invert) c = c.slice(1).trim()
  // 变量简写：.name = chat var，$name = global var
  if (/^\.[\w-]+$/.test(c)) c = s(ctx.chatVars.get(c.slice(1)))
  else if (/^\$[\w-]+$/.test(c)) c = s(ctx.globalVars.get(c.slice(1)))
  const truthy = c !== '' && c !== '0' && c.toLowerCase() !== 'false' && c.toLowerCase() !== 'null'
  return invert ? !truthy : truthy
}
reg('if', ({ unnamed, ctx }) => {
  const [cond, content] = unnamed
  if (!cond) return ''
  return evalCondition(cond, ctx) ? s(content) : ''
})
reg('else', () => '')

/* ---- instruct（App 无 instruct 模式，全部空） ---- */
for (const k of [
  'charPrimaryInstruct', 'charSecondaryInstruct', 'charGreetingInstruct', 'charGroupInstruction',
  'systemPrompt', 'instructionExampleUser', 'instructionExampleAssistant', 'instructionSystem',
  'instructionUser', 'instructionAssistant', 'instructionInput', 'instructionPostHistory',
]) {
  reg(k, () => '')
}

/* ------------------------------------------------------------------ *
 * 展开器
 * ------------------------------------------------------------------ */

const MAX_PASSES = 8

/** 单个宏内层可能有嵌套 —— 内层无 {{ 的才直接替换；嵌套靠多轮迭代 */
function expandOnce(text: string, ctx: MacroContext): string {
  return text.replace(/\{\{([^{}]+)\}\}/g, (_w, inner: string) => {
    const trimmed = inner.trim()
    if (!trimmed) return ''
    if (trimmed.startsWith('//')) return ''
    // 名称 + 参数：优先 :: 分隔；否则「名:参数」（不带空格的冒号语法）或「名 参数」（空格语法）
    let name: string
    let rawArgs = ''
    const sep = trimmed.indexOf('::')
    const colonSep = sep < 0 ? trimmed.search(/:(?!:)/) : -1
    const spaceSep = sep < 0 && colonSep < 0 ? trimmed.search(/\s/) : -1
    if (sep >= 0) {
      name = trimmed.slice(0, sep).trim()
      rawArgs = trimmed.slice(sep + 2)
    } else if (colonSep > 0) {
      name = trimmed.slice(0, colonSep).trim()
      rawArgs = trimmed.slice(colonSep + 1)
    } else if (spaceSep > 0) {
      name = trimmed.slice(0, spaceSep).trim()
      rawArgs = trimmed.slice(spaceSep + 1)
    } else {
      name = trimmed
    }
    const handler = registry.get(name.toLowerCase())
    if (!handler) return `{{${inner}}}` // 未注册宏原样保留（ST 行为）
    const unnamed = rawArgs ? rawArgs.split('::').map((x) => x.trim()) : []
    return handler({ unnamed, raw: rawArgs, ctx })
  })
}

/**
 * 块级 {{if cond}}…{{else}}…{{/if}}，支持嵌套。
 * 做法：每轮只匹配「最内层」块（body 内不含 {{if 开标签），求值后外层块在下一轮
 * 自然成形；最多迭代 MAX_PASSES 轮（不平衡标签时会自然停轮）。
 */
export function expandIfBlocks(text: string, ctx: MacroContext): string {
  if (!text.includes('{{')) return text
  // 内层优先：body 里不允许再出现 {{if 开标签
  const innermost =
    /\{\{\s*if\s*(?:::|:|：)?\s*([^}]*)\}\}((?:(?!\{\{\s*if\b)[\s\S])*?)\{\{\s*\/\s*if\s*\}\}/g
  let out = text
  for (let i = 0; i < MAX_PASSES; i++) {
    const next = out.replace(innermost, (_w, condRaw: string, body: string) => {
      // 空条件 = falsy（走 else 分支）
      const m = /\{\{\s*else\s*\}\}/.exec(body)
      const thenBranch = m ? body.slice(0, m.index) : body
      const elseBranch = m ? body.slice(m.index + m[0].length) : ''
      return evalCondition(condRaw, ctx) ? thenBranch : elseBranch
    })
    if (next === out) break
    out = next
  }
  return out
}

/**
 * 展开文本中的全部宏（块级 if + 多轮迭代处理嵌套）。
 */
export function runMacros(text: string, ctx: MacroContext): string {
  if (!text || !text.includes('{{')) return text
  // pick 求值环境：contentHash = 本次展开文本的哈希（与网页端 env.contentHash 同义）
  pickRunEnv = { contentHash: getStringHash(text), ordinal: 0 }
  let out = expandIfBlocks(text, ctx)
  for (let i = 0; i < MAX_PASSES; i++) {
    const next = expandOnce(out, ctx)
    if (next === out) break
    out = next
  }
  pickRunEnv = null
  return out
}
