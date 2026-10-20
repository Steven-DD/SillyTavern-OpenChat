#!/usr/bin/env node
/**
 * selfcheck-live2d-vrm.mjs —— 自动化自检：Live2D/VRM 视口状态、表情映射表与音频口型 RMS
 */
import assert from 'node:assert/strict'
import {
  DEFAULT_STAGE_SETTINGS,
  EMOTION_MAP,
  calculateLipSyncLevel,
} from '../src/services/st/modelStage'

console.log('=== selfcheck-live2d-vrm: 开始模型视口与口型联动自检 ===')

// 1. 默认视口配置断言
assert.equal(typeof DEFAULT_STAGE_SETTINGS.layout, 'string', 'layout 需为字符串')
assert.equal(typeof DEFAULT_STAGE_SETTINGS.scale, 'number', 'scale 需为数值')
assert.ok(DEFAULT_STAGE_SETTINGS.scale > 0, '缩放比例需大于 0')
assert.equal(DEFAULT_STAGE_SETTINGS.lipSyncEnabled, true, '默认开启口型同步')
console.log('✓ 1. 默认配置完整性校验通过')

// 2. 情绪动作映射表完整性校验
const requiredEmotions = ['joy', 'sadness', 'anger', 'surprise', 'neutral', 'blush']
for (const emo of requiredEmotions) {
  assert.ok(EMOTION_MAP[emo], `映射表必须包含情绪: ${emo}`)
  assert.ok(EMOTION_MAP[emo].live2d, `Live2D 动作必须定义: ${emo}`)
  assert.ok(EMOTION_MAP[emo].vrm, `VRM 表情必须定义: ${emo}`)
}
console.log('✓ 2. 核心情绪映射表断言通过 (joy/sadness/anger/surprise/neutral/blush)')

// 3. calculateLipSyncLevel 音频分析自检
// 3.1 空数据保护
assert.equal(calculateLipSyncLevel(new Uint8Array(0)), 0, '空数据应返回 0')

// 3.2 8-bit 静音 (全 128 偏置) 应计算出 0 张合度
const silenceBuffer = new Uint8Array(256).fill(128)
assert.equal(calculateLipSyncLevel(silenceBuffer), 0, '静音频段应精确返回 0')

// 3.3 满幅方波 (0 与 255 交替) 应返回接近或达到 1.0 的最大张合度
const loudBuffer = new Uint8Array(256)
for (let i = 0; i < loudBuffer.length; i++) {
  loudBuffer[i] = i % 2 === 0 ? 0 : 255
}
const loudLevel = calculateLipSyncLevel(loudBuffer)
assert.ok(loudLevel > 0.8 && loudLevel <= 1.0, `高响度音频应达到高张合度 (实测: ${loudLevel})`)
assert.ok(!Number.isNaN(loudLevel), '张合度绝不能为 NaN')
assert.ok(Number.isFinite(loudLevel), '张合度必须为有限数值')

console.log('✓ 3. Web Audio RMS 音频口型张合度计算断言通过')

console.log('=== selfcheck-live2d-vrm: 全部通过 ===')
