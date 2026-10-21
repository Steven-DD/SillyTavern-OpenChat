#!/usr/bin/env node
/**
 * 预设兼容性自检（Presets & Compatibility Gate）
 *
 * 验证：
 * 1. TextGen / OpenAI 采样预设双向字段转换保真度
 * 2. Instruct 格式预设序列转换与字段对齐
 * 3. Context Template Story String 的 Handlebars 编译与变量插值
 * 4. Prompt Manager 预设结构导入导出保真度
 */
import assert from 'node:assert/strict'
import Handlebars from 'handlebars'
import {
  presetToGenSettings,
  genSettingsToPreset,
  BUILTIN_GEN_PRESETS,
} from '../src/services/st/presets'
import { rawToInstructSettings } from '../src/services/st/instructPresets'
import { BUILTIN_CONTEXT_TEMPLATES } from '../src/services/st/contextTemplates'
import { DEFAULT_ORDER } from '../src/services/st/promptmanager'

console.log('=== selfcheck-presets: 开始预设系统兼容性自检 ===')

// 1. 采样预设转换断言
console.log('  — 1. 采样预设字段双向转换断言 —')
{
  const stTextgenPreset = {
    temp: 0.85,
    top_p: 0.92,
    top_k: 45,
    min_p: 0.08,
    top_a: 0.12,
    rep_pen: 1.18,
    frequency_penalty: 0.25,
    presence_penalty: 0.15,
    max_tokens: 512,
    model: 'gpt-4o-mini',
  }

  const genSettings = presetToGenSettings(stTextgenPreset)
  assert.equal(genSettings.temperature, 0.85, 'temperature 提取失败')
  assert.equal(genSettings.topP, 0.92, 'topP 提取失败')
  assert.equal(genSettings.topK, 45, 'topK 提取失败')
  assert.equal(genSettings.minP, 0.08, 'minP 提取失败')
  assert.equal(genSettings.topA, 0.12, 'topA 提取失败')
  assert.equal(genSettings.repetitionPenalty, 1.18, 'repetitionPenalty 提取失败')
  assert.equal(genSettings.frequencyPenalty, 0.25, 'frequencyPenalty 提取失败')
  assert.equal(genSettings.presencePenalty, 0.15, 'presencePenalty 提取失败')
  assert.equal(genSettings.maxTokens, 512, 'maxTokens 提取失败')
  assert.equal(genSettings.model, 'gpt-4o-mini', 'model 提取失败')

  // 反向转换
  const backToText = genSettingsToPreset(
    {
      genType: 'text',
      source: 'koboldcpp',
      textBackend: 'koboldcpp',
      textServer: '',
      model: 'my-model',
      fallbackModel: '',
      temperature: 0.75,
      maxTokens: 600,
      maxContext: 4096,
      includeExamples: true,
      topP: 0.9,
      topK: 50,
      topA: 0.05,
      minP: 0.03,
      frequencyPenalty: 0.1,
      presencePenalty: 0.1,
      repetitionPenalty: 1.1,
      reasoningEffort: 'auto',
      extraBodyYaml: '',
      thinkingPrefix: '',
    },
    'text',
  )
  assert.equal(backToText.temp, 0.75)
  assert.equal(backToText.top_p, 0.9)
  assert.equal(backToText.rep_pen, 1.1)
  assert.equal(backToText.n_predict, 600)
  console.log('✓ 采样预设字段转换全部通过')
}

// 2. Instruct 预设字段转换断言
console.log('  — 2. Instruct 预设字段解析断言 —')
{
  const stInstructRaw = {
    name: 'Custom-Llama',
    system_sequence: '<|sys_start|>',
    system_suffix: '<|sys_end|>',
    input_sequence: '<|user_start|>',
    input_suffix: '<|user_end|>',
    output_sequence: '<|bot_start|>',
    output_suffix: '<|bot_end|>',
    stop_sequence: '<|bot_end|>\n<|sys_end|>',
    wrap: false,
    macro: true,
    names_behavior: 'always',
    system_same_as_user: false,
  }

  const parsed = rawToInstructSettings(stInstructRaw, 'Custom-Llama')
  assert.equal(parsed.preset, 'Custom-Llama')
  assert.equal(parsed.system_sequence, '<|sys_start|>')
  assert.equal(parsed.output_sequence, '<|bot_start|>')
  assert.equal(parsed.names_behavior, 'always')
  assert.equal(parsed.wrap, false)
  console.log('✓ Instruct 预设解析断言通过')
}

// 3. Context Template Story String 模板编译
console.log('  — 3. Context Template 模板编译断言 —')
{
  const defaultTpl = BUILTIN_CONTEXT_TEMPLATES[0]
  if (!defaultTpl) {
    throw new Error('BUILTIN_CONTEXT_TEMPLATES is empty')
  }
  const compiled = Handlebars.compile(defaultTpl.story_string, { noEscape: true })
  const rendered = compiled({
    system: 'SYSTEM_PROMPT',
    description: 'CHAR_DESC',
    personality: 'CHAR_PERS',
    scenario: 'SCENARIO',
    persona: 'USER_PERSONA',
    wiBefore: 'WI_BEFORE',
    examples: 'EXAMPLES',
    chat: 'CHAT_HISTORY',
    wiAfter: 'WI_AFTER',
  })

  assert.ok(rendered.includes('SYSTEM_PROMPT'), '渲染结果必须包含系统提示')
  assert.ok(rendered.includes('CHAR_DESC'), '渲染结果必须包含角色描述')
  assert.ok(rendered.includes('CHAT_HISTORY'), '渲染结果必须包含会话历史')
  assert.ok(
    rendered.indexOf('SYSTEM_PROMPT') < rendered.indexOf('CHAT_HISTORY'),
    '系统提示次序必须位于会话历史之前',
  )
  console.log('✓ Context Template 模板编译断言通过')
}

// 4. Prompt Manager 预设结构断言
console.log('  — 4. Prompt Manager 预设结构对齐断言 —')
{
  assert.ok(Array.isArray(DEFAULT_ORDER), 'DEFAULT_ORDER 必须为数组')
  assert.ok(DEFAULT_ORDER.some((x) => x.identifier === 'main'), '必须包含 main 标识')
  assert.ok(DEFAULT_ORDER.some((x) => x.identifier === 'chatHistory'), '必须包含 chatHistory 标识')
  console.log('✓ Prompt Manager 预设结构对齐断言通过')
}

console.log('=== selfcheck-presets: 全部通过 ===')
