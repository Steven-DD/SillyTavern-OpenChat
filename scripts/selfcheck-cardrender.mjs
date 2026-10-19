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

console.log(fail ? `${fail} 失败` : '卡片渲染通路全部通过')
process.exit(fail ? 1 : 0)
