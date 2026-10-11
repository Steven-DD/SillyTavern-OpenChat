/**
 * 风月前置/后置词自检：
 * 1. 世界书 position 7/8（userPrefix/userSuffix）分桶
 * 2. buildPrompt 对每条 user 消息的 `[作品前置 + WI前置] 输入 [WI后置 + 作品后置]` 包裹
 * 3. chat_metadata.fy_* 读写（chatPrefixSuffix/applyPrefixSuffix）
 */
const { checkWorldInfo, WI_POSITION } = await import('../src/services/st/worldinfo.ts')
const { buildPrompt } = await import('../src/services/st/prompt.ts')
const { chatPrefixSuffix, applyPrefixSuffix, chatFyExtras, applyFyExtras, chatUserPrompt, applyUserPrompt } = await import('../src/services/st/chatdoc.ts')

let fail = 0
const eq = (a, e, n) => { if (JSON.stringify(a) !== JSON.stringify(e)) { fail++; console.error(`✗ ${n}\n  期望 ${JSON.stringify(e)} 实得 ${JSON.stringify(a)}`) } }
const ok = (c, n) => { if (!c) { fail++; console.error(`✗ ${n}`) } }
const note = (s) => console.log(`  ${s}`)

/* ---------- 1. 世界书 position 7/8 分桶 ---------- */
{
  note('— WI position 7/8 分桶 —')
  const mk = (over) => ({
    uid: 0, world: 'fy', key: ['老周'], keysecondary: [], comment: '', content: '',
    constant: false, selective: true, secondary_logic: 0, order: 100, position: WI_POSITION.before,
    disable: false, addMemo: true, excludeRecursion: false, ...over,
  })
  const entries = [
    mk({ uid: 1, position: WI_POSITION.userPrefix, content: 'P1', order: 10 }),
    mk({ uid: 2, position: WI_POSITION.userPrefix, content: 'P2', order: 20 }),
    mk({ uid: 3, position: WI_POSITION.userSuffix, content: 'S1', order: 5 }),
    mk({ uid: 4, position: WI_POSITION.before, content: 'BEFORE' }),
  ]
  const res = checkWorldInfo(['玩家见到了老周'], entries, 4096)
  eq(res.userPrefix, 'P1\nP2', 'userPrefix 按 order 升序拼接')
  eq(res.userSuffix, 'S1', 'userSuffix 分桶')
  ok(res.before.includes('BEFORE'), 'before 桶不受影响')
  ok(!res.before.includes('P1') && !res.after.includes('P1'), 'position7 不进 before/after 桶')
}

/* ---------- 2. buildPrompt user 消息 wrap ---------- */
{
  note('— buildPrompt wrap —')
  const char = { name: '雪儿', description: '', personality: '', scenario: '', first_mes: '', mes_example: '' }
  const history = [
    { is_user: true, mes: 'U1', send_date: 1700000000000 },
    { is_user: false, mes: 'A1', send_date: 1700000001000 },
    { is_user: true, mes: 'U2', send_date: 1700000002000 },
  ]
  // 仅作品级前置/后置词
  let r = buildPrompt({
    character: char, history, userName: '小明',
    userPrefix: '[前置]', userSuffix: '[后置]',
  })
  eq(r.messages[1], { role: 'user', content: '[前置]\nU1\n[后置]' }, 'user 消息被包裹')
  eq(r.messages[2], { role: 'assistant', content: 'A1' }, 'assistant 不包裹')
  eq(r.messages[3], { role: 'user', content: '[前置]\nU2\n[后置]' }, '每条 user 都包裹')
  // 作品级 + 世界书缓冲区拼接顺序（风月约定：前后置词均 = App 先、世界书后）
  r = buildPrompt({
    character: char, history, userName: '小明',
    userPrefix: '作品前置', userSuffix: '作品后置',
    worldInfo: { before: '', after: '', userPrefix: 'WI前置', userSuffix: 'WI后置' },
  })
  eq(r.messages[1].content, '作品前置\nWI前置\nU1\n作品后置\nWI后置', '作品级与世界书缓冲区拼接顺序')
  // 两侧均空 → 原样
  r = buildPrompt({ character: char, history, userName: '小明' })
  eq(r.messages[1].content, 'U1', '无前后置词时原样')
}

