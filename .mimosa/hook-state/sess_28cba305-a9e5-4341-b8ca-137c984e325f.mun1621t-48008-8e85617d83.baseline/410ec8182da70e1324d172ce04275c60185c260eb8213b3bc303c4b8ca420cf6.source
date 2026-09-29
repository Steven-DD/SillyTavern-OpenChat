/**
 * 正则脚本引擎
 *
 * 数据：settings.json 根级 extension_settings.regex = RegexScript[]
 * 作用点（regex_placement）：1=用户输入 2=AI输出 5=世界书 6=推理内容（3=斜杠命令不适用）
 * - promptOnly=true：只作用于发给 LLM 的提示词
 * - markdownOnly=true：只作用于界面显示
 * - 两者皆 false：显示与提示词都生效（ST 会改写源文本，App 侧不改存储，等价应用）
 * - minDepth/maxDepth：距末尾消息深度过滤（prompt 通路）
 * - substituteRegex：findRegex 是否先做宏替换（0 不 / 1 替换 / 2 替换并转义）
 */
import { getSettings, saveSettingsFull } from './data'
import { runMacros, type MacroContext } from './macros'

export const REGEX_PLACEMENT = {
  USER_INPUT: 1,
  AI_OUTPUT: 2,
  SLASH_COMMAND: 3,
  WORLD_INFO: 5,
  REASONING: 6,
} as const

export interface RegexScript {
  id: string
  scriptName: string
  findRegex: string
  replaceString: string
  trimStrings: string[]
  placement: number[]
  disabled: boolean
  markdownOnly: boolean
  promptOnly: boolean
  runOnEdit: boolean
  substituteRegex: 0 | 1 | 2
  minDepth: number | null
  maxDepth: number | null
}

export function newRegexScript(): RegexScript {
  return {
    id: `r${Date.now().toString(36)}${Math.floor(Math.random() * 1e4).toString(36)}`,
    scriptName: '新脚本',
    findRegex: '',
    replaceString: '',
    trimStrings: [],
    placement: [REGEX_PLACEMENT.AI_OUTPUT],
    disabled: false,
    markdownOnly: false,
    promptOnly: false,
    runOnEdit: true,
    substituteRegex: 0,
    minDepth: null,
    maxDepth: null,
  }
}

/* ---- 模块级缓存（App 单一写入方，改动经 saveRegexScripts 后失效） ---- */
let cache: RegexScript[] | null = null

export function invalidateRegexCache(): void {
  cache = null
}

export async function loadRegexScripts(force = false): Promise<RegexScript[]> {
  if (cache && !force) return cache
  const settings = await getSettings()
  const ext = settings.extension_settings as Record<string, unknown> | undefined
  const raw = ext?.regex
  cache = Array.isArray(raw) ? (raw as RegexScript[]).filter((x) => x && typeof x === 'object') : []
  return cache
}

/** 写回 settings.json（读全量 → 合并 → 写全量，先比对一致不写） */
export async function saveRegexScripts(scripts: RegexScript[]): Promise<void> {
  const settings = await getSettings()
  const before = JSON.stringify((settings.extension_settings as Record<string, unknown>)?.regex ?? [])
  const after = JSON.stringify(scripts)
  if (before === after) {
    cache = scripts
    return
  }
  settings.extension_settings = {
    ...((settings.extension_settings as Record<string, unknown>) ?? {}),
    regex: scripts,
  }
  await saveSettingsFull(settings)
  cache = scripts
}

/** /pattern/flags 写法 → RegExp；裸正则补 gu 全局标志 */
/** 编译缓存：同一脚本每条消息都会执行，重复 new RegExp 浪费明显。
 *  键 = 宏替换后的 find 串（substituteRegex 展开结果），脚本数量有限无增长风险。 */
const compileCache = new Map<string, RegExp | null>()

function regexFromString(find: string): RegExp | null {
  const hit = compileCache.get(find)
  if (hit !== undefined) return hit
  const m = /^\/([\s\S]+)\/([gimsuy]*)$/.exec(find.trim())
  let re: RegExp | null
  try {
    re = m ? new RegExp(m[1]!, m[2] ?? '') : new RegExp(find, 'gu')
  } catch {
    re = null
  }
  compileCache.set(find, re)
  return re
}

export interface RegexRunOptions {
  placement: number
  /** true = 提示词通路（promptOnly 生效）；false = 显示通路（markdownOnly 生效） */
  isPrompt: boolean
  /** 距末尾消息深度（prompt 通路用，null = 不过滤深度） */
  depth?: number | null
  /** substituteRegex 宏替换上下文 */
  macroCtx?: MacroContext
}

/**
 * 对文本应用一批正则脚本（ST getRegexedString 的等价实现）。
 * 替换串支持 $1 分组引用与 {{match}}。
 */
export function runRegex(
  text: string,
  scripts: RegexScript[],
  opts: RegexRunOptions,
): string {
  let out = text
  for (const sc of scripts) {
    if (sc.disabled || !sc.findRegex) continue
    if (!sc.placement?.includes(opts.placement)) continue
    // 通路判定：promptOnly → 仅提示词；markdownOnly → 仅显示；皆 false → 两者
    const toPrompt = sc.promptOnly || !sc.markdownOnly
    const toDisplay = sc.markdownOnly || !sc.promptOnly
    if (opts.isPrompt ? !toPrompt : !toDisplay) continue
    // 深度过滤（仅提示词通路）
    if (opts.isPrompt && opts.depth !== undefined && opts.depth !== null) {
      const depth = opts.depth
      if (sc.minDepth !== null && depth < sc.minDepth) continue
      if (sc.maxDepth !== null && depth > sc.maxDepth) continue
    }
    let find = sc.findRegex
    if (sc.substituteRegex && opts.macroCtx) {
      const expanded = runMacros(find, opts.macroCtx)
      find = sc.substituteRegex === 2 ? expanded.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') : expanded
    }
    const re = regexFromString(find)
    if (!re) continue
    try {
      // {{match}} → 替换阶段的 $&（整体匹配串）；$$& 先产出字面 $&
      const replacement = sc.replaceString.replace(/\{\{match\}\}/g, '$$&')
      out = out.replace(re, replacement)
    } catch {
      /* 单脚本异常不阻断 */
    }
    for (const t of sc.trimStrings ?? []) {
      if (t) out = out.replaceAll(t, '')
    }
  }
  return out
}

/** 便捷：取生效脚本（缓存未加载时返回空数组，供显示通路同步使用） */
export function activeRegexScripts(): RegexScript[] {
  return cache ?? []
}

/** 显示通路便捷函数（MessageBubble 同步调用，脚本需先 loadRegexScripts） */
export function applyDisplayRegex(
  content: string,
  placement: number,
): string {
  const scripts = cache
  if (!scripts?.length) return content
  return runRegex(content, scripts, { placement, isPrompt: false })
}
