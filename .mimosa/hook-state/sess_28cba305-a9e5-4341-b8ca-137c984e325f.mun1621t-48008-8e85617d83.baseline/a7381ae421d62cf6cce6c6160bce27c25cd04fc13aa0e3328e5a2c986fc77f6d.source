/**
 * 代码围栏语言自动探测（P9 对齐：网页端用 highlight.js 自动探测，shiki 需要显式语言）。
 * 无语言标记的 ``` 围栏用 highlight.js 探测语言后回写到围栏头，交给下游高亮。
 * 探测结果按代码内容缓存（LRU 式 FIFO 淘汰）。
 */
import hljs from 'highlight.js/lib/common'

const cache = new Map<string, string | null>()
const CACHE_MAX = 300
/** 探测采样上限：超长代码只取前 4KB，避免拖慢渲染 */
const SAMPLE_LEN = 4000

const FENCE = /(^|\n)(`{3,})([^\n`]*)\n([\s\S]*?)\n?\2(?=\n|$)/g

export function detectFenceLanguages(text: string): string {
  if (!text.includes('```')) return text
  return text.replace(FENCE, (full, pre: string, fence: string, info: string, code: string) => {
    if (info.trim()) return full // 已带语言
    if (!cache.has(code)) {
      const detected = code.trim().length ? hljs.highlightAuto(code.slice(0, SAMPLE_LEN)).language : null
      if (cache.size >= CACHE_MAX) cache.delete(cache.keys().next().value!)
      cache.set(code, detected ?? null)
    }
    const lang = cache.get(code)
    return lang ? `${pre}${fence}${lang}\n${code}\n${fence}` : full
  })
}
