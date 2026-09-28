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

/* ---- timedEffects: sticky（激活后强制保持 N 轮） ---- */
{
  const entries = [mk(1, { key: ['关键词'], sticky: 2 })]
  const state = {}
  const r1 = checkWorldInfo(['无关文本'], entries, 8192, {}, { state, turn: 10 })
  eq(r1.activated.length, 0, 'sticky: 未命中不激活')
  checkWorldInfo(['关键词'], entries, 8192, {}, { state, turn: 11 })
  const r2 = checkWorldInfo(['无关文本'], entries, 8192, {}, { state, turn: 12 })
  eq(r2.activated.length, 1, 'sticky: 第 1 轮强制激活')
  const r3 = checkWorldInfo(['无关文本'], entries, 8192, {}, { state, turn: 13 })
  eq(r3.activated.length, 1, 'sticky: 第 2 轮仍强制（sticky=2）')
  const r4 = checkWorldInfo(['无关文本'], entries, 8192, {}, { state, turn: 14 })
  eq(r4.activated.length, 0, 'sticky: 计数耗尽失效')
}

/* ---- timedEffects: cooldown（激活后 N 轮内不可激活） ---- */
{
  const entries = [mk(1, { key: ['关键词'], cooldown: 2 })]
  const state = {}
  checkWorldInfo(['关键词'], entries, 8192, {}, { state, turn: 1 })
  const r2 = checkWorldInfo(['关键词'], entries, 8192, {}, { state, turn: 2 })
  eq(r2.activated.length, 0, 'cooldown: 激活后第 1 轮跳过')
  const r3 = checkWorldInfo(['关键词'], entries, 8192, {}, { state, turn: 3 })
  eq(r3.activated.length, 0, 'cooldown: 第 2 轮仍跳过')
  const r4 = checkWorldInfo(['关键词'], entries, 8192, {}, { state, turn: 4 })
  eq(r4.activated.length, 1, 'cooldown: 冷却结束恢复激活')
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
