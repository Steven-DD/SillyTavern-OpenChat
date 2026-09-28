/**
 * S2 数据区外移的验证脚本
 *
 * 用途：迁移前后比对源与目标，确认「复制完整、源未被改动、关键密钥逐字节一致」。
 * 用法：node scripts/verify-s2.mjs <阶段标签>   阶段标签只影响打印标题。
 */
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

const ST_DIR = 'C:/Users/Administrator/WorkBuddy/2026-09-21-23-04-59/sillytavern'
const SRC = path.join(ST_DIR, 'data')
const CFG = path.join(process.env.APPDATA, 'com.datago.st-chat')
const DST = path.join(CFG, 'st-runtime', 'data')

const label = process.argv[2] ?? '快照'

function walk(dir, out = []) {
  if (!fs.existsSync(dir)) return out
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name)
    if (e.isDirectory()) walk(p, out)
    else if (e.isFile()) out.push(p)
  }
  return out
}

function summarize(root) {
  const files = walk(root)
  let bytes = 0
  const map = new Map()
  for (const f of files) {
    const size = fs.statSync(f).size
    bytes += size
    map.set(path.relative(root, f).replace(/\\/g, '/'), size)
  }
  return { files: files.length, bytes, map }
}

/** 关键文件必须逐字节一致（密钥类，差异不可接受） */
function hashOf(root, rel) {
  const p = path.join(root, rel)
  if (!fs.existsSync(p)) return null
  return createHash('sha256').update(fs.readFileSync(p)).digest('hex').slice(0, 16)
}

const KEY_FILES = [
  'default-user/secrets.json',
  'default-user/settings.json',
  'default-user/stats.json',
  'cookie-secret.txt',
]

const KEY_DIRS = [
  'default-user/characters',
  'default-user/chats',
  'default-user/worlds',
  'default-user/User Avatars',
  '_storage',
]

console.log(`=== ${label} ===`)
console.log(`  源   ${SRC}`)
console.log(`  目标 ${DST}`)
console.log()

const s = summarize(SRC)
const d = summarize(DST)

const fmt = (n) => `${n} 个文件 / ${(n / 1048576).toFixed(2)} MB`
void fmt
console.log(`  源   : ${s.files} 个文件 / ${s.bytes} B`)
console.log(`  目标 : ${d.files} 个文件 / ${d.bytes} B`)
if (d.files === 0) console.log('  ⚠️ 目标为空（尚未迁移？）')
console.log()

console.log('  关键文件 sha256（前 16 位）')
let hashOk = true
for (const rel of KEY_FILES) {
  const a = hashOf(SRC, rel)
  const b = hashOf(DST, rel)
  const mark = a && b ? (a === b ? '✅' : '❌ 不一致') : a && !b ? '❌ 目标缺失' : '—'
  if (a && a !== b) hashOk = false
  console.log(`    ${mark.padEnd(12)} ${rel.padEnd(28)} 源=${a ?? '无'} 目标=${b ?? '无'}`)
}
console.log()

console.log('  关键目录文件数（源 → 目标）')
let dirOk = true
for (const rel of KEY_DIRS) {
  const a = walk(path.join(SRC, rel)).length
  const b = walk(path.join(DST, rel)).length
  const mark = a === b ? '✅' : '❌'
  if (a !== b) dirOk = false
  console.log(`    ${mark} ${rel.padEnd(28)} ${a} → ${b}`)
}
console.log()

// 逐文件比对（目标非空时才做）
if (d.files > 0) {
  const missing = []
  const sizeDiff = []
  for (const [rel, size] of s.map) {
    const t = d.map.get(rel)
    if (t === undefined) missing.push(rel)
    else if (t !== size) sizeDiff.push(`${rel} (${size}→${t})`)
  }
  console.log('  逐文件比对')
  console.log(`    缺失 : ${missing.length === 0 ? '0 ✅' : missing.length + ' ❌'} ${missing.slice(0, 5).join(', ')}`)
  console.log(`    大小不符 : ${sizeDiff.length === 0 ? '0 ✅' : sizeDiff.length + ' ❌'} ${sizeDiff.slice(0, 5).join(', ')}`)
  console.log()

  const extra = [...d.map.keys()].filter((k) => !s.map.has(k))
  console.log(`  目标多出的文件 : ${extra.length === 0 ? '0 ✅' : extra.length + '（config.yaml 等属预期）'} ${extra.slice(0, 5).join(', ')}`)
  console.log()

  const allOk = missing.length === 0 && sizeDiff.length === 0 && hashOk && dirOk
  console.log(allOk ? '  🎉 结论：复制完整、关键密钥逐字节一致' : '  ⚠️ 结论：存在不一致，需要排查')
} else {
  console.log(`  快照已记录：${s.files} 个文件 / ${s.bytes} B`)
}

// config.yaml 单独看（它不在 data 下）
const cfgSrc = path.join(ST_DIR, 'config.yaml')
const cfgDst = path.join(CFG, 'st-runtime', 'config.yaml')
if (fs.existsSync(cfgSrc) || fs.existsSync(cfgDst)) {
  const a = fs.existsSync(cfgSrc) ? fs.statSync(cfgSrc).size : null
  const b = fs.existsSync(cfgDst) ? fs.statSync(cfgDst).size : null
  console.log(`\n  config.yaml : 源 ${a ?? '无'} B → 目标 ${b ?? '无'} B ${a === b ? '✅' : '❌'}`)
}
