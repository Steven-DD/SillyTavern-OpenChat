/**
 * 世界书扫描注入测试（node 端，esbuild 打包后运行）
 *
 * 用法：
 *   npx esbuild scripts/test-worldinfo.ts --bundle --format=esm --platform=node \
 *     --outfile=node_modules/.tmp/test-wi.mjs && node node_modules/.tmp/test-wi.mjs
 *
 * 两部分：
 *   A. 单元测试——合成条目验证匹配/逻辑/预算/位置
 *   B. 集成测试——从运行中的 ST 拉真实世界书（青江大学校园世界书）扫描验证（需 8000 端口，可选）
 */
// node 无 localStorage（client.ts 请求时才会用到），先补桩
;(globalThis as Record<string, unknown>).localStorage = {
  getItem: () => null,
  setItem: () => undefined,
  removeItem: () => undefined,
}

import { checkWorldInfo, type WiEntry } from '../src/services/st/worldinfo'
import { buildPrompt } from '../src/services/st/prompt'
import type { StCharacter } from '../src/services/st/types'

let pass = 0
let fail = 0
function ok(cond: boolean, name: string): void {
  if (cond) {
    pass++
    console.log(`  ✓ ${name}`)
  } else {
    fail++
    console.error(`  ✗ ${name}`)
  }
}

function mk(partial: Partial<WiEntry>): WiEntry {
  return {
    uid: 1,
    key: [],
    keysecondary: [],
    comment: '',
    content: '',
    constant: false,
    selective: false,
    order: 100,
    position: 0,
    world: 'test',
    ...partial,
  } as WiEntry
}

/* ================= A. 单元测试 ================= */
console.log('\n[A] 匹配与激活')
{
  // 常驻条目：无历史也激活
  const r = checkWorldInfo([], [mk({ uid: 1, constant: true, content: '常驻设定' })], 4096)
  ok(r.before.includes('常驻设定'), '常驻条目无历史也激活（↑Char）')

  // 中文关键词命中（默认整词关闭 → 子串包含）
  const r2 = checkWorldInfo(
    ['你好', '今天去图书馆看书'],
    [mk({ uid: 2, key: ['图书馆'], content: '图书馆是校园核心场景' })],
    4096,
  )
  ok(r2.before.includes('图书馆是校园核心场景'), '中文关键词命中')

  // 未命中
  const r3 = checkWorldInfo(
    ['今天天气不错'],
    [mk({ uid: 3, key: ['图书馆'], content: 'X' })],
    4096,
  )
  ok(!r3.before && !r3.after && !r3.depthEntries.length, '未命中不激活')

  // 停用条目跳过
  const r4 = checkWorldInfo(
    ['提到图书馆'],
    [mk({ uid: 4, key: ['图书馆'], content: 'X', disable: true })],
    4096,
  )
  ok(!r4.before, 'disable 条目跳过')

  // 正则关键词（/pattern/flags）
  const r5 = checkWorldInfo(
    ['考了 98 分'],
    [mk({ uid: 5, key: ['/\\d{2}\\s*分/'], content: '成绩相关' })],
    4096,
  )
  ok(r5.before.includes('成绩相关'), '正则关键词命中')

  // 英文大小写不敏感（默认）
  const r6 = checkWorldInfo(
    ['Visit the LIBRARY now'],
    [mk({ uid: 6, key: ['library'], content: 'Y' })],
    4096,
  )
  ok(r6.before.includes('Y'), '大小写不敏感默认')

  // 整词模式（条目级覆盖）：单次命中
  const r7 = checkWorldInfo(
    ['open the door to the library, please'],
    [mk({ uid: 7, key: ['library'], content: 'Z', matchWholeWords: true })],
    4096,
  )
  ok(r7.before.includes('Z'), '整词模式命中英文单词')

  // 整词模式：子串不算
  const r8 = checkWorldInfo(
    ['libraries are big'],
    [mk({ uid: 8, key: ['library'], content: 'W', matchWholeWords: true })],
    4096,
  )
  ok(!r8.before, '整词模式子串不命中')
}

