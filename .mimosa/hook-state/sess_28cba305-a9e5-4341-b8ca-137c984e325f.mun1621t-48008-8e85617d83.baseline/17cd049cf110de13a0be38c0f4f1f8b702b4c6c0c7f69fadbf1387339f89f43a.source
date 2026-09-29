/**
 * ST 主题文本色透传（对齐网页端消息着色）。
 *
 * 网页端消息内的 em/i（斜体）、u（下划线）、q（引号）颜色来自主题变量：
 *   .mes_text i, .mes_text em { color: var(--SmartThemeEmColor) }
 *   .mes_text u { color: var(--SmartThemeUnderlineColor) }
 *   .mes_text q { color: var(--SmartThemeQuoteColor) }
 * 这些值存于 settings.json → power_user.italics_text_color 等字段（应用主题时会写入）。
 * App 读取后注入 CSS 变量（--st-c-em 等），气泡内对应元素跟随当前 ST 主题。
 */
import { getSettings } from './data'

export interface StTextColors {
  /** 斜体色（power_user.italics_text_color） */
  em?: string
  /** 引号色（power_user.quote_text_color） */
  quote?: string
  /** 下划线色（power_user.underline_text_color） */
  underline?: string
}

let cache: StTextColors | null | undefined

export async function loadStTextColors(force = false): Promise<StTextColors | null> {
  if (cache !== undefined && !force) return cache
  try {
    const s = (await getSettings()) as Record<string, unknown>
    const pu = s.power_user as Record<string, unknown> | undefined
    if (!pu) {
      cache = null
      return cache
    }
    const pick = (k: string): string | undefined => {
      const v = pu[k]
      return typeof v === 'string' && v.trim() ? v.trim() : undefined
    }
    cache = {
      em: pick('italics_text_color'),
      quote: pick('quote_text_color'),
      underline: pick('underline_text_color'),
    }
  } catch {
    cache = null
  }
  return cache
}
