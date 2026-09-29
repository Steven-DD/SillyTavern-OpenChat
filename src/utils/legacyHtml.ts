/**
 * 旧式 HTML4 着色标签兼容改写（P11 对齐补丁）。
 *
 * 背景：ST 网页端消息渲染用 DOMPurify 宽松配置，`<font>` / `<center>` / `<u>` /
 * `<mark>` / `<big>` / `<small>` 均放行（角色卡与 AI 输出常用这类标签着色）；
 * App 的 Markdown 通路走 streamdown-vue3，其内置 rehype-sanitize 是 GitHub 白名单，
 * 上述标签全部被剥离 → 同一条消息网页端有色、App 无色（实测 font 标签被剥后只剩纯文本）。
 *
 * 方案：喂给 Streamdown 前把这些标签改写为白名单内的 span + 内联 style；
 * shiki 双主题通路产出的颜色是内联 style，不受影响。围栏代码块与行内代码原样保留。
 */

/** 不参与改写的段落：围栏代码块（流式中允许未闭合）与行内代码 */
const SKIP = /(```[\s\S]*?(?:```|$)|~~~[\s\S]*?(?:~~~|$)|`[^`\n]*`)/g

/** font size 旧式 1-7 档位 → 像素（HTML4 规范近似值） */
const SIZE_MAP: Record<string, string> = {
  '1': '10px',
  '2': '13px',
  '3': '16px',
  '4': '18px',
  '5': '24px',
  '6': '32px',
  '7': '48px',
}

/** <font color=.. face=.. size=..> → <span style=".."> */
function fontToSpan(attrs: string): string {
  const styles: string[] = []
  const re = /(color|face|size)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s">]+))/gi
  let m: RegExpExecArray | null
  while ((m = re.exec(attrs))) {
    const v = (m[2] ?? m[3] ?? m[4] ?? '').trim()
    if (!v) continue
    const key = m[1].toLowerCase()
    if (key === 'color') styles.push(`color:${v}`)
    else if (key === 'face') styles.push(`font-family:${v}`)
    else {
      const px = SIZE_MAP[v] ?? SIZE_MAP[String(Number(v))]
      if (px) styles.push(`font-size:${px}`)
    }
  }
  return styles.length ? `<span style="${styles.join(';')}">` : '<span>'
}

function rewriteSegment(seg: string): string {
  if (seg.startsWith('```') || seg.startsWith('~~~') || seg.startsWith('`')) return seg
  return seg
    .replace(/<font\b([^>]*)>/gi, (_t, attrs: string) => fontToSpan(attrs))
    .replace(/<\/font\s*>/gi, '</span>')
    .replace(/<center\b[^>]*>/gi, '<span style="display:block;text-align:center">')
    .replace(/<\/center\s*>/gi, '</span>')
    .replace(/<u\b[^>]*>/gi, '<span style="text-decoration:underline">')
    .replace(/<\/u\s*>/gi, '</span>')
    .replace(/<mark\b[^>]*>/gi, '<span style="background:#ffec99;border-radius:2px;padding:0 2px">')
    .replace(/<\/mark\s*>/gi, '</span>')
    .replace(/<big\b[^>]*>/gi, '<span style="font-size:1.2em">')
    .replace(/<\/big\s*>/gi, '</span>')
    .replace(/<small\b[^>]*>/gi, '<span style="font-size:0.85em">')
    .replace(/<\/small\s*>/gi, '</span>')
}

/** 把 ST 消息里的旧式着色标签改写为 Streamdown 白名单内的等价写法 */
export function normalizeLegacyHtml(text: string): string {
  if (!text.includes('<')) return text
  if (!/<(font|center|u|mark|big|small)\b/i.test(text)) return text
  // split 保留捕获的分隔段（围栏/行内代码在奇数位），逐段改写后拼回
  return text.split(SKIP).map(rewriteSegment).join('')
}
