/**
 * 前端界面（酒馆助手语义）消息检测与 iframe srcdoc 构建。
 *
 * 卡片自带正则脚本会把占位标记替换成完整 HTML 应用（```text 包裹的
 * <!DOCTYPE html>…），宿主把它渲染进 sandbox="allow-scripts" 的 iframe：
 * 卡内脚本 / CDN 资源在沙箱内正常运行，与 ST 网页端酒馆助手的
 * "前端界面"同构；数据访问一律经 postMessage 桥（embedBridge.ts），
 * 沙箱不给 allow-same-origin，脚本无法直连 ST 后端。
 *
 * S23（批16）：帧→宿主方向的 postMessage targetOrigin 一律用宿主 origin
 * （buildEmbedSrcdoc 构造 srcdoc 时注入，见 PARENT_ORIGIN_PLACEHOLDER）；
 * 注入失败则丢弃消息并 console.warn，禁止回退 '*'。宿主→帧方向保持 '*'
 * ——沙箱帧（无 allow-same-origin）的 origin 是 opaque null，任何具体
 * origin 都不匹配会静默丢消息，安全面由接收侧 ev.source 校验 + iframe
 * sandbox 封住（P1-2 评审项结论，批16 复核后维持并登记）。
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
 * （新代码请用 splitEmbedSegments——支持 embed 与正文共存）
 */
export function extractEmbedHtml(content: string): string {
  if (!HAS_SCRIPT.test(content)) return ''
  const fenced = FENCED_DOC.exec(content)
  if (fenced) return fenced[1]!
  if (BARE_DOC.test(content)) return content.trim()
  return ''
}

/** 显示分段：embed（iframe 渲染）与文本（markdown 渲染）交替 */
export interface EmbedSegment {
  type: 'embed' | 'text'
  content: string
}

/** 围栏包裹形态 */
const FENCED_DOC_G = /```[^\n]*\n(<!DOCTYPE\s+html[\s\S]*?<\/html>)[\s]*?```/gi

/**
 * 把消息拆成 embed/文本分段（保持原始顺序）：
 * 卡片的替换正则只替换消息中的标记段（如 <SceneInfo>…</SceneInfo> →
 * HTML 界面），文档之外的正文必须继续按 markdown 渲染——
 * 此前整条消息被 iframe 独占，界面外的文字全部丢失（bug：文字消失）。
 */
export function splitEmbedSegments(content: string): EmbedSegment[] {
  if (!HAS_SCRIPT.test(content)) return [{ type: 'text', content }]
  // 整条消息就是一份裸文档 → 单 embed 段
  if (BARE_DOC.test(content)) return [{ type: 'embed', content: content.trim() }]

  const segs: EmbedSegment[] = []
  let last = 0
  for (const m of content.matchAll(FENCED_DOC_G)) {
    const idx = m.index ?? 0
    const before = content.slice(last, idx)
    if (before.trim()) segs.push({ type: 'text', content: before })
    segs.push({ type: 'embed', content: m[1] ?? '' })
    last = idx + m[0].length
  }
  const rest = content.slice(last)
  if (rest.trim()) segs.push({ type: 'text', content: rest })
  return segs.length ? segs : [{ type: 'text', content }]
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
  character_design_complex: '🎨 角色设计',
  character_design: '🎨 角色设计',
}

/**
 * [start]<tag>…</tag>[end] 结构块 → 折叠卡片。
 * 模型输出两种形态：裸块、或整块包在 ``` 围栏里（时 A 时 B 不稳定）——
 * 前后围栏行一并吃掉（可选组），统一转为 details 卡片。
 */