console.log('\n[B] selective 二级关键词')
{
  const base = { key: ['图书馆'], keysecondary: ['雨'], selective: true, content: 'S' }
  // AND_ANY(0)：主命中 + 任一次级命中
  ok(
    checkWorldInfo(['图书馆，外面下雨'], [mk({ uid: 10, ...base, selectiveLogic: 0 })], 4096).before.includes('S'),
    'AND_ANY 主+任一次级 → 激活',
  )
  ok(
    !checkWorldInfo(['图书馆，晴天'], [mk({ uid: 11, ...base, selectiveLogic: 0 })], 4096).before.includes('S'),
    'AND_ANY 主命中无次级 → 不激活',
  )
  // NOT_ANY(2)：主命中 + 次级全不中
  ok(
    checkWorldInfo(['图书馆，晴天'], [mk({ uid: 12, ...base, selectiveLogic: 2 })], 4096).before.includes('S'),
    'NOT_ANY 次级全不中 → 激活',
  )
  ok(
    !checkWorldInfo(['图书馆，外面下雨'], [mk({ uid: 13, ...base, selectiveLogic: 2 })], 4096).before.includes('S'),
    'NOT_ANY 次级命中 → 不激活',
  )
  // AND_ALL(3)：全部次级命中
  ok(
    checkWorldInfo(['图书馆里看雨'], [mk({ uid: 14, key: ['图书馆'], keysecondary: ['雨', '书'], selective: true, selectiveLogic: 3, content: 'S' })], 4096).before.includes('S'),
    'AND_ALL 全部次级命中 → 激活',
  )
  // NOT_ALL(1)：主命中 + 任一次级不中
  ok(
    checkWorldInfo(['图书馆里看书'], [mk({ uid: 15, key: ['图书馆'], keysecondary: ['雨', '书'], selective: true, selectiveLogic: 1, content: 'S' })], 4096).before.includes('S'),
    'NOT_ALL 有次级不中 → 激活',
  )
}

console.log('\n[C] 扫描深度与概率')
{
  // depth=2：只扫最近 2 条，第 3 条前的关键词不生效
  const entry = mk({ uid: 20, key: ['旧词'], content: 'OLD' })
  ok(!checkWorldInfo(['旧词', '新词一', '新词二'], [entry], 4096).before.includes('OLD'), '默认深度 2：更早消息不参与扫描')
  ok(
    checkWorldInfo(['旧词', '新词一', '新词二'], [entry], 4096, { scanDepth: 3 }).before.includes('OLD'),
    '深度 3：第三条参与扫描',
  )
  // 条目级 scanDepth 覆盖
  ok(
    checkWorldInfo(['旧词', '一', '二'], [mk({ uid: 21, key: ['旧词'], content: 'OLD', scanDepth: 3 })], 4096).before.includes('OLD'),
    '条目级 scanDepth 覆盖全局',
  )

  // 概率 0 → 永不激活；概率不启用 → 总激活
  let anyHit = false
  for (let i = 0; i < 20; i++) {
    if (checkWorldInfo(['词'], [mk({ uid: 22, key: ['词'], content: 'P', useProbability: true, probability: 0 })], 4096).before) anyHit = true
  }
  ok(!anyHit, 'probability=0 永不激活')
  ok(
    !!checkWorldInfo(['词'], [mk({ uid: 23, key: ['词'], content: 'P', useProbability: false, probability: 1 })], 4096).before,
    'useProbability=false 不掷骰直接激活',
  )
}

console.log('\n[D] 预算与排序')
{
  // 预算 = 25% × maxContext；order 高的先占；越过预算线的那条本身也被丢弃（同 ST）
  const entries = [
    mk({ uid: 30, key: ['词'], content: 'A'.repeat(400), order: 10 }), // ≈101 tok
    mk({ uid: 31, key: ['词'], content: 'B'.repeat(400), order: 5 }), // ≈101 tok
    mk({ uid: 32, key: ['词'], content: 'C'.repeat(400), order: 1 }), // 放不下
  ]
  const r = checkWorldInfo(['词'], entries, 1000) // 预算 = 250 tok
  ok(r.before.includes('AAAA') && r.before.includes('BBBB'), 'order 高的先入预算')
  ok(!r.before.includes('CCCC'), '预算放不下的条目被丢弃')
  ok(r.usedTokens > 0 && r.usedTokens < 250, 'usedTokens 统计且未超预算')
  // 极小预算：谁都放不下 → 全部不激活（同 ST 行为）
  const r2 = checkWorldInfo(['词'], entries, 200) // 预算 50，A 就越线
  ok(!r2.before, '极小预算下全部丢弃（ST 语义）')
}

