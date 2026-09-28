/**
 * verify-persona.mjs —— 人设（Persona）读写闭环真机验证
 *
 * 用真代码（esbuild 打包 persona.ts）打真实的 ST：
 *   读取 → 新建（上传头像）→ 改名 → 改描述/位置 → 设为默认 → 删除 → 校验已还原
 * 与 verify-m1 同策略：真写，最后逐字节校验还原。
 */
import { build } from 'esbuild'
import path from 'node:path'
import fs from 'node:fs'

const APP = path.resolve(import.meta.dirname, '..')
const rawBase = process.argv[2] ?? process.env.ST_BASE ?? 'http://127.0.0.1:8000'
const BASE = rawBase.includes('://') ? rawBase : `http://${rawBase}`

let pass = 0
let fail = 0
function ok(name, cond, extra = '') {
  if (cond) {
    pass++
    console.log(`  ✅ ${name}${extra ? `  — ${extra}` : ''}`)
  } else {
    fail++
    console.log(`  ❌ ${name}${extra ? `  — ${extra}` : ''}`)
  }
}

/* ---- 打真代码 ---- */
const built = await build({
  entryPoints: [path.join(APP, 'src/services/st/persona.ts')],
  bundle: true,
  format: 'esm',
  write: false,
  logLevel: 'silent',
  external: [],
})
const mod = await import(
  'data:text/javascript;base64,' + Buffer.from(built.outputFiles[0].text).toString('base64')
)

// persona.ts 的 stBase() 读 localStorage → node 里没有，直接指定 relay 基地址
globalThis.localStorage = {
  getItem: () => BASE,
  setItem: () => {},
  removeItem: () => {},
}

console.log(`=== 真机验证（${BASE}）===`)
const before = await mod.listPersonas()
ok('读取人设列表', Array.isArray(before.personas), `${before.personas.length} 个`)
ok('读到用户名', typeof before.userName === 'string', `username=${before.userName}`)
const snapshot = JSON.stringify(before.personas)

/* ---- 新建：上传一张 1x1 png ---- */
const png = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8AAAwAB/wD/2lAAAAAASUVORK5CYII=',
  'base64',
)
const tmpPng = path.join(APP, 'scripts', '.verify-persona.png')
fs.writeFileSync(tmpPng, png)

let newId = ''
try {
  const file = new File([png], 'verify-persona.png', { type: 'image/png' })
  newId = await mod.uploadAvatar(file)
  ok('头像上传成功', !!newId, `file=${newId}`)

  const url = `${BASE}/thumbnail?type=persona&file=${encodeURIComponent(newId)}`
  const r = await fetch(url)
  ok('头像可访问', r.ok, `HTTP ${r.status} type=${r.headers.get('content-type')}`)

  const list = [...before.personas, { id: newId, name: '验证人设', description: '', position: 0 }]
  await mod.savePersonas({ personas: list, userName: before.userName })
  const after = await mod.listPersonas()
  const found = after.personas.find((p) => p.id === newId)
  ok('人设已写入映射', found?.name === '验证人设', `name=${found?.name}`)

  // 改名 + 描述 + 位置
  await mod.savePersonas({
    personas: after.personas.map((p) =>
      p.id === newId
        ? { ...p, name: '验证人设2', description: '这是一个验证用的人设描述', position: 4 }
        : p,
    ),
  })
  const after2 = await mod.listPersonas()
  const f2 = after2.personas.find((p) => p.id === newId)
  ok('改名/描述/位置 已生效', f2?.name === '验证人设2' && f2?.position === 4, `name=${f2?.name} pos=${f2?.position}`)

  // 设为默认 + 改用户名
  await mod.savePersonas({ defaultId: newId, userName: 'VerifyUser' })
  const after3 = await mod.listPersonas()
  ok('默认人设已设置', after3.defaultId === newId, `default=${after3.defaultId}`)
  ok('用户名已改写', after3.userName === 'VerifyUser', `username=${after3.userName}`)

  // 删除头像
  await mod.deleteAvatar(newId)
  const after4 = await mod.listPersonas()
  ok('头像已删除（映射仍在，需同步删映射）', !!after4.personas.find((p) => p.id === newId))
} catch (e) {
  fail++
  console.log(`  ❌ 异常：${e.message}`)
}

/* ---- 还原 ---- */
try {
  await mod.savePersonas({ personas: JSON.parse(snapshot), defaultId: before.defaultId, userName: before.userName })
  try {
    await mod.deleteAvatar(newId)
  } catch {
    /* 可能已删 */
  }
  const restored = await mod.listPersonas()
  ok(
    '已还原原始人设数据',
    JSON.stringify(restored.personas) === snapshot &&
      restored.defaultId === before.defaultId &&
      restored.userName === before.userName,
    `default=${restored.defaultId} username=${restored.userName}`,
  )
} catch (e) {
  fail++
  console.log(`  ❌ 还原失败：${e.message}`)
} finally {
  fs.rmSync(tmpPng, { force: true })
}

console.log(`\n${fail === 0 ? '🎉 全部通过' : '⚠️ 有失败项'}（通过 ${pass}，失败 ${fail}）`)
process.exit(fail === 0 ? 0 : 1)
