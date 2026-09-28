/**
 * B1 批次自检（回归脚本；用法同 selfcheck-macros.mjs）
 * 覆盖：withContinue / withImpersonate / 背景图 URL
 */
globalThis.localStorage = { getItem: () => null, setItem: () => {}, removeItem: () => {} }
const { withContinue, withImpersonate } = await import('../src/services/st/prompt.ts')
const { backgroundUrl } = await import('../src/services/st/backgrounds.ts')

let fail = 0
const eq = (a, e, n) => {
  if (JSON.stringify(a) !== JSON.stringify(e)) { fail++; console.error(`✗ ${n}\n  期望 ${JSON.stringify(e)} 实得 ${JSON.stringify(a)}`) }
}

const msgs = [
  { role: 'system', content: 'SYS' },
  { role: 'user', content: '你好' },
  { role: 'assistant', content: '你好呀，今天' },
]

/* withContinue：尾部追加 [Continue] 指令，原消息不动 */
const c = withContinue(msgs)
eq(c.length, msgs.length + 1, 'continue: 长度 +1')
eq(c[c.length - 1].content, '[Continue]', 'continue: 指令内容')
eq(c[c.length - 1].role, 'user', 'continue: 指令角色 user')
eq(msgs[msgs.length - 1].content, '你好呀，今天', 'continue: 原数组不被修改')

/* withImpersonate：追加代写指令，含用户名 */
const i = withImpersonate(msgs, '老板')
eq(i.length, msgs.length + 1, 'impersonate: 长度 +1')
eq(i[i.length - 1].role, 'user', 'impersonate: 指令角色 user')
if (!i[i.length - 1].content.includes('老板')) { fail++; console.error('✗ impersonate: 指令包含用户名') }

/* 背景图 URL */
eq(backgroundUrl('a b.png'), /\/backgrounds\/a%20b\.png$/.test(backgroundUrl('a b.png')) ? backgroundUrl('a b.png') : '', 'bg: 编码路径')
if (!backgroundUrl('森林.jpg').includes('backgrounds/%E6%A3%AE%E6%9E%97.jpg')) { fail++; console.error('✗ bg: 中文文件名编码') }

console.log(fail ? `${fail} 失败` : '全部通过')
process.exit(fail ? 1 : 0)
