/**
 * settings 写队列自检（评审整改 T5 验收：并发多模块读-改-写不丢键）
 *
 * 用内存双写（内存后端代替真实 ST）验证：
 * 1. 并发触发 QR + 正则 + 翻译 + 向量 4 个 mutateSettings → 最终文件含全部 4 项修改
 * 2. 未知键零丢失（200+ 未知键的 settings 读-改-写一轮逐键 diff）
 * 3. 队列串行性：读-改-写事务不重叠
 * 4. skip 语义：无变化不写
 * 5. 先到先写：后入队者基于前者刚写的内容合并（锁内新鲜读，防 5s TTL 旧快照）
 *
 * 说明：与 src/services/st/settings-writequeue.ts 的调度骨架同构（enqueue chain +
 * 锁内新鲜读 + 单事务合并写）。真实 data.ts 的 IO 打桩在 esbuild bundle 下不可行
 * （编译期绑定），故此处验证的是队列语义不变量；真实链路集成由 e2e 覆盖。
 * 用法：npm run selfcheck（run-selfchecks.mjs 统一打包执行）或 node 直跑。
 */

let disk = {}

const reads = []
let inflight = 0
let maxInflight = 0

let chain = Promise.resolve()
function enqueue(task) {
  const run = chain.then(task, task)
  chain = run.then(
    () => undefined,
    () => undefined,
  )
  return run
}

async function readLatest() {
  reads.push(Date.now())
  inflight++
  maxInflight = Math.max(maxInflight, inflight)
  await new Promise((r) => setTimeout(r, 5))
  inflight--
  return structuredClone(disk)
}

async function write(next) {
  disk = structuredClone(next)
}

/** 与 settings-writequeue.ts 同款事务骨架 */
function mutate(fn) {
  return enqueue(async () => {
    const current = await readLatest()
    const { next, result, skip } = fn(current)
    if (!skip) await write(next)
    return result
  })
}

let fail = 0
const ok = (cond, name) => {
  if (!cond) {
    fail++
    console.error(`✗ ${name}`)
  }
}

/* ── 场景 1：4 个模块并发读-改-写，全键保留 ── */
disk = { version: 1, power_user: { keep_me: true }, extension_settings: {} }

const writeExt = (key, value) =>
  mutate((cur) => {
    const ext = { ...(cur.extension_settings ?? {}) }
    ext[key] = value
    return { next: { ...cur, extension_settings: ext }, result: ext }
  })

const results = await Promise.all([
  writeExt('stchat_quick_replies', [{ label: 'qr1' }]),
  writeExt('regex', [{ script: 're1' }]),
  writeExt('stchat_translate', { lang: 'zh' }),
  writeExt('stchat_vectors', { enabled: true }),
])

const ext1 = disk.extension_settings ?? {}
ok(
  ['stchat_quick_replies', 'regex', 'stchat_translate', 'stchat_vectors'].every((k) => k in ext1),
  `场景1：4 个并发模块的键全部落盘（实际仅 ${Object.keys(ext1).join(',')}）`,
)
ok(disk.power_user?.keep_me === true, '场景1：非 extension_settings 键不被抹')
ok(results.length === 4, '场景1：全部任务的返回值透传')

/* ── 场景2：200+ 未知键零丢失 ── */
const unknown = {}
for (let i = 0; i < 200; i++) unknown[`unknown_key_${i}`] = { i, pad: 'x'.repeat(10) }
disk = { ...unknown, extension_settings: {} }

await mutate((cur) => {
  const ext = { ...(cur.extension_settings ?? {}) }
  ext.stchat_speech = { provider: 'system' }
  return { next: { ...cur, extension_settings: ext } }
})

const lost = Object.keys(unknown).filter((k) => !(k in disk))
ok(lost.length === 0, `场景2：200 未知键零丢失（丢 ${lost.length} 个）`)
ok(disk.extension_settings?.stchat_speech !== undefined, '场景2：新键写入')

/* ── 场景3：串行性 —— 队列事务不重叠 ── */
ok(maxInflight <= 1, `场景3：读-改-写事务不重叠（maxInflight=${maxInflight}）`)

/* ── 场景4：skip 语义不写盘 ── */
const before = JSON.stringify(disk)
await mutate((cur) => ({ next: cur, skip: true }))
ok(JSON.stringify(disk) === before, '场景4：skip=true 不产生写盘')

/* ── 场景5：先到先写 —— 后入队者基于前者的结果合并 ── */
disk = { extension_settings: {}, counter: 0 }
await Promise.all([
  mutate((cur) => {
    const ext = { ...(cur.extension_settings ?? {}) }
    ext.a = 'from-a'
    return { next: { ...cur, extension_settings: ext } }
  }),
  mutate((cur) => {
    // b 的合并输入必须已含 a 的键（若读的是旧快照就会丢）
    const ext = { ...(cur.extension_settings ?? {}) }
    ok(ext.a === 'from-a', '场景5：后入队任务读到前者刚写的内容（锁内新鲜读）')
    ext.b = 'from-b'
    return { next: { ...cur, extension_settings: ext } }
  }),
])
const ext5 = disk.extension_settings ?? {}
ok(ext5.a === 'from-a' && ext5.b === 'from-b', '场景5：a/b 两键共存')

console.log(fail ? `${fail} 失败` : 'settings 写队列语义全部通过')
process.exit(fail ? 1 : 0)
