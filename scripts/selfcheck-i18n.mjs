#!/usr/bin/env node
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

console.log('  — i18n 键集对称性与变量插值校验 —')

const root = process.cwd()
const zhPath = join(root, 'src', 'locales', 'zh-CN.json')
const enPath = join(root, 'src', 'locales', 'en-US.json')

const zh = JSON.parse(readFileSync(zhPath, 'utf-8'))
const en = JSON.parse(readFileSync(enPath, 'utf-8'))

function flattenKeys(obj, prefix = '') {
  const result = {}
  for (const [k, v] of Object.entries(obj)) {
    const full = prefix ? `${prefix}.${k}` : k
    if (v && typeof v === 'object' && !Array.isArray(v)) {
      Object.assign(result, flattenKeys(v, full))
    } else {
      result[full] = v
    }
  }
  return result
}

const flatZh = flattenKeys(zh)
const flatEn = flattenKeys(en)

const zhKeys = Object.keys(flatZh).sort()
const enKeys = Object.keys(flatEn).sort()

// 1. 键名完全对称断言
const missingInEn = zhKeys.filter((k) => !(k in flatEn))
const missingInZh = enKeys.filter((k) => !(k in flatZh))

assert.equal(missingInEn.length, 0, `en-US 缺少键: ${missingInEn.join(', ')}`)
assert.equal(missingInZh.length, 0, `zh-CN 缺少键: ${missingInZh.join(', ')}`)

// 2. 所有翻译叶子节点非空
for (const [k, v] of Object.entries(flatZh)) {
  assert.ok(typeof v === 'string' && v.trim().length > 0, `zh-CN[${k}] 翻译不应为空`)
}
for (const [k, v] of Object.entries(flatEn)) {
  assert.ok(typeof v === 'string' && v.trim().length > 0, `en-US[${k}] 翻译不应为空`)
}

// 3. 校验插值变量一致性（如 {name}）
const VAR_REGEX = /\{([a-zA-Z0-9_]+)\}/g
for (const k of zhKeys) {
  const zhVars = Array.from(flatZh[k].matchAll(VAR_REGEX), (m) => m[1]).sort()
  const enVars = Array.from(flatEn[k].matchAll(VAR_REGEX), (m) => m[1]).sort()
  assert.deepEqual(zhVars, enVars, `键 [${k}] 占位变量不一致: zh=${zhVars.join(',')}, en=${enVars.join(',')}`)
}

console.log(`✓ i18n 校验通过：共 ${zhKeys.length} 个多语言键，zh-CN 与 en-US 100% 对称`)
