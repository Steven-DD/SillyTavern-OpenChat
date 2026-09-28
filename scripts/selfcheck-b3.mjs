/**
 * B3 批次自检（回归脚本；用法同 selfcheck-macros.mjs）
 * 覆盖：世界书递归扫描、timedEffects（sticky/cooldown/delay）、摘要提示词、向量 hash/集合名
 */
globalThis.localStorage = { getItem: () => null, setItem: () => {}, removeItem: () => {} }
const { checkWorldInfo } = await import('../src/services/st/worldinfo.ts')
const { generateSummaryPrompt } = await import('../src/services/st/memory.ts')
const { hashMessage, vectorsCollectionId } = await import('../src/services/st/vectors.ts')

let fail = 0
const ok = (c, n) => { if (!c) { fail++; console.error(`✗ ${n}`) } }
const eq = (a, e, n) => { if (a !== e) { fail++; console.error(`✗ ${n}（期望 ${JSON.stringify(e)} 实得 ${JSON.stringify(a)}）`) } }

const mk = (uid, p = {}) => ({
  uid, key: [], keysecondary: [], comment: '', content: `C${uid}`, constant: false,
  selective: false, order: 100, position: 0, ...p,
})

/* ---- timedEffects: delay ---- */
{
  const entries = [mk(1, { constant: true, delay: 3 })]
  const state = {}
  const r1 = checkWorldInfo(['x'], entries, 8192, {}, { state, turn: 2 })
  eq(r1.activated.length, 0, 'delay: turn<3 不激活')
  const r2 = checkWorldInfo(['x'], entries, 8192, {}, { state, turn: 3 })
  eq(r2.activated.length, 1, 'delay: turn>=3 激活')
}

/* ---- timedEffects: sticky（ST 区间语义，world-info.js:604-655：end = 触发轮 + N，
 *      chat.length >= end 即过期【排他】→ sticky=N = 触发轮后再强制 N-1 轮） ---- */
{
  const entries = [mk(1, { key: ['关键词'], sticky: 2 })]
  const state = {}
  const r1 = checkWorldInfo(['无关文本'], entries, 8192, {}, { state, turn: 10 })
  eq(r1.activated.length, 0, 'sticky: 未命中不激活')
  checkWorldInfo(['关键词'], entries, 8192, {}, { state, turn: 11 }) // 触发，记录 {start:11, end:13}
  const r2 = checkWorldInfo(['无关文本'], entries, 8192, {}, { state, turn: 12 })
  eq(r2.activated.length, 1, 'sticky: 触发后 1 轮内强制（12 < 13）')
  const r3 = checkWorldInfo(['无关文本'], entries, 8192, {}, { state, turn: 13 })
  eq(r3.activated.length, 0, 'sticky: 到 end 轮过期（排他区间，world-info.js:651）')
  const r4 = checkWorldInfo(['无关文本'], entries, 8192, {}, { state, turn: 14 })
  eq(r4.activated.length, 0, 'sticky: 过期后保持失效')
}

/* ---- min_activations：激活不足时逐步扩大扫描深度重扫 ---- */
{
  // 关键词在最旧一条消息里，默认扫描深度（2）扫不到
  const entries = [mk(1, { key: ['深藏关键词'] })]
  const texts = ['深藏关键词', '无关', '无关', '无关'] // 旧→新
  const off = checkWorldInfo(texts, entries, 8192, {}, { state: {}, turn: 5 })
  eq(off.activated.length, 0, 'min_act: 默认深度不激活')
  const on = checkWorldInfo(texts, entries, 8192, { minActivations: 1, minActivationsDepthMax: 4 }, { state: {}, turn: 5 })
  eq(on.activated.length, 1, 'min_act: 扩窗后激活')
  const capped = checkWorldInfo(texts, entries, 8192, { minActivations: 1, minActivationsDepthMax: 2 }, { state: {}, turn: 5 })
  eq(capped.activated.length, 0, 'min_act: 深度上限截断')
  // 已激活条目在扩窗重扫中不重复计数
  const dedup = checkWorldInfo(
    ['深藏关键词', '重复关键词', '无关', '无关'],
    [mk(1, { key: ['深藏关键词'] }), mk(2, { key: ['重复关键词'] })],
    8192,
    { minActivations: 2, minActivationsDepthMax: 4 },
    { state: {}, turn: 5 },
  )
  eq(dedup.activated.length, 2, 'min_act: 扩窗重扫不重复激活')
}

