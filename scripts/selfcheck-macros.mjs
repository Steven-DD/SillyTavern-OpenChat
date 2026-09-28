/**
 * 宏引擎 v2 自检（回归脚本）
 * 运行：npx esbuild scripts/selfcheck-macros.mjs --bundle --format=esm --platform=node --outfile=node_modules/.tmp/macros.mjs && node node_modules/.tmp/macros.mjs
 * 按类别断言，对齐 ST 宏定义语义
 */
globalThis.document = { createElement: () => ({}) }
globalThis.localStorage = {
  _d: {},
  getItem(k) { return this._d[k] ?? null },
  setItem(k, v) { this._d[k] = String(v) },
  removeItem(k) { delete this._d[k] },
}

const { runMacros, expandIfBlocks, emptyContext, getChatVarStore, getGlobalVarStore } = await import(
  '../src/services/st/macros.ts'
)

let fail = 0
let n = 0
function ok(cond, name) {
  n++
  if (!cond) { fail++; console.error(`  ✗ ${name}`) }
}
function eq(actual, expected, name) {
  n++
  if (actual !== expected) { fail++; console.error(`  ✗ ${name}\n    期望 ${JSON.stringify(expected)} 实得 ${JSON.stringify(actual)}`) }
}

const ctx = (over = {}) => emptyContext({
  charName: '爱丽丝', userName: '老板',
  persona: '我是老板的助手', charDescription: 'DESC', charPersonality: 'PERS',
  charScenario: 'SCEN', charCreatorNotes: 'NOTES', charFirstMessage: 'HELLO',
  charVersion: '2.0', mesExamples: 'EX1', model: 'test-model',
  maxContext: 8192, maxResponse: 512,
  history: [
    { role: 'user', content: 'U1' },
    { role: 'assistant', content: 'A1' },
    { role: 'user', content: 'U2' },
    { role: 'assistant', content: 'A2' },
  ],
  ...over,
})
const R = (t, over) => runMacros(t, ctx(over))

/* ---- 基础与 env ---- */
eq(R('{{char}}爱{{user}}'), '爱丽丝爱老板', 'env: char/user')
eq(R('{{CHAR}}'), '爱丽丝', 'env: 大小写不敏感')
eq(R('{{charDescription}}/{{charPersonality}}/{{charScenario}}'), 'DESC/PERS/SCEN', 'env: 卡字段')
eq(R('{{persona}}'), '我是老板的助手', 'env: persona')
eq(R('{{charCreatorNotes}}{{charFirstMessage}}{{charVersion}}'), 'NOTESHELLO2.0', 'env: 备注/开场白/版本')
eq(R('{{mesExamples}}'), 'EX1', 'env: 示例')
eq(R('{{model}}'), 'test-model', 'env: model')
eq(R('{{maxContext}} {{maxResponse}} {{maxPrompt}}'), '8192 512 8192', 'core: 上限三兄弟')
eq(R('{{isMobile}}'), 'false', 'env: isMobile')

/* ---- chat ---- */
eq(R('{{lastMessage}}'), 'A2', 'chat: lastMessage')
eq(R('{{lastUserMessage}}'), 'U2', 'chat: lastUserMessage')
eq(R('{{lastCharMessage}}'), 'A2', 'chat: lastCharMessage')
eq(R('{{lastMessageId}}'), '3', 'chat: lastMessageId')
ok(/^\d+$/.test(R('{{lastSwipeId}}')) && /^\d+$/.test(R('{{currentSwipeId}}')), 'chat: swipe id 数字')

/* ---- core ---- */
eq(R('a{{space}}b'), 'a b', 'core: space')
eq(R('a{{newline}}b'), 'a\nb', 'core: newline')
eq(R('{{newline::3}}x'), '\n\n\nx', 'core: newline count')
eq(R('{{trim::  hi  }}'), 'hi', 'core: trim 带参')
eq(R('{{reverse::abc}}'), 'cba', 'core: reverse')
eq(R('{{noop::x}}'), '', 'core: noop 返回空（ST 语义）')
eq(R('前{{// 注释}}后'), '前后', 'core: 注释删除')
eq(R('{{unknownMacro}}'), '{{unknownMacro}}', 'core: 未注册宏保留')
eq(R('{{banned::delve}}x'), 'x', 'core: banned 返回空')
ok(ctx().bannedWords.length === 0 && R('{{banned::delve}}') === '' && true, 'core: banned 不炸')

/* ---- time ---- */
ok(/^\d{2}:\d{2}$/.test(R('{{time}}')), 'time: HH:mm')
ok(/^\d{4}年\d{1,2}月\d{1,2}日$/.test(R('{{date}}')), 'date: 中文长格式')
ok(/^(星期|周)[日一二三四五六]$/.test(R('{{weekday}}')), 'weekday: 中文星期')
ok(/^\d{4}-\d{2}-\d{2}$/.test(R('{{isodate}}')), 'isodate: YYYY-MM-DD')
ok(/^\d{2}:\d{2}$/.test(R('{{isotime}}')), 'isotime')
ok(R('{{datetimeformat::YYYY/MM/DD HH:mm}}').includes('/'), 'datetimeformat: 自定义格式')
eq(R('{{idleDuration}}{{timeDiff::a::b}}'), '', 'time: 空宏')

