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

/** 围栏行形态分类（写卡系模型输出实测）：
 *  OPEN_TAINT  "```lang {必须输出```"      —— 开（带提示尾巴）
 *  CLOSE_TAINT "``` {必须输出```}"         —— 开/闭（无语言，同款尾巴；按状态判定）
 *  CLOSE_HEAD  "```## 结语…```"            —— 闭 + 结语标题
 *  OPEN/CLOSE_PLAIN                        —— 普通 ``` 围栏 */
const RE_OPEN_TAINT = /^```([a-z]+)[ \t]*\{[^`]*```[ \t]*\}?[ \t]*$/i
const RE_CLOSE_TAINT = /^```[ \t]*\{[^`]*```[ \t]*\}?[ \t]*$/i
const RE_CLOSE_HEAD = /^```##([^`]*)```[ \t]*$/i

/** [start]<tag>…</tag>[end] 结构块的折叠标签映射 */
const STRUCT_LABELS: Record<string, string> = {
  world_view: '🌍 世界观设定',
  rule_setting_complex: '📜 复杂规则',
  rule_setting_simple: '📜 简单规则',
  rule_setting: '📜 规则',
  character_sheet: '👤 角色卡',
}

/**
 * 卡片输出标记适配（显示通路，写卡/风月系模型通用）：
 *
 * 1. [start]<tag>…</tag>[end] 结构块（world_view/rule_setting_complex 等
 *    卡数据，ST 网页端靠用户自装美化正则渲染）→ 折叠卡片 + 内部 yaml 代码块；
 * 2. <Status_block> 表格 → 📊 状态栏折叠块；
 * 3. <maintext> 剥壳（正文本身是 markdown）；
 * 4. 围栏状态机：行级扫描，按形态与 in/out 状态把被污染的围栏行
 *    （"```yaml {必须输出```" / "``` {必须输出```}" / "```## …```"）
 *    还原为正常 opener/closer——修复思维链 yaml 裸奔与内容被吞；
 * 5. <user> 标签 → 「用户」；"# …生成器…" 工作流泄漏段折叠。
 */
export function adaptCardMarkup(s: string): string {
  if (!/```|<maintext>|<Status_block>|<world_view>|<rule_setting/i.test(s)) return s

  // [start]…[end] 结构块 → 折叠卡片
  s = s.replace(
    /\[start\][^\n]*\n<([\w-]+)>([\s\S]*?)<\/\1>\n\[end\][^\n]*/g,
    (_m, tag: string, inner: string) => {
      const label = STRUCT_LABELS[tag] ?? '📦 数据块'
      return `\n<details>\n<summary>${label}</summary>\n\n\`\`\`yaml\n${inner.trim()}\n\`\`\`\n\n</details>\n`
    },
  )

  // 状态栏 → 折叠块（maintext 剥壳必须在围栏状态机之后：
  // 状态机靠 <maintext> 判定"open 紧跟正文"的 closer 场景）
  s = s.replace(
    /<Status_block>([\s\S]*?)<\/Status_block>/gi,
    (_m, inner: string) =>
      `\n<details>\n<summary>📊 状态栏</summary>\n\n${inner.trim()}\n\n</details>\n`,
  )

  // 围栏状态机（行级）：inFence 跟踪开合，污染行按状态还原
  const lines = s.split('\n')
  const out: string[] = []
  let inFence = false
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!
    if (!inFence) {
      if (/^```[a-z]+/i.test(line)) {
        // open：紧随 <maintext> 的 open 实为上一块的 closer（模型漏闭合）
        let j = i + 1
        while (j < lines.length && !lines[j]!.trim()) j++
        if (lines[j]?.trim() === '<maintext>') {
          out.push('```')
          inFence = false
          continue
        }
        out.push(line.replace(RE_OPEN_TAINT, '```$1'))
        inFence = true
      } else if (RE_CLOSE_HEAD.test(line)) {
        // fence 外的闭合+结语 → 提取结语标题
        out.push('```', '', `##${line.replace(RE_CLOSE_HEAD, '$1')}`, '')
      } else if (RE_CLOSE_TAINT.test(line)) {
        // fence 外的冗余闭合行 → 删除
      } else {
        if (/^```/.test(line)) inFence = true
        out.push(line)
      }
    } else {
      // fence 内的 OPEN_TAINT：模型在未闭合块里又开了新块（漏闭合）
      // → 闭合当前块；若下一非空行是 <maintext> 则只闭合，否则拆为新块
      const ot = RE_OPEN_TAINT.exec(line)
      if (ot) {
        let j = i + 1
        while (j < lines.length && !lines[j]!.trim()) j++
        const nextIsMaintext = lines[j]?.trim() === '<maintext>'
        out.push('```')
        inFence = false
        if (!nextIsMaintext) {
          out.push(`\`\`\`${ot[1] ?? ''}`)
          inFence = true
        }
        continue
      }
      if (
        RE_CLOSE_TAINT.test(line) ||
        /^```[ \t]*$/.test(line) ||
        RE_CLOSE_HEAD.test(line)
      ) {
        if (RE_CLOSE_HEAD.test(line)) {
          out.push('```', '', `##${line.replace(RE_CLOSE_HEAD, '$1')}`, '')
        } else {
          out.push('```')
        }
        inFence = false
      } else {
        out.push(line) // 内容行原样
      }
    }
  }
  if (inFence) out.push('```')
  s = out.join('\n')

  // maintext 剥壳（状态机之后：正文交 markdown 渲染）
  s = s.replace(/<\/?maintext>/gi, '')

  // 模型泄漏的工作流/格式说明段（"# …生成器…" 至下一结构前）→ 折叠
  s = s.replace(
    /(^|\n)(#[^\n]*生成器[^\n]*\n(?:(?!<details>)[\s\S])*?)(?=\n<details>|\n## |$)/g,
    (_m, pre: string, bodyText: string) =>
      `${pre}<details>\n<summary>📋 生成器工作流说明（模型泄漏的格式模板，仅供参考）</summary>\n\n\`\`\`\`\n${bodyText.trimEnd()}\n\`\`\`\`\n\n</details>\n`,
  )

  // <user> 标签 → 「用户」
  s = s.replace(/<\/?user>/gi, '用户')
  return s
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
