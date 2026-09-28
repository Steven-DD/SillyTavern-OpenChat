/**
 * B4 群聊编排自检（回归脚本；用法同 selfcheck-macros.mjs）
 * 断言对齐 ST 1.19.0 group-chats.js 激活策略语义。
 */
globalThis.localStorage = { getItem: () => null, setItem: () => {}, removeItem: () => {} }
const { pickSpeakers, parseGroupSessionKey, groupSessionKey, GROUP_ACTIVATION_STRATEGY } =
  await import('../src/services/st/group-orchestrate.ts')

let fail = 0
const eq = (a, e, n) => {
  if (JSON.stringify(a) !== JSON.stringify(e)) {
    fail++
    console.error(`✗ ${n}\n  期望 ${JSON.stringify(e)} 实得 ${JSON.stringify(a)}`)
  }
}
const ok = (c, n) => {
  if (!c) {
    fail++
    console.error(`✗ ${n}`)
  }
}

const base = { members: ['a.png', 'b.png', 'c.png'] }
const meta = {
  'a.png': { avatar: 'a.png', name: 'Alice', talkativeness: 0.5 },
  'b.png': { avatar: 'b.png', name: 'Bob', talkativeness: 0.5 },
  'c.png': { avatar: 'c.png', name: 'Cindy', talkativeness: 0.5 },
}
// 全员 talk=0：掷骰必不命中
const metaMute = {
  'a.png': { avatar: 'a.png', name: 'Alice', talkativeness: 0 },
  'b.png': { avatar: 'b.png', name: 'Bob', talkativeness: 0 },
  'c.png': { avatar: 'c.png', name: 'Cindy', talkativeness: 0 },
}

/* LIST(1)：启用成员按表序全部发言 */
{
  eq(pickSpeakers({ ...base, activationStrategy: 1, meta }).map((s) => s.avatar), ['a.png', 'b.png', 'c.png'], 'LIST: 全员按表序')
  eq(
    pickSpeakers({ ...base, activationStrategy: 1, meta, disabledMembers: ['a.png'] }).map((s) => s.avatar),
    ['b.png', 'c.png'],
    'LIST: 禁用成员排除',
  )
}

/* NATURAL(0)：@mentions + talkativeness 掷骰 */
{
  // mentions 命中（random=0.9 → 掷骰不激活，只剩 mentions）
  eq(
    pickSpeakers({ ...base, activationStrategy: 0, meta: metaMute, isUserInput: true, activationText: 'hey Bob', random: () => 0.9 }).map((s) => s.avatar),
    ['b.png'],
    'NATURAL: @mentions 激活 Bob',
  )
  // 掷骰命中（random=0 → 全员激活；乱序掷骰 → 顺序为 shuffle 结果）
  const all = pickSpeakers({ ...base, activationStrategy: 0, meta, isUserInput: true, activationText: '', random: () => 0 })
  eq(all.map((s) => s.avatar), ['b.png', 'c.png', 'a.png'], 'NATURAL: talk≥roll 全员激活（乱序）')
  // 全员 talk=0 → 兜底随机 1 人
  eq(pickSpeakers({ ...base, activationStrategy: 0, meta: metaMute, isUserInput: true, activationText: '', random: () => 0.9 }).length, 1, 'NATURAL: 无人命中兜底 1 人')
  // 非 用户输入 触发：禁上一位发言者连说（mentions + 掷骰均跳过）
  eq(
    pickSpeakers({ ...base, activationStrategy: 0, meta: metaMute, isUserInput: false, activationText: 'Alice', lastSpeakerName: 'Alice', random: () => 0 }).map((s) => s.avatar),
    ['b.png', 'c.png'],
    'NATURAL: 上位发言者被禁',
  )
  // allow_self_responses 豁免
  ok(
    pickSpeakers({ ...base, activationStrategy: 0, meta: metaMute, isUserInput: false, activationText: 'Alice', lastSpeakerName: 'Alice', random: () => 0, allowSelfResponses: true }).some((s) => s.avatar === 'a.png'),
    'NATURAL: allow_self_responses 豁免禁言',
  )
}

/* MANUAL(2)：用户输入不激活；非用户输入 shuffle 取 1 人 */
{
  eq(pickSpeakers({ ...base, activationStrategy: 2, meta, isUserInput: true }), [], 'MANUAL: 用户输入不激活任何人')
  // random=0 时 Fisher-Yates 结果确定为 [b,a,c] → 取 b
  eq(pickSpeakers({ ...base, activationStrategy: 2, meta, isUserInput: false, random: () => 0 }).map((s) => s.avatar), ['b.png'], 'MANUAL: 非用户输入随机 1 人')
}

/* POOLED(3)：优先未发言者；否则排除上一位发言者 */
{
  eq(
    pickSpeakers({ ...base, activationStrategy: 3, meta, spokenSinceUser: ['a.png'], random: () => 0 }).map((s) => s.avatar),
    ['b.png'],
    'POOLED: 未发言者优先（跳过已发言 a）',
  )
  eq(
    pickSpeakers({ ...base, activationStrategy: 3, meta, spokenSinceUser: ['a.png', 'b.png', 'c.png'], lastSpeakerAvatar: 'a.png', random: () => 0 }).map((s) => s.avatar),
    ['b.png'],
    'POOLED: 全发言后随机排除上一位',
  )
}

/* 会话 key */
eq(parseGroupSessionKey('group::g1::chat1'), { groupId: 'g1', chatId: 'chat1' }, 'key: 解析')
eq(parseGroupSessionKey('avatar::file'), null, 'key: 非群聊 key 返回 null')
eq(groupSessionKey('g1', 'chat1'), 'group::g1::chat1', 'key: 构造')
eq(GROUP_ACTIVATION_STRATEGY.MANUAL, 2, '枚举: MANUAL=2（1.19 语义）')

console.log(fail ? `${fail} 失败` : '全部通过')
process.exit(fail ? 1 : 0)
