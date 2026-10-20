#!/usr/bin/env node
import assert from 'node:assert/strict'
import { formatInstructChat } from '../src/services/st/instruct'
import { INSTRUCT_PRESETS } from '../src/services/st/instructPresets'

console.log('  — instruct presets 格式化与 Stop Sequences 断言 —')

const sample = [
  { role: 'system', content: 'You are helpful.' },
  { role: 'user', content: 'Hello!' },
  { role: 'assistant', content: 'Hi there!' },
  { role: 'user', content: 'How are you?' },
]

// 1. 测试全部预设都能成功执行且不抛异常
for (const p of INSTRUCT_PRESETS) {
  const r = formatInstructChat(sample, p.settings, 'User', 'Character', {})
  assert.ok(r.text.length > 0, `${p.name} 格式化文本非空`)
}

// 2. Llama 3 格式精确断言
const llama3 = INSTRUCT_PRESETS.find((p) => p.id === 'llama3')
assert.ok(llama3, '存在 llama3 预设')
const rLlama = formatInstructChat(sample, llama3.settings, 'User', 'Character', {})
assert.ok(rLlama.text.includes('<|start_header_id|>system<|end_header_id|>\n\nYou are helpful.<|eot_id|>'), '包含 system 标记')
assert.ok(rLlama.text.includes('<|start_header_id|>user<|end_header_id|>\n\nHello!<|eot_id|>'), '包含 user 标记')
assert.ok(rLlama.text.includes('<|start_header_id|>assistant<|end_header_id|>\n\nHi there!<|eot_id|>'), '包含 assistant 标记')
assert.ok(rLlama.text.endsWith('<|start_header_id|>assistant<|end_header_id|>\n\n'), '末尾包含 assistant 诱导接续')

// 3. ChatML 格式精确断言
const chatml = INSTRUCT_PRESETS.find((p) => p.id === 'chatml')
assert.ok(chatml, '存在 chatml 预设')
const rChatml = formatInstructChat(sample, chatml.settings, 'User', 'Character', {})
assert.ok(rChatml.text.includes('<|im_start|>system\nYou are helpful.<|im_end|>'), '包含 ChatML system 标记')
assert.ok(rChatml.text.includes('<|im_start|>user\nHello!<|im_end|>'), '包含 ChatML user 标记')

// 4. Mistral 格式精确断言
const mistral = INSTRUCT_PRESETS.find((p) => p.id === 'mistral')
assert.ok(mistral, '存在 mistral 预设')
const rMistral = formatInstructChat(sample, mistral.settings, 'User', 'Character', {})
assert.ok(rMistral.text.includes('[INST]'), '包含 [INST]')
assert.ok(rMistral.text.includes('[/INST]'), '包含 [/INST]')

console.log('Instruct 预设自检全部通过')
