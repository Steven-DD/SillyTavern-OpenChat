#!/usr/bin/env node
import assert from 'node:assert/strict'
import { parseScript, runScript, SCRIPT_COMMAND_NAMES } from '../src/services/st/stscript'
import { eventBus } from '../src/services/st/eventBus'

console.log('  — STscript 扩展命令集 (40+) 与 EventBus 门禁自检 —')

// 1. 命令总数断言 >= 40
console.log(`已注册命令数量: ${SCRIPT_COMMAND_NAMES.length}`)
assert.ok(
  SCRIPT_COMMAND_NAMES.length >= 40,
  `命令集数量不足 40: 当前为 ${SCRIPT_COMMAND_NAMES.length}`
)

// 创建测试用上下文
const mockVars = new Map()
const memoryStore = {
  get: (k) => mockVars.get(k),
  set: (k, v) => mockVars.set(k, v),
  has: (k) => mockVars.has(k),
  delete: (k) => mockVars.delete(k),
}

const mockCtx = {
  vars: memoryStore,
  expand: (text) => text.replace(/\{\{getvar::([\w:-]+)\}\}/g, (_, k) => String(mockVars.get(k) ?? '')),
  send: async () => {},
  genraw: async () => 'genraw_output',
  echo: () => {},
}

// 2. /roll 掷骰运算断言
const r1 = await runScript('/roll 1d20', mockCtx)
const n1 = Number(r1)
assert.ok(n1 >= 1 && n1 <= 20, `/roll 1d20 结果超限: ${n1}`)

const r2 = await runScript('/roll 2d6+5', mockCtx)
const n2 = Number(r2)
assert.ok(n2 >= 7 && n2 <= 17, `/roll 2d6+5 结果超限: ${n2}`)

// 3. /math 算术计算断言
const rMath = await runScript('/math 2 + 3 * 4', mockCtx)
assert.equal(rMath, '14', `/math 2 + 3 * 4 结果应为 14: 当前为 ${rMath}`)

// 4. 字符串工具命令断言
assert.equal(await runScript('/len hello world', mockCtx), '11', '/len 计算正确')
assert.equal(await runScript('/upper abc', mockCtx), 'ABC', '/upper 转换正确')
assert.equal(await runScript('/lower XYZ', mockCtx), 'xyz', '/lower 转换正确')
assert.equal(await runScript('/trim   foo bar   ', mockCtx), 'foo bar', '/trim 处理正确')
assert.equal(await runScript('/replace a=b a-a-a', mockCtx), 'b-b-b', '/replace 处理正确')
assert.equal(await runScript('/slice 1,4 hello', mockCtx), 'ell', '/slice 处理正确')
assert.equal(await runScript('/split :,1 a:b:c', mockCtx), 'b', '/split 处理正确')
assert.equal(await runScript('/min 5,2,9,1,7', mockCtx), '1', '/min 计算正确')
assert.equal(await runScript('/max 5,2,9,1,7', mockCtx), '9', '/max 计算正确')
assert.equal(await runScript('/abs -42', mockCtx), '42', '/abs 计算正确')

// 5. 逻辑与变量命令断言
assert.equal(await runScript('/and 1 1', mockCtx), 'true', '/and 1 1')
assert.equal(await runScript('/and 1 0', mockCtx), 'false', '/and 1 0')
assert.equal(await runScript('/or 0 1', mockCtx), 'true', '/or 0 1')
assert.equal(await runScript('/not 1', mockCtx), 'false', '/not 1')

await runScript('/setvar hp=100', mockCtx)
assert.equal(mockVars.get('hp'), '100', '/setvar')
assert.equal(await runScript('/hasvar hp', mockCtx), 'true', '/hasvar')

await runScript('/incvar hp', mockCtx)
assert.equal(mockVars.get('hp'), 101, '/incvar')

await runScript('/decvar hp', mockCtx)
assert.equal(mockVars.get('hp'), 100, '/decvar')

// 6. /return 与流控
const rReturn = await runScript('/setvar stage=1 | /return early_exit | /setvar stage=2', mockCtx)
assert.equal(rReturn, 'early_exit', '/return 返回管道值')
assert.equal(mockVars.get('stage'), '1', '/return 中断后续段执行')

// 7. /while 循环断言
mockVars.set('counter', 5)
await runScript('/while {{getvar::counter}} > 0 | /decvar counter', mockCtx)
assert.equal(mockVars.get('counter'), 0, '/while 循环正常结束并在 counter=0 时退出')

// 8. EventBus 生命周期总线断言
let sendHookFired = false
eventBus.on('before_message_send', (payload) => {
  sendHookFired = true
  payload.text = payload.text + ' [modified]'
})

const pipeRes = await eventBus.pipeline('before_message_send', {
  text: 'Hello',
})
assert.ok(sendHookFired, 'before_message_send 成功触发')
assert.equal(pipeRes.text, 'Hello [modified]', '流水线对文本成功进行就地改写')

eventBus.clear()

console.log('✓ STscript 扩展命令集 (40+) 与 EventBus 全部自检通过')