/* ---- timedEffects: cooldown（记录 {start, end=start+N}，区间内跳过、到 end 轮恢复） ---- */
{
  const entries = [mk(1, { key: ['关键词'], cooldown: 2 })]
  const state = {}
  checkWorldInfo(['关键词'], entries, 8192, {}, { state, turn: 1 }) // 触发，记录 {start:1, end:3}
  const r2 = checkWorldInfo(['关键词'], entries, 8192, {}, { state, turn: 2 })
  eq(r2.activated.length, 0, 'cooldown: 触发后第 1 轮跳过（2 < 3）')
  const r3 = checkWorldInfo(['关键词'], entries, 8192, {}, { state, turn: 3 })
  eq(r3.activated.length, 1, 'cooldown: 到 end 轮恢复激活（world-info.js:651）')
}

/* ---- sticky → cooldown 链：sticky 到期那一刻立即进入 protected cooldown（onEnded，:518-528） ---- */
{
  const entries = [mk(1, { key: ['关键词'], sticky: 2, cooldown: 2 })]
  const state = {}
  checkWorldInfo(['关键词'], entries, 8192, {}, { state, turn: 11 }) // 触发 sticky {11,13}
  const r2 = checkWorldInfo(['无关文本'], entries, 8192, {}, { state, turn: 12 })
  eq(r2.activated.length, 1, 'sticky→cd: sticky 区间内强制')
  const r3 = checkWorldInfo(['无关文本'], entries, 8192, {}, { state, turn: 13 })
  eq(r3.activated.length, 0, 'sticky→cd: sticky 到期当轮即被 cooldown 压制（立即生效）')
  const r4 = checkWorldInfo(['关键词'], entries, 8192, {}, { state, turn: 14 })
  eq(r4.activated.length, 0, 'sticky→cd: cooldown 区间内关键词命中也不激活')
  const r5 = checkWorldInfo(['关键词'], entries, 8192, {}, { state, turn: 15 })
  eq(r5.activated.length, 1, 'sticky→cd: cooldown 到期恢复激活（end=15 排他）')
}

/* ---- 递归扫描：二跳激活 ---- */
{
  // 条目 A 常驻，内容含 "引导词"；条目 B 关键词 "引导词"（首轮扫描文本里没有）
  const entries = [
    mk(1, { constant: true, content: '这是引导词的来源' }),
    mk(2, { key: ['引导词'], content: 'B 被递归激活' }),
  ]
  const off = checkWorldInfo(['x'], entries, 8192, { recursive: false })
  eq(off.activated.map((a) => a.uid).join(), '1', '递归关闭：B 不激活')
  const on = checkWorldInfo(['x'], entries, 8192, { recursive: true })
  eq(on.activated.map((a) => a.uid).sort().join(), '1,2', '递归开启：B 二跳激活')
}

/* ---- 摘要提示词 ---- */
{
  const p = generateSummaryPrompt('旧摘要', ['老板: 你好', '爱丽丝: 你好呀'], '爱丽丝', '老板')
  ok(p.includes('旧摘要'), '摘要: 包含上一版摘要')
  ok(p.includes('老板: 你好'), '摘要: 包含历史')
  ok(p.includes('200'), '摘要: 词数限制')
}

/* ---- 向量 ---- */
eq(hashMessage('abc'), hashMessage('abc'), '向量: hash 稳定')
ok(hashMessage('abc') !== hashMessage('abd'), '向量: hash 区分文本')
eq(vectorsCollectionId('Seraphina.png', 'Chat - 2026/9·27'), 'stchat-seraphina-png-chat-2026-9-27', '向量: 集合名 sanitize')

console.log(fail ? `${fail} 失败` : '全部通过')
process.exit(fail ? 1 : 0)
