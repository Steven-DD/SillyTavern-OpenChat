/**
 * 前端界面（酒馆助手语义）消息检测与 iframe srcdoc 构建。
 *
 * 卡片自带正则脚本会把占位标记替换成完整 HTML 应用（```text 包裹的
 * <!DOCTYPE html>…），宿主把它渲染进 sandbox="allow-scripts" 的 iframe：
 * 卡内脚本 / CDN 资源在沙箱内正常运行，与 ST 网页端酒馆助手的
 * "前端界面"同构；数据访问一律经 postMessage 桥（embedBridge.ts），
 * 沙箱不给 allow-same-origin，脚本无法直连 ST 后端。
 */

/** 被代码围栏包裹的完整 HTML 文档（ST 正则 markdownOnly 替换后的典型形态） */
const FENCED_DOC =
  /```[\w-]*[ \t]*\r?\n(<!DOCTYPE\s+html[\s\S]*?<\/html>)[\s]*?```/i
/** 裸完整文档（无围栏） */
const BARE_DOC = /^\s*<!DOCTYPE\s+html[\s\S]*<\/html>\s*$/i
/** 带 <script> 的判定（纯静态 HTML 不值得走 iframe，markstream 直接渲染更好） */
const HAS_SCRIPT = /<script[\s>]/i

/**
 * 提取消息中的前端界面 HTML。命中返回文档字符串，否则空串。
 * 优先取被围栏包裹的文档（可与正文共存），否则整段必须是完整文档。
 */
export function extractEmbedHtml(content: string): string {
  if (!HAS_SCRIPT.test(content)) return ''
  const fenced = FENCED_DOC.exec(content)
  if (fenced) return fenced[1]!
  if (BARE_DOC.test(content)) return content.trim()
  return ''
}

/**
 * 跨块顶层 const/let 重复声明修复。
 * 卡常见缺陷：两个内联 <script> 顶层用 const 声明同名标识符 → 全局词法环境
 * 冲突，后声明块整块 SyntaxError 不执行（任何宿主如此，含 ST 网页端）。
 * 宿主侧自动修复：重复声明的块整体包 IIFE——同名声明变块内局部，
 * 对先前块全局绑定的读取不受影响；顶层判定取行首列 0（卡源码函数体均有缩进）。
 */
export function fixCrossBlockDeclCollision(html: string): string {
  const seen = new Set<string>()
  return html.replace(/<script>([\s\S]*?)<\/script>/gi, (m, body: string) => {
    const names = new Set<string>()
    for (const x of body.matchAll(/^(?:const|let)\s+([A-Za-z_$][\w$]*)/gm)) names.add(x[1]!)
    const dup = [...names].some((n) => seen.has(n))
    names.forEach((n) => seen.add(n))
    if (!dup || !body.trim()) return m
    return `<script>(() => {\n${body}\n})()</script>`
  })
}

/**
 * 组装 iframe srcdoc：在卡 HTML 的 <head>（或 <html>、文档最前）注入
 * 桥接引导脚本——必须先于卡内脚本执行，才能把 API 挂成全局函数。
 */
export function buildEmbedSrcdoc(html: string, bootstrap: string): string {
  const injected = `<script>${bootstrap}</script>`
  const fixed = fixCrossBlockDeclCollision(html)
  if (/<head[^>]*>/i.test(fixed)) return fixed.replace(/<head[^>]*>/i, (m) => `${m}${injected}`)
  if (/<html[^>]*>/i.test(fixed)) return fixed.replace(/<html[^>]*>/i, (m) => `${m}${injected}`)
  return injected + fixed
}
