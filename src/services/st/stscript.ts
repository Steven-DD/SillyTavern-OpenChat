/**
 * STscript 子集
 *
 * ── 语法 ──
 * - 管道 `|` 分段（引号内不分割）；上一段输出进入下一段的 {{pipe}} 宏
 * - 命令大小写不敏感；参数中的宏（{{getvar::x}} 等）由调用方先跑 runMacros 后传入
 * - `/if 条件 | then链 | else链`：条件支持 == != > < >= <=（两侧做宏展开与 trim）
 *
 * ── v1 命令集 ──
 * echo / send / genraw / setvar / getvar / addvar / flushvar / random / pick / delay / if / sys
 * 变量存 App 宏存储（chat/global 两级，键名前缀 local: 表示全局）。
 */
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

/** 执行上下文（由调用方注入，命令可测） */
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
}

/** 数值尝试解析（addvar/random 范围用） */
function num(v: string): number | null {
  const n = Number(v.trim())
  return Number.isFinite(n) ? n : null
}

/** 命令处理表：返回值进入管道（undefined = 不改变管道） */
type Handler = (args: string, ctx: ScriptContext, pipe: string) => Promise<string | undefined> | string | undefined

const COMMANDS: Record<string, Handler> = {
  echo: (args, ctx) => {
    ctx.echo(args)
    return undefined
  },
  send: async (args, ctx) => {
    await ctx.send(args)
    return undefined
  },
  genraw: async (args, ctx) => ctx.genraw(args),
  setvar: (args, ctx) => {
    // /setvar key=value（或 key value）
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
  flushvar: (args, ctx) => {
    const key = args.trim()
    if (key) ctx.vars.delete(key)
    ctx.afterChange?.()
    return undefined
  },
  random: (args) => {
    // /random a,b,c 或 /random 1,100
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
  delay: async (args) => {
    const ms = Math.min(30_000, Math.max(0, num(args) ?? 0))
    await new Promise((r) => setTimeout(r, ms))
    return undefined
  },
  if: () => {
    throw new Error('/if 应由执行器特判，不应直接调用')
  },
  sys: async (args, ctx) => {
    if (!ctx.sys) throw new Error('/sys 仅在会话上下文可用')
    await ctx.sys(args)
    return undefined
  },
}

/** 条件比较（/if） */
function evalCondition(cond: string): boolean {
  const m = /^([\s\S]+?)\s*(==|!=|>=|<=|>|<)\s*([\s\S]+)$/.exec(cond.trim())
  if (!m) return cond.trim().length > 0 && cond.trim() !== 'false'
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

/**
 * 执行一段脚本（单管道链）。
 * /if 条件 | 其后全部段：条件真 → 执行剩余段；假 → 跳过剩余段返回当前 pipe。
 * （v1 无 else 分支：ST 的 else 语法依赖命令面板组合，App 端以两条 /if 表达）
 * 返回最后一段的管道输出（有输出时）。
 */
export async function runScript(script: string, ctx: ScriptContext): Promise<string> {
  const commands = parseScript(script)
  let pipe = ''
  for (let i = 0; i < commands.length; i++) {
    const cmd = commands[i]!
    if (cmd.name === 'if') {
      const cond = ctx.expand(cmd.args)
      if (!evalCondition(cond)) return pipe
      // 条件成立：剩余段作为链继续执行
      const rest = commands.slice(i + 1)
      if (rest.length) return runCommands(rest, ctx, pipe)
      return pipe
    }
    pipe = (await runCommands([cmd], ctx, pipe)) ?? pipe
  }
  return pipe
}

/** 顺序执行一小段命令链（_plain 文本段直接作为输出） */
async function runCommands(cmds: ScriptCommand[], ctx: ScriptContext, pipe: string): Promise<string> {
  let p = pipe
  for (const cmd of cmds) {
    if (cmd.name === '__plain') {
      p = ctx.expand(cmd.args)
      continue
    }
    const handler = COMMANDS[cmd.name]
    if (!handler) throw new Error(`未知命令：/${cmd.name}（v1 支持：${Object.keys(COMMANDS).filter((k) => k !== 'if').join(' /')}）`)
    const args = ctx.expand(cmd.args.replace(/\{\{\s*pipe\s*\}\}/gi, p))
    const out = await handler(args, ctx, p)
    if (out !== undefined) p = out
  }
  return p
}

/** 支持的命令清单（UI 提示用） */
export const SCRIPT_COMMAND_NAMES = Object.keys(COMMANDS)
