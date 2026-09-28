/**
 * 会话锁定（chat_metadata.persona）真机验证：写 → 读回 → 解除 → 校验还原
 */
const rawBase = process.argv[2] ?? 'http://127.0.0.1:8000'
const BASE = rawBase.includes('://') ? rawBase : `http://${rawBase}`
globalThis.localStorage = { getItem: () => BASE, setItem: () => {}, removeItem: () => {} }

const AVATAR = 'default_Seraphina.png'
const FILE = 'Seraphina - 2023-5-12 @21h 32m 29s 224ms'
let pass = 0
let fail = 0
const ok = (n, c, e = '') => {
  if (c) { pass++; console.log(`  ✅ ${n}${e ? `  — ${e}` : ''}`) }
  else { fail++; console.log(`  ❌ ${n}${e ? `  — ${e}` : ''}`) }
}

const path = await import('node:path')
const { build } = await import('esbuild')
async function load(entry) {
  const b = await build({
    entryPoints: [path.resolve(entry)],
    bundle: true, format: 'esm', write: false, logLevel: 'silent',
  })
  return import('data:text/javascript;base64,' + Buffer.from(b.outputFiles[0].text).toString('base64'))
}
const persona = await load('src/services/st/persona.ts')
const data = await load('src/services/st/data.ts')

console.log(`=== 会话锁定验证（${BASE}）===`)
const before = await data.getChat(AVATAR, FILE)
ok('读到会话', Array.isArray(before) && before.length > 0, `${before.length} 项`)
const beforeId = persona.chatPersonaId(before)
ok('初始锁定状态已读出', true, `locked=${beforeId ?? 'null'}`)

try {
  // 锁到 user-default.png（当前唯一人设）
  await persona.setChatPersona(AVATAR, FILE, 'user-default.png')
  const after = await data.getChat(AVATAR, FILE)
  ok('写入锁定后读回正确', persona.chatPersonaId(after) === 'user-default.png', `=${persona.chatPersonaId(after)}`)
  ok('会话消息条数未变', after.length === before.length, `${before.length} → ${after.length}`)

  // 解除锁定
  await persona.setChatPersona(AVATAR, FILE, null)
  const after2 = await data.getChat(AVATAR, FILE)
  ok('解除锁定后为空', persona.chatPersonaId(after2) === null, `=${persona.chatPersonaId(after2)}`)
  ok('解除后消息条数未变', after2.length === before.length, `${before.length} → ${after2.length}`)

  // 还原到初始锁定状态（若原本有）
  if (beforeId) await persona.setChatPersona(AVATAR, FILE, beforeId)
  const restore = await data.getChat(AVATAR, FILE)
  ok('已还原初始锁定状态', persona.chatPersonaId(restore) === beforeId, `=${persona.chatPersonaId(restore)}`)
} catch (e) {
  fail++
  console.log(`  ❌ 异常：${e.message}`)
}

console.log(`\n${fail === 0 ? '🎉 全部通过' : '⚠️ 有失败项'}（通过 ${pass}，失败 ${fail}）`)
