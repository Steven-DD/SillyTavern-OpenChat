/**
 * Instruct 模式（Text Completion 拼接）
 *
 * - 数据源：settings.json power_user.instruct（ST Instruct 模板）
 * - prefix 按角色取序列（system→system_sequence（system_same_as_user 时用 input）
 *   user→input_sequence、assistant→output_sequence）
 * - 首/末变体：
 *   · first_*_sequence 只作用于**历史第 0 条**
 *   · last_input_sequence 只作用于**最后一条用户消息**（impersonate 时不加）
 *   · last_output_sequence 仅在 **continue** 时作用于被续写消息，且截掉 suffix
 * - {{name}} 宏：user→name1、assistant→name2、system→'System'
 * - names_behavior：none 不加名 / always 加名 / force（ST 群聊场景，App 单聊视同 none）
 * - wrap=true：片段间 '\n' 连接，空 suffix 补 '\n'
 * - 末尾补 output_sequence 前缀诱导接续；continue 时截掉（prompt 止于半截内容，ST prefill 语义）
 * - impersonate 时同样不补 output 前缀（ST 代写不添加 assistant 前缀）
 */
import { getSettings } from './data'
import type { PromptMessage } from './prompt'

export interface InstructSettings {
  enabled?: boolean
  preset?: string
  wrap?: boolean
  macro?: boolean
  names_behavior?: 'none' | 'force' | 'always'
  input_sequence?: string
  input_suffix?: string
  output_sequence?: string
  output_suffix?: string
  system_sequence?: string
  system_suffix?: string
  first_input_sequence?: string
  last_input_sequence?: string
  first_output_sequence?: string
  last_output_sequence?: string
  last_system_sequence?: string
  system_same_as_user?: boolean
  sequences_as_stop_strings?: boolean
  stop_sequence?: string
  [k: string]: unknown
}

/** 读 ST 的 Instruct 设置（power_user.instruct，未配置 → enabled: false） */
export async function loadInstruct(): Promise<InstructSettings> {
  const s = await getSettings()
  const pu = (s as Record<string, unknown>).power_user as Record<string, unknown> | undefined
  const ins = (pu?.instruct ?? {}) as InstructSettings
  return ins
}

function subst(seq: string | undefined, name: string): string {
  return (seq ?? '').replace(/{{name}}/gi, name)
}

export interface InstructFormatOptions {
  /** 续写：末条 assistant 消息用 last_output_sequence 且无 suffix，不补 output 前缀 */
  isContinue?: boolean
  /** 代写：不补 output 前缀、不加 last_input_sequence */
  isImpersonate?: boolean
}

/**
 * 把 messages 拼接为 instruct 文本。
 * 首/末变体语义见文件头注释。
 */
export function formatInstructChat(
  messages: PromptMessage[],
  instruct: InstructSettings,
  name1: string,
  name2: string,
  opts: InstructFormatOptions = {},
): { text: string; stop: string[] } {
  const wrap = instruct.wrap !== false
  const names = instruct.names_behavior === 'always'
  const sameAsUser = !!instruct.system_same_as_user

  // first_*：历史第 0 条（App 的 messages[0] 通常是 system 提示 → 取第一条非 system）
  const firstIdx = messages.findIndex((m) => m.role !== 'system')
  // last_input_sequence：最后一条用户消息（coreChat.findLastIndex(is_user)，:4770/4789）
  let lastUserIdx = -1
  messages.forEach((m, i) => {
    if (m.role === 'user') lastUserIdx = i
  })
  const lastIdx = messages.length - 1

  const parts: string[] = []
  messages.forEach((m, i) => {
    let prefix: string
    let suffix: string
    let name: string
    if (m.role === 'system') {
      prefix = (sameAsUser ? instruct.input_sequence : instruct.system_sequence) ?? ''
      suffix = (sameAsUser ? instruct.input_suffix : instruct.system_suffix) ?? ''
      name = 'System'
    } else if (m.role === 'user') {
      // ST：先按 first 重排版（:4784-4787），last 命中时覆盖（:4789-4792）→ LAST 优先
      prefix =
        (i === lastUserIdx && !opts.isImpersonate && instruct.last_input_sequence
          ? instruct.last_input_sequence
          : i === firstIdx && instruct.first_input_sequence
            ? instruct.first_input_sequence
            : instruct.input_sequence) ?? ''
      suffix = instruct.input_suffix ?? ''
      name = name1
    } else {
      prefix =
        (opts.isContinue && i === lastIdx && instruct.last_output_sequence
          ? instruct.last_output_sequence
          : i === firstIdx && instruct.first_output_sequence
            ? instruct.first_output_sequence
            : instruct.output_sequence) ?? ''
      suffix = instruct.output_suffix ?? ''
      name = name2
    }
    prefix = subst(prefix, name)
    suffix = subst(suffix, name)
    // continue：被续写消息不带任何 suffix（ST FORMAT_TOKEN 截尾技巧，:4794-4809）
    if (opts.isContinue && i === lastIdx) suffix = ''
    else if (!suffix && wrap) suffix = '\n'
    const body = names && name ? `${name}: ${m.content}` : m.content
    const seg = wrap ? [prefix, body + suffix].filter((x) => x).join('\n') : `${prefix}${body}${suffix}`
    parts.push(seg)
  })

  // stop：stop_sequence + 各角色序列（sequences_as_stop_strings !== false 时）
  const stop: string[] = []
  const push = (v: string | undefined) => {
    const t = (v ?? '').replace(/{{name}}/gi, '').trim()
    if (t) stop.push(t)
  }
  push(instruct.stop_sequence)
  if (instruct.sequences_as_stop_strings !== false) {
    push(instruct.input_sequence)
    push(instruct.output_sequence)
    push(instruct.first_output_sequence)
    push(instruct.last_output_sequence)
    push(instruct.system_sequence)
    push(instruct.last_system_sequence)
  }
  // 末尾补 output 前缀诱导接续（continue/impersonate 不补：prompt 止于半截内容或 system 轮）
  let text = parts.join(wrap ? '\n' : '')
  if (!opts.isContinue && !opts.isImpersonate) {
    const tail = subst(instruct.output_sequence, name2)
    if (tail) text += (wrap ? '\n' : '') + tail
  }
  return { text, stop }
}
