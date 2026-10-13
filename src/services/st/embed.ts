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

/** 围栏 opener 带提示尾巴（如 "```yaml {必须输出```"）——info 串含反引号，
 *  CommonMark 下该 opener 无效，围栏奇偶错乱导致后续内容被吞进代码块 */
const TAINTED_FENCE = /```(\w+)[ \t]*\{[^`]*?```/g

/**
 * 卡片输出标记适配（显示通路，写卡/风月系模型通用）：
 *
 * 1. 围栏尾巴：把 "```lang {…}```" 按出现顺序交替规范化为 opener/closer
 *    （模型把"必须输出```"提示文字混进围栏标记，且常漏闭合——交替配对
 *    恰好还原其本意：思维链块开/闭）；
 * 2. <Status_block>…</Status_block>：ST 网页端靠用户自装"美化状态栏"
 *    正则替换成 HTML 界面，App 无此正则 → 转为折叠块（内部管道表格
 *    经空行中断按 markdown 渲染）；
 * 3. <maintext> 剥壳：正文本身是 markdown，去标签后正常渲染；
 * 4. 奇数围栏兜底：文末补闭合，避免吞掉尾部内容。
 */
export function adaptCardMarkup(s: string): string {
  if (!/```(\w+)[ \t]*\{[^`]*?```|<maintext>|<Status_block>/i.test(s)) return s
  let n = 0
  let out = s.replace(TAINTED_FENCE, (_m, lang: string | undefined) =>
    n++ % 2 === 0 ? '```' + (lang ?? '') : '```',
  )
  out = out.replace(
    /<Status_block>([\s\S]*?)<\/Status_block>/gi,
    (_m, inner: string) =>
      `\n<details>\n<summary>📊 状态栏</summary>\n\n${inner.trim()}\n\n</details>\n`,
  )
  out = out.replace(/<\/?maintext>/gi, '')
  const fenceCount = (out.match(/^[ \t]*`{3,}/gm) ?? []).length
  if (fenceCount % 2 === 1) out += '\n```'
  return out
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