/* ---------- 3. AI 总结开启 → 历史固定保留最近一轮 ---------- */
{
  note('— 总结开启截短历史 —')
  const char = { name: '雪儿', description: '', personality: '', scenario: '', first_mes: '', mes_example: '' }
  const history = [1, 2, 3, 4, 5, 6].map((i) => ({
    is_user: i % 2 === 1,
    mes: `M${i}`,
    send_date: 1700000000000 + i,
  }))
  // 无总结：短历史不裁剪
  let r = buildPrompt({ character: char, history, userName: '小明' })
  eq(r.trimmed, 0, '无总结时不裁剪')
  eq(r.messages.length, 7, '无总结时全量历史（system + 6 条）')
  // 有总结：只留最后 2 条（最近一轮），总结进 system
  const memory = { content: '[Summary] 摘要内容', position: 0, depth: 4, role: 0 }
  r = buildPrompt({ character: char, history, userName: '小明', memory })
  eq(r.trimmed, 4, '有总结时裁掉 4 条')
  eq(r.messages.length, 3, '只留 system + 最近一轮 2 条')
  eq(r.messages[1], { role: 'user', content: 'M5' }, '倒数第 2 条为上一轮 user')
  eq(r.messages[2], { role: 'assistant', content: 'M6' }, '末条为本轮 assistant')
  ok(r.messages[0].content.includes('[Summary] 摘要内容'), '总结注入 system')
  // 总结 + wrap 共存
  r = buildPrompt({ character: char, history, userName: '小明', memory, userPrefix: '[前]' })
  eq(r.messages[1].content, '[前]\nM5', '截短后 user 消息仍带前置词')
}

/* ---------- 4. Mod/用户自定义提示词层（System 叠加序） ---------- */
{
  note('— Mod/用户自定义层 —')
  const char = { name: '雪儿', description: '', personality: '', scenario: '', first_mes: '', mes_example: '' }
  const history = [{ is_user: true, mes: 'U1', send_date: 1700000000000 }]
  const r = buildPrompt({
    character: char, history, userName: '小明',
    modPrompt: 'MOD层内容', userPrompt: 'USER层内容',
  })
  const sys = r.messages[0].content
  ok(sys.includes("Write 雪儿's next reply"), 'main 在 System 中')
  ok(sys.includes('MOD层内容') && sys.includes('USER层内容'), 'Mod/用户层注入 System')
  const iMain = sys.indexOf("Write 雪儿's next reply")
  const iMod = sys.indexOf('MOD层内容')
  const iUser = sys.indexOf('USER层内容')
  ok(iMain < iMod && iMod < iUser, '叠加序 = main → Mod → 用户自定义')
  // 空层不注入
  const r2 = buildPrompt({ character: char, history, userName: '小明' })
  ok(!r2.messages[0].content.includes('MOD层内容'), '未配置时不注入')
  // chatdoc fy_user_prompt 读写
  const chat = [{ chat_metadata: {} }]
  ok(chatUserPrompt(chat) === '', 'userPrompt 未设置 → 空')
  applyUserPrompt(chat, '自定义内容')
  eq(chatUserPrompt(chat), '自定义内容', 'userPrompt 写入读回')
  applyUserPrompt(chat, '  ')
  ok(chatUserPrompt(chat) === '' && !('fy_user_prompt' in chat[0].chat_metadata), '空白 = 清除')
}

/* ---------- 5. chat_metadata.fy_* 读写 ---------- */
{
  note('— chatdoc fy_* —')
  const chat = [{ chat_metadata: {} }]
  ok(chatPrefixSuffix(chat) === null, '未设置 → null')
  applyPrefixSuffix(chat, { prefix: '前', suffix: '后' })
  eq(chatPrefixSuffix(chat), { prefix: '前', suffix: '后' }, '写入后读回')
  applyPrefixSuffix(chat, { prefix: '  ', suffix: '' })
  ok(chatPrefixSuffix(chat) === null, '全空白 = 清除')
  ok(!('fy_prefix' in chat[0].chat_metadata), '键已删除')
  // BGM / 内置 CSS（逐字段清空回落语义）
  ok(chatFyExtras(chat) === null, 'fy 外观未设置 → null')
  applyFyExtras(chat, { bgm: 'https://x/bgm.mp3', css: '.a{color:red}' })
  eq(chatFyExtras(chat), { bgm: 'https://x/bgm.mp3', css: '.a{color:red}' }, '外观写入读回')
  applyFyExtras(chat, { bgm: '', css: '.b{}' })
  eq(chatFyExtras(chat), { bgm: '', css: '.b{}' }, '空 bgm 字段保留、回落角色卡')
  ok(!('fy_bgm' in chat[0].chat_metadata), '空 bgm 键已删除')
  applyFyExtras(chat, { bgm: '', css: '' })
  ok(chatFyExtras(chat) === null, '外观全空 = 清除')
}

console.log(fail ? `\n${fail} 失败` : '\n风月前置/后置词自检全部通过')
process.exit(fail ? 1 : 0)
