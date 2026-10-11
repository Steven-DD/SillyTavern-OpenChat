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

/** 解析 CSS 颜色的相对亮度（0-1）。支持 rgb/rgba 与 #hex（3/6 位）；解析失败返回 null */
export function cssLuminance(css: string): number | null {
  const m = css.match(/rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i)
  if (m) {
    const r = Number(m[1])
    const g = Number(m[2])
    const b = Number(m[3])
    return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255
  }
  const hex = css.trim().startsWith('#') ? css.trim().slice(1) : ''
  if (/^[0-9a-f]{3}$|^[0-9a-f]{6}$/i.test(hex)) {
    const full = hex.length === 3 ? [...hex].map((x) => x + x).join('') : hex
    const r = parseInt(full.slice(0, 2), 16)
    const g = parseInt(full.slice(2, 4), 16)
    const b = parseInt(full.slice(4, 6), 16)
    return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255
  }
  return null
}

/** 浅色底适配：过亮的颜色（如 Azure 主题的纯白斜体）压暗到可读，其余原样 */
export function adaptColorForLight(css: string): string {
  const l = cssLuminance(css)
  if (l !== null && l > 0.6) return `color-mix(in srgb, ${css} 55%, #26282b)`
  return css
}

/** 把 ST 主题文本色转成 CSS 变量组；light=true 时对过亮颜色做压暗适配 */
export function stTextColorsToVars(c: StTextColors, light: boolean): Record<string, string> {
  const adapt = (v?: string): string | undefined =>
    v ? (light ? adaptColorForLight(v) : v) : undefined
  const vars: Record<string, string> = {}
  const em = adapt(c.em)
  const quote = adapt(c.quote)
  const underline = adapt(c.underline)
  if (em) vars['--st-c-em'] = em
  if (quote) vars['--st-c-quote'] = quote
  if (underline) vars['--st-c-underline'] = underline
  return vars
}
