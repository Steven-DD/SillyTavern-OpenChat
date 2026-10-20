#!/usr/bin/env node
import assert from 'node:assert/strict'
import {
  vectorsCollectionId,
  hashMessage,
  VECTOR_DEFAULTS,
} from '../src/services/st/vectors'

console.log('  — 向量服务结构与哈希相似度门禁自检 —')

// 1. collectionId 规范化清洗测试
const col1 = vectorsCollectionId('Alice.png', 'chat-2026-10-20')
assert.ok(col1.startsWith('stoc-alice-png-chat-2026-10-20'), `集合 ID 清洗合规: ${col1}`)
assert.ok(!col1.includes('.'), `集合 ID 不含点号: ${col1}`)

const col2 = vectorsCollectionId('Special/Avatar#1?.png', 'file:with/slashes')
assert.ok(!/[^a-z0-9-]/.test(col2), `非法字符被替换为中划线: ${col2}`)

// 2. 消息哈希稳定性与区分度
const h1 = hashMessage('Hello world')
const h2 = hashMessage('Hello world')
const h3 = hashMessage('Hello world!')
assert.equal(h1, h2, '相同内容哈希值恒等')
assert.notEqual(h1, h3, '不同内容哈希值产生区分')
assert.ok(Number.isInteger(h1) && h1 > 0, '哈希值为正整数')

// 3. 模板注入占位符验证
assert.ok(VECTOR_DEFAULTS.template.includes('{{text}}'), '默认向量注入模板必须包含 {{text}} 占位符')
assert.ok(VECTOR_DEFAULTS.top_k >= 1, '默认 top_k 有效')
assert.ok(VECTOR_DEFAULTS.depth >= 0, '默认 depth 有效')

// 4. 余弦相似度基础算法验证
function dot(a, b) {
  return a.reduce((sum, v, i) => sum + v * b[i], 0)
}
function norm(a) {
  return Math.sqrt(dot(a, a))
}
function cosineSim(a, b) {
  const na = norm(a)
  const nb = norm(b)
  return na && nb ? dot(a, b) / (na * nb) : 0
}

const vA = [1, 2, 3]
const vB = [1, 2, 3]
const vC = [-1, -2, -3]
const vD = [0, 0, 1]

assert.ok(Math.abs(cosineSim(vA, vB) - 1.0) < 1e-6, '同向向量相似度为 1')
assert.ok(Math.abs(cosineSim(vA, vC) - (-1.0)) < 1e-6, '反向向量相似度为 -1')
assert.ok(cosineSim(vA, vD) > 0 && cosineSim(vA, vD) < 1, '部分重合向量相似度在 0 到 1 之间')

console.log('✓ 向量检索核心模块自检通过')
