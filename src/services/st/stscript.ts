/**
 * STscript 脚本引擎 (40+ 核心命令集与流控)
 *
 * ── 语法 ──
 * - 管道 `|` 分段（引号内不分割）；上一段输出进入下一段的 {{pipe}} 宏
 * - 命令大小写不敏感；参数中的宏（{{getvar::x}} 等）由调用方先跑 runMacros 后传入
 * - `/if 条件 | then链`：条件支持 == != > < >= <=（两侧做宏展开与 trim）
 * - `/while 条件 | 循环链`：带最大 100 次硬性上限守护，防死循环冻结 UI
 * - `/return`、`/abort`：流控中断
 *
 * ── 命令集 ──
 * 跑团掷骰：roll
 * 人设与角色：character / char / persona / cut / fork
 * 提示词干预：inject / an / pass / sys
 * UI 与反馈：echo / toast / popup / sound
 * 变量与运算：setvar / getvar / addvar / incvar / decvar / flushvar / hasvar / math
 * 字符串工具：len / trim / upper / lower / concat / replace / slice / split
 * 逻辑与数值：if / while / return / abort / and / or / not / min / max / abs / round / floor / ceil
 * 流程与时延：delay / sleep / random / pick / send / genraw
 */
import { rollDice } from './macros'
import type { MacroVariables } from './macros'

export interface ScriptCommand {
  name: string
  args: string
}

/** 按管道分段（忽略引号内的 |），每段拆出命令名与参数 */
export function parseScript(input: string): ScriptCommand[] {
  const segs: string[] = []
  let cur = ''
  let inQuote: string | null = null
  for (let i = 0; i < input.length; i++) {
    const ch = input[i]!
    if (inQuote) {
      if (ch === inQuote) inQuote = null
      cur += ch
      continue
    }
    if (ch === '"' || ch === "'") {
      inQuote = ch
      cur += ch
      continue
    }
    if (ch === '|') {
      segs.push(cur)
      cur = ''
      continue
    }
    cur += ch
  }
  segs.push(cur)
  return segs
    .map((s) => s.trim())
    .filter(Boolean)
    .map((seg) => {
      const m = /^\/?([\w:-]+)\s*([\s\S]*)$/.exec(seg)
      return m ? { name: m[1]!.toLowerCase(), args: (m[2] ?? '').trim() } : { name: '', args: seg }
    })
    .filter((c) => c.name)
}

/** 执行上下文（由调用方注入） */
export interface ScriptContext {
  /** 宏变量存取（chat 级） */
  vars: MacroVariables
  /** 宏展开（对 args 里的 {{...}} 先行展开，由调用方绑定当前会话上下文） */
  expand: (text: string) => string
  /** 发送一条用户消息（/send） */
  send: (text: string) => Promise<void>
  /** 无落盘生成（/genraw）；prompt 已宏展开 */
  genraw: (prompt: string) => Promise<string>
  /** 输出反馈（/echo）；UI 可接 toast */
  echo: (text: string) => void
  /** 插入旁白（/sys）：is_system 消息，不进 prompt */
  sys?: (text: string) => Promise<void>
  /** 保存变量副作用（执行后调用一次，默认空） */
  afterChange?: () => void

  /* --- 扩展宿主能力 --- */
  toast?: (text: string, level?: 'info' | 'warn' | 'error') => void
  popup?: (title: string, text: string) => Promise<void> | void
  sound?: (nameOrUrl: string) => Promise<void> | void
  setCharacter?: (name: string) => Promise<void>
  setPersona?: (name: string) => Promise<void>
  cut?: (count: number) => Promise<void>
  fork?: (name?: string) => Promise<void>
  injectPrompt?: (pos: 'system' | 'before' | 'after' | 'depth', text: string, depth?: number) => void
  setAuthorsNote?: (text: string, depth?: number) => void
  abort?: (reason?: string) => void
}

/** 数值尝试解析 */
function num(v: string): number | null {
  const n = Number(v.trim())
  return Number.isFinite(n) ? n : null
}

/** 安全求值简单算术四则运算（只允许数字和 + - * / ( ) . %） */
function safeEvalMath(expr: string): number | null {
  const sanitized = expr.replace(/\s+/g, '')
  if (!/^[0-9+\-*/().%]+$/.test(sanitized)) return null
  try {
    // 使用 Function 限制沙箱环境
    const res = Function(`"use strict"; return (${sanitized});`)()
    return typeof res === 'number' && Number.isFinite(res) ? res : null
  } catch {
    return null
  }
}

