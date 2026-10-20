#!/usr/bin/env node
/**
 * selfcheck-lua-sandbox.mjs —— 自动化自检：Lua 沙箱安全性、Risu 宿主桥与生命周期钩子
 */
import assert from 'node:assert/strict'
import {
  runLuaScript,
  runLuaOnPrompt,
  runLuaOnMessage,
} from '../src/services/st/luaSandbox'

console.log('=== selfcheck-lua-sandbox: 开始 Lua 沙箱自检 ===')

// 准备测试上下文
const testVars = new Map([
  ['affinity', 10],
  ['player_name', 'Hero'],
])
let varChangeCount = 0

const ctx = {
  characterName: 'Alice',
  userName: 'Bob',
  variables: testVars,
  messages: [
    { role: 'user', content: 'Hello Alice!' },
    { role: 'assistant', content: 'Hello Bob, nice to meet you.' },
  ],
  onVarChange: () => {
    varChangeCount++
  },
}

// 1. 基础算术与变量读写自检
const basicScript = `
  local cur = risu.getVar("affinity", 0)
  risu.setVar("affinity", cur + 5)
  risu.setVar("stage", "intro")
  return cur
`
const res1 = runLuaScript(basicScript, ctx)
assert.equal(res1.success, true, '基础脚本应执行成功')
assert.equal(res1.result, 10, '脚本返回值应为原始 affinity 10')
assert.equal(testVars.get('affinity'), 15, 'affinity 应被更新为 15')
assert.equal(testVars.get('stage'), 'intro', 'stage 变量应被存入')
assert.ok(varChangeCount >= 2, '变量变更回调应触发')
console.log('✓ 1. 基础算术与 risu.getVar / setVar 断言通过')

// 2. 掷骰与上下文只读查询自检
const diceScript = `
  local d = risu.rollDice("1d20+5")
  local char = risu.getCharacterName()
  local user = risu.getUserName()
  local last = risu.getLastMessage()
  local len = risu.getChatLength()
  local first = risu.getMessage(1)
  assert(d >= 6 and d <= 25, "骰子范围需在 6~25 之间")
  assert(char == "Alice", "角色名应为 Alice")
  assert(user == "Bob", "用户名应为 Bob")
  assert(len == 2, "消息长度应为 2")
  assert(first == "Hello Alice!", "首条消息内容断言")
  return d
`
const res2 = runLuaScript(diceScript, ctx)
assert.equal(res2.success, true, '掷骰与上下文查询应成功')
assert.ok(typeof res2.result === 'number' && res2.result >= 6, '掷骰结果应有效')
console.log('✓ 2. risu.rollDice 与上下文查询断言通过')

// 3. 安全沙箱隔离断言（危险全局变量必须被剔除）
const dangerousScript = `
  assert(os == nil, "os 库必须为 nil")
  assert(io == nil, "io 库必须为 nil")
  assert(package == nil, "package 库必须为 nil")
  assert(dofile == nil, "dofile 必须为 nil")
  assert(loadfile == nil, "loadfile 必须为 nil")
  return "safe"
`
const res3 = runLuaScript(dangerousScript, ctx)
assert.equal(res3.success, true, '危险模块隔离断言脚本应成功')
assert.equal(res3.result, 'safe')
console.log('✓ 3. os / io / package 危险库沙箱隔离断言通过')

// 4. 指令步数上限防死循环断言
const infiniteLoopScript = `
  while true do
    -- 死循环测试
  end
`
const startTime = Date.now()
const res4 = runLuaScript(infiniteLoopScript, ctx)
const elapsed = Date.now() - startTime
assert.equal(res4.success, false, '死循环应被安全打断')
assert.ok(res4.error?.includes('配额上限') || res4.error?.includes('timeout'), '错误信息应包含配额上限')
assert.ok(elapsed < 2000, '应在 2 秒内安全中止，不卡死进程')
console.log(`✓ 4. 死循环硬上限防护断言通过 (耗时 ${elapsed}ms)`)

// 5. onPrompt 生命周期钩子测试
const promptHookScript = `
  function onPrompt(info)
    local aff = risu.getVar("affinity", 0)
    return "【当前好感度: " .. tostring(aff) .. "】"
  end
`
const injectedPrompt = runLuaOnPrompt(promptHookScript, ctx)
assert.ok(injectedPrompt.includes('【当前好感度: 15】'), 'onPrompt 应正确追加系统提示词')
console.log('✓ 5. onPrompt 生命周期钩子断言通过')

// 6. onMessage 生命周期钩子测试
const messageHookScript = `
  function onMessage(msg, role)
    if role == "assistant" then
      risu.setVar("reply_received", true)
    end
  end
`
runLuaOnMessage(messageHookScript, ctx, 'Thanks Bob!', 'assistant')
assert.equal(testVars.get('reply_received'), true, 'onMessage 应更新状态')
console.log('✓ 6. onMessage 生命周期钩子断言通过')

console.log('=== selfcheck-lua-sandbox: 全部通过 ===')