function foldStructBlocks(s: string): string {
  return s.replace(
    /(?:^|\n)(```[^\n]*\n)?\[start\][^\n]*\n<([\w-]+)>([\s\S]*?)<\/\2>\n\[end\][^\n]*\n?(```[ \t]*\n)?/g,
    (_m, _pre: string | undefined, tag: string, inner: string, _post: string | undefined) => {
      const label = STRUCT_LABELS[tag] ?? `📦 ${tag.replace(/_/g, ' ')}`
      return `\n<details>\n<summary>${label}</summary>\n\n\`\`\`yaml\n${inner.trim()}\n\`\`\`\n\n</details>\n`
    },
  )
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

  // [start]…[end] 结构块（裸块或围栏包裹两种形态）→ 折叠卡片
  s = foldStructBlocks(s)

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

/** S23：宿主 origin 占位符（引导脚本里带引号出现）。buildEmbedSrcdoc 在宿主
 *  侧构造 srcdoc 时替换为实际 origin 的 JSON 字面量；未替换（绕过
 *  buildEmbedSrcdoc 直接用 SANDBOX_BOOTSTRAP）或空值时，帧内 ORIGIN_OK
 *  判定失败 → 消息丢弃 + console.warn，绝不回退 '*'。 */
export const PARENT_ORIGIN_PLACEHOLDER = '__EMBED_PARENT_ORIGIN__'

/* ---------------- 帧内预置库（方案批1+批2） ----------------
 * 上游酒馆助手给每个前端帧注入一套第三方库，卡作者是按「这些全局量存在」写卡的；
 * 我们此前只注入 TavernHelper 桥，依赖 $ / Vue / jsyaml 的卡会直接 ReferenceError。
 *
 * 走本地后端只读路由 `/card-libs/<带版本文件>`，不是宿主 blob：我们的帧是
 * sandbox 无 allow-same-origin 的**不透明源**，读不到宿主创建的 blob URL
 * （实测脚本不执行、零 CSP 违规 ⇒ 源检查而非策略拦截）；上游能用 blob 是因为它
 * 的帧同源零沙箱。
 *
 * 本清单 = **可注入集**（css/js），须与 `src-tauri/src/backend/cardlibs.rs` 的
 * `injectable()` 同名同序；后端另有仅供 CSS 相对引用的附属资源（webfonts/主题图片），
 * 不可注入。三向一致由 scripts/selfcheck-cardlibs.mjs 守。
 * 版本与许可登记在 src-tauri/assets/card-libs/LICENSES.md。 */
export const CARD_LIB_FILES: readonly string[] = [
  // 批1：基础全局量
  'jquery-3.7.1.min.js',
  'toastr-2.1.4.min.css',
  'toastr-2.1.4.min.js',
  'lodash-4.17.21.min.js',
  'js-yaml-4.1.0.min.js',
  'vue-3.5.13.global.prod.js',
  'vue-router-4.5.0.global.prod.js',
  // 批2：图标、jQuery 生态、markdown 转换
  'fontawesome-6.7.2/css/all.min.css',
  'jquery-ui-1.13.3/themes/base/theme.css',
  'jquery-ui-1.13.3/jquery-ui.min.js',
  'jquery-ui-touch-punch-0.2.3.min.js',
  'showdown-2.1.0.min.js',
]

/**
 * 生成注入帧 `<head>` 的预置库标签（不带 defer/async ⇒ parser 阻塞，
 * 保证卡内脚本执行前全局量已就绪）。
 * 基地址未就绪（桥接尚未写入 stoc.base）时返回空串，绝不退化成相对路径。
 */
export function buildCardLibTags(base: string): string {
  const b = base.replace(/\/+$/, '')
  if (!b) return ''
  const out: string[] = []
  for (const f of CARD_LIB_FILES) {
    out.push(
      f.endsWith('.css')
        ? `<link rel="stylesheet" href="${b}/card-libs/${f}">`
        : `<script src="${b}/card-libs/${f}"></script>`,
    )
    // js-yaml 浏览器构建挂 window.jsyaml；上游文档以 YAML 称呼该库，补个别名（我们额外加的）
    if (f.startsWith('js-yaml')) out.push('<script>window.YAML=window.YAML||window.jsyaml;</script>')
  }
  return out.join('\n')
}

/** 宿主 origin 注入：沙箱帧是 opaque origin，自己读不到宿主源，必须由宿主
 *  构造 srcdoc 时显式告知。消息前端界面（HtmlEmbedFrame.vue）与脚本库运行时
 *  （scriptHost.ts）两个消费方都经 buildEmbedSrcdoc，注入在此单点覆盖。 */
function injectParentOrigin(bootstrap: string): string {
  const origin = typeof window !== 'undefined' && window.location ? window.location.origin : ''
  return bootstrap.split(`'${PARENT_ORIGIN_PLACEHOLDER}'`).join(JSON.stringify(origin))
}

/**
 * 组装 iframe srcdoc：在卡 HTML 的 <head>（或 <html>、文档最前）注入
 * 桥接引导脚本——必须先于卡内脚本执行，才能把 API 挂成全局函数。
 * 注入前替换宿主 origin 占位符（S23：帧→宿主 targetOrigin 收紧）。
 *
 * `headExtra`（方案批1）：额外的 <head> 注入内容，当前用于帧内预置库标签
 * （buildCardLibTags）。只有需要这套库的调用方传它——脚本库运行时帧不传。
 */
export function buildEmbedSrcdoc(html: string, bootstrap: string, headExtra = ''): string {
  // 掐掉文档级滚动条：帧高按内容精确给，只要差 1px 就会冒出纵向滚动条 →
  // 帧内可用宽度骤减约 15px → 文本重排 → 高度又变 → 滚动条消失 …… 形成
  // 「内部宽度与气泡宽度互相触发」的持续抖动（用户实测症状）。
  // 内容永远由父页调高度承接，故文档级滚动本就不需要；卡自己的内层滚动容器
  // （overflow:auto 的子元素，如弹窗正文）不受影响。
  const injected =
    `<style>html{overflow:hidden!important}</style>` +
    `<script>${injectParentOrigin(bootstrap)}</script>` +
    (headExtra ? `\n${headExtra}` : '')
  const fixed = fixCrossBlockDeclCollision(html)
  if (/<head[^>]*>/i.test(fixed)) return fixed.replace(/<head[^>]*>/i, (m) => `${m}${injected}`)
  if (/<html[^>]*>/i.test(fixed)) return fixed.replace(/<html[^>]*>/i, (m) => `${m}${injected}`)
  return injected + fixed
}

/** 沙箱引导脚本（TavernHelper shim + localStorage 垫片 + 错误/高度上报 + eventOn 事件桥），
 *  消息前端界面 iframe 与脚本库运行时共用同一份。
 *  S23：帧→宿主 postMessage 统一走 postToHost（targetOrigin = 注入的宿主 origin）。 */
export const SANDBOX_BOOTSTRAP = `
(function () {
  // S23：帧→宿主 postMessage 的 targetOrigin。占位符由 buildEmbedSrcdoc 在
  // 宿主侧构造 srcdoc 时替换为实际 origin（JSON 字面量）；ORIGIN_OK 判定
  // 失败（未替换/空值/non-origin 形态）→ 消息丢弃 + console.warn 一次，
  // 禁止回退 '*'。宿主→帧方向仍为 '*'（opaque origin 豁免，见文件头注释）。
  var PARENT_ORIGIN = '__EMBED_PARENT_ORIGIN__'
  var ORIGIN_OK = !!PARENT_ORIGIN && PARENT_ORIGIN.indexOf('//') !== -1 && PARENT_ORIGIN.charAt(0) !== '_'
  var ORIGIN_WARNED = false
  function postToHost(msg) {
    if (!ORIGIN_OK) {
      if (!ORIGIN_WARNED) {
        ORIGIN_WARNED = true
        try { console.warn('[tavern-embed] 未注入宿主 origin，帧→宿主消息已丢弃（S23 禁止回退 "*"）：', msg && msg.type) } catch (e) { /* ignore */ }
      }
      return
    }
    try { parent.postMessage(msg, PARENT_ORIGIN) } catch (e) { /* ignore */ }
  }
  function reportErr(kind, msg, where) {
    postToHost({ source: 'tavern-embed', type: 'th-error', msg: String(msg), where: String(where ?? '') })
  }
  // 沙箱无 allow-same-origin → localStorage/sessionStorage 访问必抛 SecurityError。
  // 通过宿主桥（getStorage/setStorage）双向同步到 chat_metadata.variables.__local_storage__，保证卡内小游戏/状态持久化。
  function makeStorage() {
    var store = {}
    try {
      call('getStorage', {}).then(function (s) {
        if (s && typeof s === 'object') {
          Object.keys(s).forEach(function (k) { store[k] = String(s[k]) })
        }
      }).catch(function () {})
    } catch (e) {}
    return {
      getItem: function (k) { return Object.prototype.hasOwnProperty.call(store, String(k)) ? store[String(k)] : null },
      setItem: function (k, v) {
        store[String(k)] = String(v)
        try { call('setStorage', { key: String(k), value: String(v) }).catch(function () {}) } catch (e) {}
      },
      removeItem: function (k) {
        delete store[String(k)]
        try { call('setStorage', { key: String(k), value: null }).catch(function () {}) } catch (e) {}
      },
      clear: function () {
        store = {}
      },
      key: function (i) { return Object.keys(store)[i] ?? null },
      get length() { return Object.keys(store).length },
    }
  }
  try {
    window.localStorage.getItem('__probe__')
  } catch (e) {
    try {
      Object.defineProperty(window, 'localStorage', { value: makeStorage(), configurable: true })
      Object.defineProperty(window, 'sessionStorage', { value: makeStorage(), configurable: true })
    } catch (e2) { /* 极老内核忽略 */ }
  }
  window.addEventListener('error', function (e) {
    reportErr('error', e.message, (e.filename || '') + ':' + (e.lineno ?? ''))
  })
  window.addEventListener('unhandledrejection', function (e) {
    var r = e.reason
    reportErr('promise', r && r.message ? r.message : String(r), '')
  })
  var seq = 0
  var pending = {}
  function call(api, args) {
    return new Promise(function (resolve, reject) {
      var id = ++seq
      pending[id] = { resolve: resolve, reject: reject }
      postToHost({ source: 'tavern-embed', type: 'th-call', id: id, api: api, args: args })
      window.setTimeout(function () {
        if (pending[id]) { delete pending[id]; reject(new Error('调用超时：' + api)) }
      }, 120000)
    })
  }
  window.addEventListener('message', function (ev) {
    var d = ev.data
    if (!d || d.source !== 'tavern-host' || d.type !== 'th-result') return
    var p = pending[d.id]
    if (!p) return
    delete pending[d.id]
    if (d.error) p.reject(new Error(d.error))
    else p.resolve(d.result)
  })
  var api = {
    getChatMessages: function (range) { return call('getChatMessages', { range: range }) },
    setChatMessages: function (message_id, message) { return call('setChatMessages', { message_id: message_id, message: message }) },
    getVariables: function () { return call('getVariables', {}) },
    getVariable: function (key) { return call('getVariable', { key: key }) },
    setVariable: function (key, value) { return call('setVariable', { key: key, value: value }) },
    getContext: function () { return call('getContext', {}) },
    replaceVariables: function (value) { return call('replaceVariables', { value: value }) },
    insertVariables: function (value) { return call('insertVariables', { value: value }) },
    triggerSlash: function (cmds) { return call('triggerSlash', { cmds: cmds }) },
    eventOn: function (name, listener) {
      var id = ++evSeq
      listeners[id] = { name: name, fn: listener }
      postToHost({ source: 'tavern-embed', type: 'th-event-on', id: id, name: String(name) })
      return id
    },
    eventOff: function (id) {
      delete listeners[id]
      postToHost({ source: 'tavern-embed', type: 'th-event-off', id: id })
    },
    eventEmit: function (name, payload) { return call('eventEmit', { name: name, payload: payload }) },
  }
  var evSeq = 0
  var listeners = {}
  window.event_types = {
    MESSAGE_SENT: 'message_sent',
    MESSAGE_RECEIVED: 'message_received',
    MESSAGE_EDITED: 'message_edited',
    CHAT_CHANGED: 'chat_changed',
    GENERATION_ENDED: 'generation_ended',
  }
  window.tavern_events = window.event_types
  window.addEventListener('message', function (ev) {
    var d = ev.data
    if (!d || d.source !== 'tavern-host' || d.type !== 'th-event') return
    Object.keys(listeners).forEach(function (id) {
      var l = listeners[id]
      if (l.name === d.name) {
        try { l.fn(d.payload) } catch (e) { reportErr('listener', e.message, l.name) }
      }
    })
  })
  window.TavernHelper = api
  window.SillyTavern = {
    getContext: function () { return call('getContext', {}) },
    getVariable: function (key) { return call('getVariable', { key: key }) },
    getVariables: function () { return call('getVariables', {}) },
    setVariable: function (key, value) { return call('setVariable', { key: key, value: value }) },
    replaceVariables: function (value) { return call('replaceVariables', { value: value }) },
    insertVariables: function (value) { return call('insertVariables', { value: value }) },
    triggerSlash: function (cmds) { return call('triggerSlash', { cmds: cmds }) },
  }
  Object.keys(api).forEach(function (k) { window[k] = api[k] })
  // 高度上报：必须测「内容高度」而不是 document.documentElement.scrollHeight ——
  // 后者的下界是帧自身视口高度，父页把帧调高后它永远 >= 当前高度，于是折叠/收起
  // 只会涨不会缩（实测症状：卡片折叠后气泡仍留原高度）。观察者同理必须挂 body：
  // documentElement 的盒子就是视口，内容尺寸变化不会触发它。
  var lastH = 0
  function contentHeight() {
    var b = document.body
    var d = document.documentElement
    if (!b) return 0
    var cs = null
    try { cs = window.getComputedStyle(b) } catch (e) { cs = null }
    var mb = cs ? (parseFloat(cs.marginBottom) || 0) : 0
    var bodyH = Math.ceil(b.getBoundingClientRect().height)
    var scrollH = b.scrollHeight
    var viewport = d.clientHeight
    // body 被拉伸到满视口（height:100% / min-height:100vh 之类）时上面两个测量都等于
    // 视口高度，会重新引入「只涨不缩」；此时退回子元素最大下沿（与视口解耦）。
    if (viewport > 0 && bodyH >= viewport - 1 && scrollH >= viewport - 1) {
      var maxBottom = 0
      for (var i = 0; i < b.children.length; i++) {
        var r = b.children[i].getBoundingClientRect()
        if (r.height > 0 && r.bottom > maxBottom) maxBottom = r.bottom
      }
      return Math.ceil(maxBottom + mb)
    }
    return Math.ceil(Math.max(bodyH, scrollH) + mb)
  }
  var hPending = false
  // 底部安全区：卡容器常带 margin-top（本卡 .container 是 15px），而 box-shadow
  // （如 0 8px 25px）与末元素外边距都在盒模型之外、不计入内容高度 —— 于是
  // 「按内容精确给高」会切掉底部（用户实测：顶部有留白、底部没有）。
  // 同时留 1~2px 余量，配合 html{overflow:hidden} 让帧内永远不因舍入冒出滚动条。
  var HEIGHT_GUTTER = 16
  function postHeight() {
    hPending = false
    var h = contentHeight()
    if (!h) return
    h += HEIGHT_GUTTER
    if (Math.abs(h - lastH) < 1) return
    lastH = h
    postToHost({ source: 'tavern-embed', type: 'th-resize', height: h })
  }
  function scheduleHeight() {
    if (hPending) return
    hPending = true
    // 不用 rAF 单点调度：标签页隐藏 / 窗口最小化时 rAF 会被节流甚至冻结，
    // 高度就会永远停在旧值（本仓库已两次踩到"后台标签页定时器被冻"）。
    // 微任务不受可见性影响；postHeight 自身有 1px 去重，成本可控。
    if (typeof Promise === 'function') Promise.resolve().then(postHeight)
    else window.setTimeout(postHeight, 16)
  }
  // 引导脚本注入在 <head> 内，此刻 body 还不存在 —— 观察者必须等 body 就绪再挂
  function watchHeight() {
    if (!document.body) return
    if (window.ResizeObserver) new ResizeObserver(scheduleHeight).observe(document.body)
    if (window.MutationObserver) {
      // 折叠/展开常见实现是切 class 或 display，body 尺寸可能同帧不变、RO 漏拍，
      // 故再挂一层 mutation（结构变化 + class/style/hidden/open 属性）
      new MutationObserver(scheduleHeight).observe(document.body, {
        childList: true,
        subtree: true,
        attributes: true,
        attributeFilter: ['class', 'style', 'hidden', 'open'],
      })
    }
    scheduleHeight()
  }
  window.addEventListener('load', scheduleHeight)
  window.addEventListener('resize', scheduleHeight)
  // 宿主主动索取测量：子帧首次上报可能早于宿主 message 监听就位（竞态），
  // 漏掉后就只剩"内容变化"这一个触发点；宿主在 iframe load 时会发 th-measure 补一次。
  window.addEventListener('message', function (ev) {
    var d = ev.data
    if (!d || d.source !== 'tavern-host' || d.type !== 'th-measure') return
    scheduleHeight()
  })
  // CSS 过渡/动画结束后再测一次：折叠常见实现是 max-height/height 过渡，
  // RO 可能在过渡中途拍到最后一次，过渡结束才是最终高度。
  ;['transitionend', 'animationend'].forEach(function (t) {
    document.addEventListener(t, scheduleHeight, true)
  })
  if (document.body) watchHeight()
  else document.addEventListener('DOMContentLoaded', watchHeight)
})()
`