/** 条件比较（/if 与 /while） */
export function evalCondition(cond: string): boolean {
  const m = /^([\s\S]+?)\s*(==|!=|>=|<=|>|<)\s*([\s\S]+)$/.exec(cond.trim())
  if (!m) return cond.trim().length > 0 && cond.trim().toLowerCase() !== 'false' && cond.trim() !== '0'
  const norm = (s: string) => s.trim().replace(/^["']|["']$/g, '')
  const a = norm(m[1]!)
  const b = norm(m[3]!)
  const na = num(a)
  const nb = num(b)
  const bothNum = na !== null && nb !== null
  switch (m[2]) {
    case '==': return bothNum ? na === nb : a === b
    case '!=': return bothNum ? na !== nb : a !== b
    case '>': return bothNum ? na! > nb! : a > b
    case '<': return bothNum ? na! < nb! : a < b
    case '>=': return bothNum ? na! >= nb! : a >= b
    case '<=': return bothNum ? na! <= nb! : a <= b
    default: return false
  }
}

/** 特殊流控信号 */
class ScriptAbortError extends Error {
  constructor(public reason: string = 'Script execution aborted') {
    super(reason)
  }
}

class ScriptReturnSignal {
  constructor(public value: string) {}
}

/** 命令处理表：返回值进入管道（undefined = 不改变管道） */
type Handler = (args: string, ctx: ScriptContext, pipe: string) => Promise<string | undefined> | string | undefined

const COMMANDS: Record<string, Handler> = {
  // 1. 基础输出与消息
  echo: (args, ctx) => {
    ctx.echo(args)
    return undefined
  },
  send: async (args, ctx) => {
    await ctx.send(args)
    return undefined
  },
  genraw: async (args, ctx) => ctx.genraw(args),
  sys: async (args, ctx) => {
    if (!ctx.sys) throw new Error('/sys 仅在会话上下文可用')
    await ctx.sys(args)
    return undefined
  },

  // 2. 跑团与随机
  roll: (args) => {
    const res = rollDice(args || '1d20')
    if (res === null) throw new Error(`/roll 表达式无效: "${args}"（示例: 1d20+5, 2d6）`)
    return String(res)
  },
  random: (args) => {
    const items = args.split(',').map((s) => s.trim()).filter(Boolean)
    if (items.length === 2 && num(items[0]!) !== null && num(items[1]!) !== null) {
      const lo = num(items[0]!)!
      const hi = num(items[1]!)!
      return String(lo + Math.floor(Math.random() * (Math.abs(hi - lo) + 1)) * Math.sign(hi - lo || 1))
    }
    if (!items.length) throw new Error('/random 需要逗号分隔的候选项')
    return items[Math.floor(Math.random() * items.length)]!
  },
  pick: (args) => {
    const items = args.split(',').map((s) => s.trim()).filter(Boolean)
    if (!items.length) throw new Error('/pick 需要逗号分隔的候选项')
    return items[Math.floor(Math.random() * items.length)]!
  },

  // 3. 变量操作
  setvar: (args, ctx) => {
    const m = /^([\w:-]+?)\s*=\s*([\s\S]*)$/.exec(args) ?? /^([\w:-]+)\s+([\s\S]*)$/.exec(args)
    if (!m) throw new Error('/setvar 需要 key=value 参数')
    ctx.vars.set(m[1]!, m[2] ?? '')
    ctx.afterChange?.()
    return undefined
  },
  getvar: (args, ctx) => {
    const key = args.trim()
    if (!key) throw new Error('/getvar 需要变量名')
    return ctx.vars.get(key) === undefined ? '' : String(ctx.vars.get(key))
  },
  addvar: (args, ctx) => {
    const m = /^([\w:-]+?)\s*=\s*(-?[\d.]+)$/.exec(args) ?? /^([\w:-]+)\s+(-?[\d.]+)$/.exec(args)
    if (!m) throw new Error('/addvar 需要 key=数值 参数')
    const cur = num(String(ctx.vars.get(m[1]!) ?? '0')) ?? 0
    ctx.vars.set(m[1]!, cur + Number(m[2]))
    ctx.afterChange?.()
    return undefined
  },
  incvar: (args, ctx) => {
    const key = args.trim()
    if (!key) throw new Error('/incvar 需要变量名')
    const cur = num(String(ctx.vars.get(key) ?? '0')) ?? 0
    ctx.vars.set(key, cur + 1)
    ctx.afterChange?.()
    return undefined
  },
  decvar: (args, ctx) => {
    const key = args.trim()
    if (!key) throw new Error('/decvar 需要变量名')
    const cur = num(String(ctx.vars.get(key) ?? '0')) ?? 0
    ctx.vars.set(key, cur - 1)
    ctx.afterChange?.()
    return undefined
  },
  flushvar: (args, ctx) => {
    const key = args.trim()
    if (key) ctx.vars.delete(key)
    ctx.afterChange?.()
    return undefined
  },
  hasvar: (args, ctx) => {
    const key = args.trim()
    return ctx.vars.has(key) ? 'true' : 'false'
  },

  // 4. UI 与用户交互反馈
  toast: (args, ctx) => {
    if (ctx.toast) ctx.toast(args, 'info')
    else ctx.echo(args)
    return undefined
  },
  popup: async (args, ctx) => {
    if (ctx.popup) await ctx.popup('脚本提示', args)
    else ctx.echo(args)
    return undefined
  },
  sound: async (args, ctx) => {
    if (ctx.sound) await ctx.sound(args)
    return undefined
  },
  draw: async (args, ctx) => {
    const prompt = args.trim()
    if (!prompt) throw new Error('/draw 需要提供图像提示词')
    const { generateImage } = await import('./imageGen')
    if (ctx.toast) ctx.toast('正在绘制图像...', 'info')
    const res = await generateImage({ prompt })
    if (ctx.toast) ctx.toast(`图像生成完成: ${res.filename}`, 'info')
    return res.url
  },

  // 5. 角色与会话切换
  character: async (args, ctx) => {
    if (ctx.setCharacter) await ctx.setCharacter(args.trim())
    return undefined
  },
  char: async (args, ctx) => {
    if (ctx.setCharacter) await ctx.setCharacter(args.trim())
    return undefined
  },
  persona: async (args, ctx) => {
    if (ctx.setPersona) await ctx.setPersona(args.trim())
    return undefined
  },
  cut: async (args, ctx) => {
    const count = num(args) ?? 1
    if (ctx.cut) await ctx.cut(count)
    return undefined
  },
  fork: async (args, ctx) => {
    if (ctx.fork) await ctx.fork(args.trim() || undefined)
    return undefined
  },

  // 6. 提示词动态注入
  inject: (args, ctx) => {
    const m = /^(?:depth=(\d+)\s+)?([\s\S]+)$/i.exec(args)
    const depth = m?.[1] ? Number(m[1]) : 4
    const content = m?.[2] ?? args
    if (ctx.injectPrompt) ctx.injectPrompt('depth', content, depth)
    return undefined
  },
  an: (args, ctx) => {
    const m = /^(?:depth=(\d+)\s+)?([\s\S]+)$/i.exec(args)
    const depth = m?.[1] ? Number(m[1]) : 4
    const content = m?.[2] ?? args
    if (ctx.setAuthorsNote) ctx.setAuthorsNote(content, depth)
    return undefined
  },
  pass: (_args, ctx) => {
    ctx.abort?.('Pass generation')
    return undefined
  },

  // 7. 流控与中断
  abort: (args) => {
    throw new ScriptAbortError(args || 'Script aborted')
  },
  return: (args, _ctx, pipe) => {
    throw new ScriptReturnSignal(args.trim() ? args : pipe)
  },
  delay: async (args) => {
    const ms = Math.min(30_000, Math.max(0, num(args) ?? 0))
    await new Promise((r) => setTimeout(r, ms))
    return undefined
  },
  sleep: async (args) => {
    const ms = Math.min(30_000, Math.max(0, num(args) ?? 0))
    await new Promise((r) => setTimeout(r, ms))
    return undefined
  },

  // 8. 字符串处理
  len: (args) => String(args.length),
  trim: (args) => args.trim(),
  upper: (args) => args.toUpperCase(),
  lower: (args) => args.toLowerCase(),
  concat: (args) => {
    const parts = args.includes('::') ? args.split('::') : args.split(',')
    return parts.join('')
  },
  replace: (args) => {
    const m = /^([\s\S]+?)=([\s\S]*?)\s+([\s\S]+)$/.exec(args)
    if (!m) return args
    return m[3]!.split(m[1]!).join(m[2]!)
  },
  slice: (args) => {
    const m = /^(-?\d+)(?:,(-?\d+))?\s+([\s\S]*)$/.exec(args)
    if (!m) return args
    const start = Number(m[1])
    const end = m[2] !== undefined ? Number(m[2]) : undefined
    return m[3]!.slice(start, end)
  },
  split: (args) => {
    const m = /^([\s\S]+?),(\d+)\s+([\s\S]*)$/.exec(args)
    if (!m) return args
    const sep = m[1]!
    const idx = Number(m[2])
    const parts = m[3]!.split(sep)
    return parts[idx] ?? ''
  },

  // 9. 数学算术与统计
  math: (args) => {
    const res = safeEvalMath(args)
    if (res === null) throw new Error(`/math 表达式语法错误: "${args}"`)
    return String(res)
  },
  abs: (args) => {
    const n = num(args)
    return n === null ? '0' : String(Math.abs(n))
  },
  round: (args) => {
    const n = num(args)
    return n === null ? '0' : String(Math.round(n))
  },
  floor: (args) => {
    const n = num(args)
    return n === null ? '0' : String(Math.floor(n))
  },
  ceil: (args) => {
    const n = num(args)
    return n === null ? '0' : String(Math.ceil(n))
  },
  min: (args) => {
    const list = args.split(',').map((s) => num(s)).filter((n): n is number => n !== null)
    return list.length ? String(Math.min(...list)) : '0'
  },
  max: (args) => {
    const list = args.split(',').map((s) => num(s)).filter((n): n is number => n !== null)
    return list.length ? String(Math.max(...list)) : '0'
  },

  // 10. 逻辑门
  and: (args) => {
    const parts = args.split(/\s+/).filter(Boolean)
    return parts.every((p) => evalCondition(p)) ? 'true' : 'false'
  },
  or: (args) => {
    const parts = args.split(/\s+/).filter(Boolean)
    return parts.some((p) => evalCondition(p)) ? 'true' : 'false'
  },
  not: (args) => (evalCondition(args) ? 'false' : 'true'),

  // 11. 控制结构占位（由 runScript 执行器特判）
  if: () => {
    throw new Error('/if 应由执行器特判')
  },
  while: () => {
    throw new Error('/while 应由执行器特判')
  },
  '//': () => undefined,
  comment: () => undefined,
}

/**
 * 执行一段脚本（单管道链）。
 */
export async function runScript(script: string, ctx: ScriptContext): Promise<string> {
  const commands = parseScript(script)
  let pipe = ''
  try {
    for (let i = 0; i < commands.length; i++) {
      const cmd = commands[i]!
      // /if 逻辑分支：条件真 → 继续；假 → 跳过剩余段
      if (cmd.name === 'if') {
        const cond = ctx.expand(cmd.args)
        if (!evalCondition(cond)) return pipe
        const rest = commands.slice(i + 1)
        if (rest.length) return runCommands(rest, ctx, pipe)
        return pipe
      }

      // /while 循环：带最大 100 次防卡死限流
      if (cmd.name === 'while') {
        const rest = commands.slice(i + 1)
        let loops = 0
        const MAX_LOOPS = 100
        while (loops < MAX_LOOPS) {
          const cond = ctx.expand(cmd.args)
          if (!evalCondition(cond)) break
          pipe = await runCommands(rest, ctx, pipe)
          loops++
        }
        if (loops >= MAX_LOOPS) {
          console.warn('[STscript] /while loop hit safety cap (100 iterations)')
        }
        return pipe
      }

      pipe = (await runCommands([cmd], ctx, pipe)) ?? pipe
    }
  } catch (err) {
    if (err instanceof ScriptReturnSignal) {
      return err.value
    }
    if (err instanceof ScriptAbortError) {
      console.warn(`[STscript] Aborted: ${err.reason}`)
      return pipe
    }
    throw err
  }
  return pipe
}

/** 顺序执行一小段命令链 */
async function runCommands(cmds: ScriptCommand[], ctx: ScriptContext, pipe: string): Promise<string> {
  let p = pipe
  for (const cmd of cmds) {
    if (cmd.name === '__plain') {
      p = ctx.expand(cmd.args)
      continue
    }
    const handler = COMMANDS[cmd.name]
    if (!handler) {
      throw new Error(`未知命令：/${cmd.name}（可用: ${Object.keys(COMMANDS).filter((k) => k !== 'if' && k !== 'while').join(', ')}）`)
    }
    const args = ctx.expand(cmd.args.replace(/\{\{\s*pipe\s*\}\}/gi, p))
    const out = await handler(args, ctx, p)
    if (out !== undefined) p = out
  }
  return p
}

/** 支持的命令清单（UI 提示用） */
export const SCRIPT_COMMAND_NAMES = Object.keys(COMMANDS).filter((k) => k !== '//')
