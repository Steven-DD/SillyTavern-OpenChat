/**
 * 卡片渲染通路自检（回归：2026-10-19「卡片自带 HTML 前端界面渲染成裸文本」）
 *
 * 覆盖此前零门禁的显示通路三段：
 *   ① 卡内嵌正则（Scoped Regex）把占位标记替换成完整 HTML 应用
 *   ② adaptCardMarkup：卡片输出标记 → 折叠卡片（含 📜 规则 / 📊 状态栏）
 *   ③ splitEmbedSegments：HTML 应用段与正文段共存
 *
 * 症状回顾：卡里那条 findRegex=`\[萧谴写卡小助手\]` 的显示正则被 /edit 丢掉后，
 * `[萧谴写卡小助手]` 就只剩裸文本 —— 数据侧由 cargo 测试守（compat/chara_card.rs、
 * backend/characters.rs），这里守渲染侧不再退化。
 */
const { applyDisplayRegex, setScopedRegexScripts, setActiveScopedAvatar, newRegexScript, REGEX_PLACEMENT } =
  await import('../src/services/st/regex.ts')
const { adaptCardMarkup, splitEmbedSegments } = await import('../src/services/st/embed.ts')
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
// run-selfchecks 恒以仓库根为 cwd（打包产物的 import.meta.url 不可用作基准）
const repo = process.cwd()

let fail = 0
const ok = (c, n) => {
  if (!c) {
    fail++
    console.error(`✗ ${n}`)
  }
}
const eq = (a, e, n) => {
  if (a !== e) {
    fail++
    console.error(`✗ ${n}\n  期望 ${JSON.stringify(e)} 实得 ${JSON.stringify(a)}`)
  }
}

/* 卡内嵌正则实测形态：```text 围栏 + 完整文档 + <script>（→ 走 iframe 沙箱） */
const HTML_APP = [
  '```text',
  '<!DOCTYPE html>',
  '<html><head><script>window.__cardApp = 1;<\/script></head>',
  '<body><div id="ui">卡片前端界面</div></body></html>',
  '```',
].join('\n')

const cardRule = {
  ...newRegexScript(),
  scriptName: '萧谴写卡小助手',
  findRegex: '\\[萧谴写卡小助手\\]',
  replaceString: HTML_APP,
  placement: [1, 2],
  markdownOnly: true, // 实测：卡内嵌显示正则正是 markdownOnly=true
  promptOnly: false,
  disabled: false,
}

/* ① 卡内嵌正则：占位标记 → HTML 应用 */
{
  const avatar = 'selfcheck-cardrender.png'
  setScopedRegexScripts(avatar, [cardRule])
  setActiveScopedAvatar(avatar)

  const out = applyDisplayRegex('[萧谴写卡小助手]', REGEX_PLACEMENT.AI_OUTPUT)
  ok(!out.includes('[萧谴写卡小助手]'), '① 显示通路命中卡内嵌正则：占位标记已被替换')
  ok(out.includes('<!DOCTYPE html>'), '① 替换结果含完整 HTML 文档')
  ok(out.includes('卡片前端界面'), '① 替换结果含卡内界面内容')

  const segs = splitEmbedSegments(out)
  eq(segs.length, 1, '① 单段：整条消息为 embed 段')
  eq(segs[0].type, 'embed', '① 该段类型为 embed（走 iframe 沙箱）')
  ok(segs[0].content.includes('<script>'), '① embed 段保留卡内 <script>')

  // 作用域切走 → 规则不再生效（防串场）
  setActiveScopedAvatar('')
  eq(
    applyDisplayRegex('[萧谴写卡小助手]', REGEX_PLACEMENT.AI_OUTPUT),
    '[萧谴写卡小助手]',
    '① 非当前角色时不套用其卡内嵌正则',
  )
}

/* ② 卡片输出标记 → 折叠卡片（特殊卡片与规则） */
{
  const ruleBlock = [
    '[start]',
    '<rule_setting_complex>',
    'key: value',
    '</rule_setting_complex>',
    '[end]',
  ].join('\n')
  const folded = adaptCardMarkup(ruleBlock)
  ok(folded.includes('<details>'), '② [start]…[/end] 结构块折成 details 卡片')
  ok(folded.includes('📜 复杂规则'), '② 规则块标签为「📜 复杂规则」')
  ok(!folded.includes('[start]'), '② 折叠后不再残留 [start] 原文')

  const status = adaptCardMarkup('<Status_block>\nHP: 100\n</Status_block>')
  ok(status.includes('📊 状态栏'), '② <Status_block> 折成「📊 状态栏」')
  ok(!status.includes('<Status_block>'), '② 折叠后不再残留 <Status_block> 原文')

  const main = adaptCardMarkup('<maintext>正文 **markdown**</maintext>')
  ok(!main.includes('<maintext>'), '② <maintext> 剥壳（正文交 markdown）')
  ok(main.includes('正文 **markdown**'), '② 剥壳保留正文内容')
}

