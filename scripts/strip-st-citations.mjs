#!/usr/bin/env node
/**
 * 一次性内容审计 codemod：清除 src/ 注释里的 ST 引用性废话。
 * 只处理注释行（//、/*、*、<!-- 与块注释内部），不碰任何字符串字面量/代码。
 * 用户可见文案（模板文本、字符串）由人工单独处理。
 */
import { readFileSync, writeFileSync, readdirSync, statSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..', 'src')

// 噪音子句：含这些特征的子句整段删除
const NOISE = /对齐|同\s?ST|与\s?ST|ST\s?同|同源|同字段|同款|复刻|互通|跨端|js:\d|\bM-\d|\bP\d-\d|\bB\d(?![a-zA-Z0-9-])|1\.19|dummyId|10{4,}/
// 子句同时含「一致/同」与 ST/网页端 才算噪音（避免误伤「与上次一致」）
const CONSISTENT = /一致|相同|同/
const STCTX = /ST|网页端/

function isNoiseClause(c) {
  if (NOISE.test(c)) return true
  if (CONSISTENT.test(c) && STCTX.test(c)) return true
  if (/^\s*(P\d-\d|M-\d+|B\d)\s*$/.test(c)) return true
  return false
}

function cleanLine(line) {
  const before = line
  let out = line
  // 1) 最内层纯引用括注（含 .js:NNN）整对删除，可重复嵌套
  for (let i = 0; i < 4; i++) {
    out = out.replace(/（[^（）]*\.js:\d+[^（）]*）/g, '')
  }
  // 2) 子句级清理：把括注内容按 ，、； 拆开，丢掉噪音子句
  out = out.replace(/（([^（）]*)）/g, (m, inner) => {
    const clauses = inner.split(/[，、；]/)
    const kept = clauses.filter((c) => !isNoiseClause(c))
    const rest = kept.join('，').replace(/^[，、；]+|[，、；]+$/g, '')
    return rest.trim() ? `（${rest}）` : ''
  })
  // 3) 行尾/行中残留的引用碎片（字符类排除 * / —— 绝不能吞掉 */ 或 --> 闭合符）
  out = out.replace(/[，,；;]?\s*(?:——\s*)?对齐[^，。；)）*/]*$/g, '')
  out = out.replace(/[，,；;]?\s*——\s*与\s?ST[^，。；)）*/]*/g, '')
  out = out.replace(/[，,；;]\s*——\s*?同\s?ST[^，。；)）*/]*/g, '')
  out = out.replace(/[，,；;]?\s*与\s?ST[^，。；)）*/]*$/g, '')
  out = out.replace(/[，,；;]?\s*同\s?ST[^，。；)）*/]*$/g, '')
  // 4) 残留行号引用与坏括号
  out = out.replace(/[（(][^（）()]*\.js:\d+[^（）()]*[)）]/g, '')
  out = out.replace(/（（/g, '（').replace(/））/g, '）')
  out = out.replace(/（\s*）/g, '').replace(/\s+（$/g, '')
  out = out.replace(/[，,、]\s*（/g, '（').replace(/）\s*[，,、]$/g, '）')
  out = out.replace(/([，、；])(?=[，、；])/g, '')
  // 防护：绝不允许吃掉注释闭合符或引入空 JSDoc
  if (/\*\*/.test(before) && !/\*[^*]/.test(out.replace(/^[\s*/]+/, ''))) return line // JSDoc 内容被清空
  if ((before.includes('*/') && !out.includes('*/')) || (before.includes('-->') && !out.includes('-->'))) return line
  if ((before.includes('<!--') && !out.includes('<!--'))) return line
  return out
}

function processFile(path) {
  const src = readFileSync(path, 'utf8')
  const lines = src.split('\n')
  let inBlock = false // <!-- --> 多行模板注释
  let changed = false
  const outLines = lines.map((line) => {
    const t = line.trim()
    const isComment =
      t.startsWith('//') || t.startsWith('*') || t.startsWith('/*') || t.startsWith('<!--')
    const wasInBlock = inBlock
    if (inBlock) {
      inBlock = !t.includes('-->')
    } else if (t.startsWith('<!--') && !t.includes('-->')) {
      inBlock = true
    }
    if (!isComment && !wasInBlock) return line
    const cleaned = cleanLine(line)
    if (cleaned !== line) {
      changed = true
      return cleaned
    }
    return line
  })
  if (changed) writeFileSync(path, outLines.join('\n'))
  return changed
}

function walk(dir, acc = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    const st = statSync(p)
    if (st.isDirectory()) walk(p, acc)
    else if (/\.(ts|vue)$/.test(name)) acc.push(p)
  }
  return acc
}

let n = 0
for (const f of walk(root)) if (processFile(f)) n++
console.log(`已清理 ${n} 个文件`)