console.log('\n[E] 位置分桶与 buildPrompt 注入')
{
  const entries = [
    mk({ uid: 40, key: ['词'], content: '前置设定', position: 0 }),
    mk({ uid: 41, key: ['词'], content: '后置设定', position: 1 }),
    mk({ uid: 42, key: ['词'], content: '深度注入', position: 4, depth: 2 }),
  ]
  const r = checkWorldInfo(['词'], entries, 8192)
  ok(r.before === '前置设定', 'position=0 → before')
  ok(r.after === '后置设定', 'position=1 → after')
  ok(r.depthEntries.length === 1 && r.depthEntries[0].depth === 2 && r.depthEntries[0].role === 'system', 'position=4 → depthEntries')

  const char: StCharacter = {
    name: '测试角色',
    description: '角色描述',
    personality: '性格',
    scenario: '场景',
    first_mes: '开场',
    mes_example: '',
    avatar: 'test.png',
    chat: '',
    create_date: '',
    talkativeness: 0.5,
    fav: false,
  }
  const history = [
    { name: '测试角色', is_user: false, mes: '第一轮', send_date: '2026-01-01', extra: {} },
    { name: '我', is_user: true, mes: '词', send_date: '2026-01-01', extra: {} },
  ]
  const built = buildPrompt({
    character: char,
    history: history as never,
    userName: '我',
    maxContext: 8192,
    worldInfo: { before: r.before, after: r.after, depthEntries: r.depthEntries },
  })
  const sys = built.messages[0].content
  const sysParts = sys.split('\n\n')
  ok(sysParts.indexOf('前置设定') < sysParts.indexOf('角色描述'), 'before 在角色描述之前')
  ok(sysParts.indexOf('后置设定') > sysParts.indexOf('### Scenario'), 'after 在场景之后')
  // 深度注入：depth=2 → 聊天流（不含 system）倒数第 2 条之前 → 紧跟 system 之后
  ok(
    built.messages.length === 4 && built.messages[1].role === 'system' && built.messages[1].content === '深度注入',
    '@Depth 条目按深度插入聊天流',
  )
}

/* ================= B. 集成测试（真实 ST 世界书，可选） ================= */
async function integration(): Promise<void> {
  console.log('\n[F] 集成：真实 ST 世界书（青江大学校园世界书）')
  try {
    const base = 'http://127.0.0.1:8000'
    // CSRF 会话 = 2 个 Cookie，必须合并携带（Node fetch 不自动管 Cookie）
    const r1 = await fetch(`${base}/csrf-token`)
    const csrf = (await r1.json()) as { token: string }
    const cookie = r1.headers
      .getSetCookie()
      .map((c) => c.split(';')[0])
      .join('; ')
    const book = (await (
      await fetch(`${base}/api/worldinfo/get`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf.token, Cookie: cookie },
        body: JSON.stringify({ name: '青江大学校园世界书' }),
      })
    ).json()) as { entries: Record<string, Record<string, unknown>> }

    const entries: WiEntry[] = Object.values(book.entries ?? {}).map((e) => ({
      ...(e as object),
      world: '青江大学校园世界书',
    })) as WiEntry[]
    console.log(`  真实条目数：${entries.length}`)
    ok(entries.length > 0, '真实世界书可加载')

    // 用第一条关键词构造历史，验证能激活出真实内容
    const first = entries.find((e) => !e.disable && Array.isArray(e.key) && e.key.length && (e.content ?? '').trim())
    if (first) {
      const kw = first.key.find((k) => k.trim()) ?? ''
      const texts = [`我们来聊聊${kw}吧`]
      const r = checkWorldInfo(texts, entries, 8192)
      const all = r.before + r.after + r.depthEntries.map((d) => d.content).join('')
      ok(all.includes(String(first.content).slice(0, 20).trim()), `真实条目激活（关键词「${kw}」）`)
      console.log(`  激活条目数：${r.activated.length}，估算 ${r.usedTokens} tok`)
    } else {
      console.log('  （书中无可测条目，跳过激活断言）')
    }
  } catch (e) {
    console.log(`  ⚠ ST 未运行或拉取失败，跳过集成测试：${e instanceof Error ? e.message : e}`)
  }
}

await integration()
console.log(`\n结果：${pass} 通过 / ${fail} 失败`)
process.exit(fail ? 1 : 0)
