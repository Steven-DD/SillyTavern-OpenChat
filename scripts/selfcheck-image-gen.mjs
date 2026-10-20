#!/usr/bin/env node
/**
 * selfcheck-image-gen.mjs —— 自动化自检：文生图参数装配与 {draw::...} 宏解析
 */
import assert from 'node:assert/strict'
import { extractDrawPrompts, DEFAULT_IMAGE_GEN_CONFIG } from '../src/services/st/imageGen'

console.log('=== selfcheck-image-gen: 开始文生图自检 ===')

// 1. 默认配置完整性断言
assert.equal(typeof DEFAULT_IMAGE_GEN_CONFIG.provider, 'string', 'provider 需为字符串')
assert.equal(typeof DEFAULT_IMAGE_GEN_CONFIG.width, 'number', 'width 需为数值')
assert.equal(typeof DEFAULT_IMAGE_GEN_CONFIG.height, 'number', 'height 需为数值')
assert.ok(DEFAULT_IMAGE_GEN_CONFIG.width > 0 && DEFAULT_IMAGE_GEN_CONFIG.height > 0, '尺寸需大于 0')
console.log('✓ 1. 默认配置完整性校验通过')

// 2. {draw::...} 宏提取校验
const sampleText1 = '你好！这是为你准备的画作：{draw::1girl, white hair, sunset, smiling} 希望你喜欢。'
const res1 = extractDrawPrompts(sampleText1)
assert.equal(res1.prompts.length, 1, '应提取到 1 个生图宏')
assert.equal(res1.prompts[0], '1girl, white hair, sunset, smiling')
assert.ok(!res1.cleanText.includes('{draw::'), 'cleanText 应剔除宏标签')
assert.ok(res1.cleanText.includes('你好！这是为你准备的画作：'), 'cleanText 应保留普通文本')
console.log('✓ 2. 单个 {draw::...} 宏提取与清理断言通过')

// 3. 多宏与兼容格式 {image:...} 提取校验
const sampleText2 = '前置 {image:castle on a cliff} 中间文字 {draw::forest path at dawn} 尾声'
const res2 = extractDrawPrompts(sampleText2)
assert.equal(res2.prompts.length, 2, '应提取到 2 个生图宏')
assert.equal(res2.prompts[0], 'castle on a cliff')
assert.equal(res2.prompts[1], 'forest path at dawn')
console.log('✓ 3. 多宏与 {image:...} 别名断言通过')

// 4. 空文本安全边界
const resEmpty = extractDrawPrompts('')
assert.equal(resEmpty.prompts.length, 0)
assert.equal(resEmpty.cleanText, '')
console.log('✓ 4. 空文本保护断言通过')

console.log('=== selfcheck-image-gen: 全部通过 ===')
