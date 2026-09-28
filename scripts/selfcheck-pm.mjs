/**
 * B2 Prompt Manager 自检（回归脚本；用法同 selfcheck-macros.mjs）
 */
globalThis.localStorage = { getItem: () => null, setItem: () => {}, removeItem: () => {} }
const { buildPrompt } = await import('../src/services/st/prompt.ts')

let fail = 0
const eq = (a, e, n) => {
  if (JSON.stringify(a) !== JSON.stringify(e)) { fail++; console.error(`✗ ${n}\n  期望 ${JSON.stringify(e)} 实得 ${JSON.stringify(a)}`) }
}
const ok = (c, n) => { if (!c) { fail++; console.error(`✗ ${n}`) } }

const char = {
  name: '爱丽丝', avatar: 'a.png', description: 'DESC', personality: 'PERS', scenario: 'SCEN',
  first_mes: '', mes_example: 'EX1', tags: [], creator: '', character_version: '',
}

const pm = {
  order: [
    { identifier: 'main', enabled: true },
    { identifier: 'worldInfoBefore', enabled: true },
    { identifier: 'charDescription', enabled: true },
    { identifier: 'chatHistory', enabled: true },
    { identifier: 'jailbreak', enabled: true },
  ],
  main: 'CUSTOM MAIN',
  nsfw: '',
  jailbreak: 'JB AFTER HISTORY',
}

/* 有序装配 + post-history */
const r1 = buildPrompt({ character: char, history: [], userName: '老板', pm })
ok(r1.systemPrompt.includes('CUSTOM MAIN'), 'PM: 自定义 main 生效')
ok(r1.systemPrompt.includes('DESC'), 'PM: charDescription 进系统提示')
ok(r1.systemPrompt.indexOf('CUSTOM MAIN') < r1.systemPrompt.indexOf('DESC'), 'PM: main 在 description 前')
eq(r1.messages.length, 2, 'PM: system + post(jailbreak)')
eq(r1.messages[1].role, 'system', 'PM: post-history 为 system')
eq(r1.messages[1].content, 'JB AFTER HISTORY', 'PM: jailbreak 在历史后')

/* enabled=false 跳过 + 未支持标识跳过 */
const r2 = buildPrompt({
  character: char, history: [], userName: '老板',
  pm: {
    order: [
      { identifier: 'main', enabled: true },
      { identifier: 'charDescription', enabled: false },
      { identifier: 'enhanceDefinitions', enabled: true },
      { identifier: 'scenario', enabled: true },
    ],
    main: 'M2',
  },
})
ok(!r2.systemPrompt.includes('DESC'), 'PM: disabled 条目跳过')
ok(r2.systemPrompt.includes('SCEN'), 'PM: enabled 条目保留')
ok(!r2.messages.some((m) => m.content.includes('enhance')), 'PM: 未支持标识跳过')

/* 未传 pm → 传统装配不变 */
const r3 = buildPrompt({ character: char, history: [], userName: '老板' })
ok(r3.systemPrompt.startsWith("Write 爱丽丝's next reply"), '传统: main 指令开头')
ok(r3.systemPrompt.includes('DESC') && r3.systemPrompt.includes('EX1'), '传统: 字段与示例齐全')
eq(r3.messages.length, 1, '传统: 无 post-history')

/* dialogueExamples 受 includeExamples 控制（PM 模式） */
const r4 = buildPrompt({ character: char, history: [], userName: '老板', includeExamples: false, pm })
ok(!r4.systemPrompt.includes('EX1'), 'PM: includeExamples=false 跳过示例')

/* impersonate 模板宏替换 */
const { withImpersonate } = await import('../src/services/st/prompt.ts')
const { emptyContext } = await import('../src/services/st/macros.ts')
const imp = withImpersonate(msgs(), '老板', '你是{{user}}的秘书', emptyContext({ userName: '老板' }))
eq(imp[imp.length - 1].role, 'system', 'impersonate: 模板为 system')
eq(imp[imp.length - 1].content, '你是老板的秘书', 'impersonate: 模板宏替换')
const imp2 = withImpersonate(msgs(), '老板')
eq(imp2[imp2.length - 1].role, 'system', 'impersonate: 无模板回落 ST 默认 impersonation prompt（P1-7，system 轮）')

function msgs() {
  return [{ role: 'system', content: 'S' }, { role: 'user', content: 'U' }]
}

console.log(fail ? `${fail} 失败` : '全部通过')
process.exit(fail ? 1 : 0)