/* ---- random / pick / roll ---- */
ok(['甲', '乙', '丙'].includes(R('{{random::甲::乙::丙}}')), 'random: :: 语法')
ok(['甲', '乙', '丙'].includes(R('{{random:甲,乙,丙}}')), 'random: 冒号语法')
ok(['甲', '乙', '丙'].includes(R('{{random 甲,乙,丙}}')), 'random: 空格逗号语法')
{
  const a = R('{{pick::甲::乙::丙}}')
  const b = R('{{pick::甲::乙::丙}}')
  eq(a, b, 'pick: 同文本稳定')
  ok(['甲', '乙', '丙'].includes(a), 'pick: 取值在列表内')
}
{
  const r = R('{{roll::d20}}')
  ok(/^\d+$/.test(r) && +r >= 1 && +r <= 20, 'roll: d20')
  const r2 = R('{{roll::3d6+4}}')
  ok(/^\d+$/.test(r2) && +r2 >= 7 && +r2 <= 22, 'roll: 3d6+4 范围')
  const r3 = R('{{roll::50}}')
  ok(/^\d+$/.test(r3) && +r3 >= 1 && +r3 <= 50, 'roll: 裸数字=1dX')
  eq(R('{{roll::bad}}'), '', 'roll: 非法公式返回空')
}

/* ---- if / 块级 ---- */
eq(R('{{if::yes::真}}'), '真', 'if: 行内真')
eq(R('{{if::::真}}'), '', 'if: 行内空条件')
eq(R('{{if::!yes::真}}'), '', 'if: 取反')
eq(expandIfBlocks('{{if::1}}是{{else}}否{{/if}}', ctx()), '是', 'if: 块级 then')
eq(expandIfBlocks('{{if::}}是{{else}}否{{/if}}', ctx()), '否', 'if: 块级 else')
eq(expandIfBlocks('{{if::!x}}非{{else}}正{{/if}}', ctx()), '正', 'if: 块级取反')
eq(R('{{if::yes::A}}{{if::!yes::B}}'), 'A', 'if: 连用')

/* ---- 变量（chat 级） ---- */
{
  const c = ctx({
    chatVars: getChatVarStore('selfcheck::chat'),
    globalVars: getGlobalVarStore(),
  })
  eq(runMacros('{{setvar::hp::100}}', c), '', 'var: setvar 返回空')
  eq(runMacros('{{getvar::hp}}', c), '100', 'var: getvar 读回')
  runMacros('{{addvar::hp::-30}}', c)
  eq(runMacros('{{getvar::hp}}', c), '70', 'var: addvar 负数')
  runMacros('{{incvar::hp}}', c)
  eq(runMacros('{{getvar::hp}}', c), '71', 'var: incvar')
  runMacros('{{decvar::hp}}', c)
  eq(runMacros('{{getvar::hp}}', c), '70', 'var: decvar')
  eq(runMacros('{{hasvar::hp}}{{hasvar::nope}}', c), 'true', 'var: hasvar')
  runMacros('{{setvarkey::card::name::莉莉}}', c)
  eq(runMacros('{{getvarkey::card::name}}', c), '莉莉', 'var: setvarkey/getvarkey')
  runMacros('{{deletevar::hp}}', c)
  eq(runMacros('{{getvar::hp}}', c), '', 'var: deletevar')
  eq(runMacros('{{if::.hp}}有{{else}}无{{/if}}', c), '无', 'var: 条件简写 .var')
  runMacros('{{setvar::hp::5}}', c)
  eq(runMacros('{{if::.hp}}有{{else}}无{{/if}}', c), '有', 'var: 条件简写非零')
}
/* ---- 变量（global 级） ---- */
{
  const c = ctx({
    chatVars: getChatVarStore('selfcheck::chat'),
    globalVars: getGlobalVarStore(),
  })
  runMacros('{{setglobalvar::gold::999}}', c)
  eq(runMacros('{{getglobalvar::gold}}', c), '999', 'gvar: set/get')
  eq(runMacros('{{hasglobalvar::gold}}', c), 'true', 'gvar: has')
  eq(runMacros('{{if::$gold}}富{{else}}穷{{/if}}', c), '富', 'gvar: $ 简写')
}
/* ---- 持久化：chat 变量跨实例（同 localStorage） ---- */
{
  const store = getChatVarStore('sess::a')
  store.set('k', 'v1')
  const store2 = getChatVarStore('sess::a')
  eq(store2.get('k'), 'v1', 'var: localStorage 持久')
  const g1 = getGlobalVarStore()
  g1.set('gk', 'gv')
  eq(getGlobalVarStore().get('gk'), 'gv', 'gvar: localStorage 持久')
}

/* ---- 嵌套 ---- */
eq(R('{{trim::  {{char}}  }}'), '爱丽丝', '嵌套: trim(char)')
eq(R('{{if::{{char}}::有角色}}'), '有角色', '嵌套: 条件内宏')

console.log(fail ? `\n${fail} / ${n} 失败` : `\n全部通过（${n} 项）`)
process.exit(fail ? 1 : 0)