/* ③ HTML 应用与正文共存：必须分成 embed + text 两段（此前整条被 iframe 独占，正文丢失） */
{
  const mixed = `开场正文\n\n${HTML_APP}\n\n结尾正文`
  const segs = splitEmbedSegments(mixed)
  eq(segs.filter((s) => s.type === 'embed').length, 1, '③ 共存时恰好 1 个 embed 段')
  eq(segs.filter((s) => s.type === 'text').length, 2, '③ 共存时正文分两段，不被 iframe 吞掉')
  ok(segs.some((s) => s.type === 'text' && s.content.includes('开场正文')), '③ 前段正文保留')
  ok(segs.some((s) => s.type === 'text' && s.content.includes('结尾正文')), '③ 后段正文保留')

  // 无 <script> 的纯静态 HTML 不值得走 iframe → 全部按文本渲染
  const staticOnly = '```text\n<!DOCTYPE html>\n<html><body>静态</body></html>\n```'
  const segs2 = splitEmbedSegments(staticOnly)
  eq(segs2.length, 1, '③ 无脚本的静态文档不切 embed 段')
  eq(segs2[0].type, 'text', '③ 静态文档走 markdown 文本段')
}

/* ④ 尺寸契约（防回归）：
 *   a) 高度上报要有「多点触发」—— 旧实现只靠 documentElement 的盒子变化，任何一次漏拍
 *      （首个上报早于宿主监听就位、rAF 在隐藏/最小化时被冻结、过渡结束时刻）都会让高度
 *      永久停在旧值；故要求：内容高度测量 + body 观察者 + mutation 观察者 + 微任务调度。
 *      （注：并未证实「documentElement.scrollHeight 的视口下界必然导致只涨不缩」，此处
 *       守的是触发面与测量口径，不是某个已确证的根因。）
 *   b) 含前端界面的气泡必须放开到整行宽（卡 HTML 多为 max-width:1200px;margin:0 auto
 *      的响应式布局，被文本气泡的 78% 约束会压窄）。 */
{
  const { SANDBOX_BOOTSTRAP } = await import('../src/services/st/embed.ts')
  ok(typeof SANDBOX_BOOTSTRAP === 'string' && SANDBOX_BOOTSTRAP.length > 0, '④ 引导脚本存在')
  ok(SANDBOX_BOOTSTRAP.includes('contentHeight'), '④ 高度上报走 contentHeight（内容高度）')
  ok(
    SANDBOX_BOOTSTRAP.includes('getBoundingClientRect'),
    '④ 高度测量用 body 的布局盒（与视口高度解耦）',
  )
  ok(
    !SANDBOX_BOOTSTRAP.replace(/\/\/[^\n]*/g, '').includes('documentElement.scrollHeight'),
    '④ 不得回退到 documentElement.scrollHeight（下界=视口高度，只涨不缩）',
  )
  ok(SANDBOX_BOOTSTRAP.includes('MutationObserver'), '④ 折叠/展开（切 class/display）也要能触发重测')
  ok(
    /if \(document\.body\) watchHeight\(\)/.test(SANDBOX_BOOTSTRAP),
    '④ body 未就绪时等待 DOMContentLoaded 再挂观察者（引导脚本注入在 <head>）',
  )

  const bubble = readFileSync(join(repo, 'src/components/MessageBubble.vue'), 'utf8')
  ok(/has-embed': hasEmbed/.test(bubble), '④ 气泡按 has-embed 标记')
  // 口径（2026-10-19 用户定）：普通消息与卡片消息统一把行宽上限提到 90%；
  // 卡片消息另外需要 flex:1「撑满到上限」—— iframe 是 replaced element，
  // width:100% 没有确定父宽会退化成约 300px 的固有宽度。
  ok(
    /\.wrap \{[\s\S]{0,200}max-width: 90%/.test(bubble),
    '④ 气泡行宽上限统一为 90%（普通消息与卡片消息同口径）',
  )
  ok(
    /\.msg\.has-embed \.wrap \{[\s\S]{0,120}flex: 1/.test(bubble),
    '④ 卡片消息撑满到上限（否则 iframe 退化成固有宽度）',
  )

  // 抖动与底部裁切（用户实测）：
  //   ① 帧高按内容精确给 → 差 1px 就冒帧内纵向滚动条 → 帧内可用宽度骤减约 15px →
  //      文本重排 → 高度再变 → 滚动条消失 …… 「内部宽度与气泡宽度互相触发」持续抖动；
  //   ② 盒模型之外的 box-shadow / 末元素外边距不计入内容高度 → 底部被切
  //      （顶部有 margin-top 留白、底部没有）。
  const { buildEmbedSrcdoc } = await import('../src/services/st/embed.ts')
  const sample = buildEmbedSrcdoc(
    '<!DOCTYPE html><html><head><meta charset="utf-8"></head><body>x</body></html>',
    SANDBOX_BOOTSTRAP,
  )
  ok(
    sample.includes('html{overflow:hidden!important}'),
    '④ 帧文档级滚动条被禁用（否则宽度/高度互相触发抖动）',
  )
  ok(
    SANDBOX_BOOTSTRAP.includes('HEIGHT_GUTTER'),
    '④ 上报高度带底部安全区（盒模型外的阴影/外边距不被切）',
  )
  ok(
    SANDBOX_BOOTSTRAP.includes('Promise.resolve().then(postHeight)'),
    '④ 测量调度用微任务（rAF 在隐藏/最小化标签页会被冻结）',
  )
}

console.log(fail ? `${fail} 失败` : '卡片渲染通路全部通过')
process.exit(fail ? 1 : 0)
