/**
 * Summarize 总结记忆
 * - 摘要正文：chat_metadata.summary
 * - 设置：extension_settings.memory
 *   + App 开关 extension_settings.stchat_memory.enabled
 * - 摘要生成本身由 chat store 用主生成通道执行（generateSummaryPrompt 提供提示词）
 */
import { getSettings, saveSettingsFull } from './data'
import { substituteMacros } from './prompt'

/** ST memory 扩展默认提示词（promptWords 替换 {{words}}） */
export const MEMORY_DEFAULTS = {
  prompt:
    'Ignore previous instructions. Summarize the most important parts of the chat so far, ' +
    'including all relevant facts, the current scenario, and the overall plot. Other characters\u2019 ' +
    'speech should be described indirectly. Continue using the same language as the chat. ' +
    'Limit yourself to at most {{words}} words.',
  /** 注入模板（{{summary}} 替换） */
  template: '[Summary of the chat so far: {{summary}}]',
  /** 每 N 条消息更新一次摘要（ST 默认 1，App 默认 10 省配额，可调） */
  interval: 10,
  depth: 2,
  /** 0 system / 1 user / 2 assistant */
  role: 0,
  /** 0 = 系统提示顶部；1 = @Depth */
  position: 1,
  words: 200,
}

export interface MemorySettings {
  prompt: string
  template: string
  interval: number
  depth: number
  role: number
  position: number
}

let cache: MemorySettings | null = null

export function invalidateMemoryCache(): void {
  cache = null
}

export async function loadMemorySettings(force = false): Promise<MemorySettings> {
  if (cache && !force) return cache
  const settings = await getSettings()
  const ext = settings.extension_settings as Record<string, unknown> | undefined
  const raw = ext?.memory as Partial<MemorySettings> | undefined
  cache = {
    prompt: typeof raw?.prompt === 'string' && raw.prompt.trim() ? raw.prompt : MEMORY_DEFAULTS.prompt,
    template:
      typeof raw?.template === 'string' && raw.template.trim() ? raw.template : MEMORY_DEFAULTS.template,
    interval:
      typeof raw?.interval === 'number' && raw.interval > 0 ? Math.floor(raw.interval) : MEMORY_DEFAULTS.interval,
    depth: typeof raw?.depth === 'number' ? Math.max(0, Math.floor(raw.depth)) : MEMORY_DEFAULTS.depth,
    role: typeof raw?.role === 'number' ? raw.role : MEMORY_DEFAULTS.role,
    position: raw?.position === 0 || raw?.position === 1 ? raw.position : MEMORY_DEFAULTS.position,
  }
  return cache
}

export async function saveMemorySettings(patch: Partial<MemorySettings>): Promise<MemorySettings> {
  const next = { ...(cache ?? MEMORY_DEFAULTS), ...patch }
  const settings = await getSettings()
  const ext = (settings.extension_settings as Record<string, unknown> | undefined) ?? {}
  if (JSON.stringify(ext.memory) === JSON.stringify(next)) {
    cache = next
    return next
  }
  settings.extension_settings = { ...ext, memory: next }
  await saveSettingsFull(settings)
  cache = next
  return next
}

/** App 开关（extension_settings.stchat_memory.enabled，默认关，避免悄悄烧配额） */
export async function loadMemoryAppEnabled(): Promise<boolean> {
  const settings = await getSettings()
  const ext = settings.extension_settings as Record<string, unknown> | undefined
  const raw = ext?.stchat_memory as { enabled?: boolean } | undefined
  return raw?.enabled === true
}

export async function saveMemoryAppEnabled(enabled: boolean): Promise<void> {
  const settings = await getSettings()
  const ext = (settings.extension_settings as Record<string, unknown> | undefined) ?? {}
  settings.extension_settings = { ...ext, stchat_memory: { ...(ext.stchat_memory as object | undefined), enabled } }
  await saveSettingsFull(settings)
}

/** 生成摘要的提示词 */
export function generateSummaryPrompt(
  summary: string,
  recentTexts: string[],
  charName: string,
  userName: string,
  settings?: Partial<MemorySettings>,
): string {
  const cfg = { ...MEMORY_DEFAULTS, ...settings }
  const words = cfg.prompt.includes('{{words}}') ? String(cfg.words) : ''
  const instruction = cfg.prompt.replace(/\{\{words\}\}/g, words)
  const prev = summary.trim()
  const history = recentTexts.slice(-40).join('\n')
  return [
    instruction,
    '',
    `Chat between ${userName} and ${charName}:`,
    history || '（空）',
    prev ? `Previous summary:\n${prev}` : '',
  ]
    .filter(Boolean)
    .join('\n\n')
}

/** 注入块（{{summary}} 替换由调用方通过宏/直接替换完成） */
export function summaryInjectionBlock(summary: string, charName: string, userName: string): string {
  const tpl = cache?.template ?? MEMORY_DEFAULTS.template
  return substituteMacros(tpl.replace(/\{\{summary\}\}/gi, summary.trim()), charName, userName)
}
