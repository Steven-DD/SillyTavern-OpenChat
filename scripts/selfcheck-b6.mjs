/**
 * B6 自检：is_system 过滤 + /sys 命令（M-20）；回归脚本，用法同 selfcheck-b5.mjs
 */
const { buildPrompt } = await import('../src/services/st/prompt.ts')
const { parseScript, runScript } = await import('../src/services/st/stscript.ts')
const { toDisplayMessages, toStMessages } = await import('../src/services/st/chatdoc.ts')

let fail = 0
const eq = (a, e, n) => { if (JSON.stringify(a) !== JSON.stringify(e)) { fail++; console.error(`✗ ${n}\n  期望 ${JSON.stringify(e)} 实得 ${JSON.stringify(a)}`) } }
const ok = (c, n) => { if (!c) { fail++; console.error(`✗ ${n}`) } }
const note = (s) => console.log(`  ${s}`)

/* ---------- chatdoc：is_system 保留进显示模型、写回 ---------- */
{
  note('— chatdoc is_system —')
  const raw = [
    { chat_metadata: {}, user_name: 'u', character_name: 'c' },
    { name: 'c', is_user: false, mes: '正常', send_date: 1 },
    { name: 'System', is_user: false, is_system: true, mes: '旁白', send_date: 2 },
  ]
  const dm = toDisplayMessages(raw)
  eq(dm.length, 2, '隐藏行保留（不再过滤）')
  eq(dm[1].isSystem, true, '标记 isSystem')
  const back = toStMessages(dm, 'c')
  eq(back[1].is_system, true, '写回保留 is_system')
  eq(back[0].is_system, undefined, '普通消息无 is_system')
}

/* ---------- prompt：is_system 不进 history ---------- */
{
  note('— prompt 过滤 —')
  const character = {
    name: '雪儿', description: '', personality: '', scenario: '',
    first_mes: '你好', mes_example: '', creatorcomment: '', tags: [], talkativeness: 0.5,
    create_date: '', chat: 'x', avatar: 'x.png',
  }
  const history = [
    { name: 'u', is_user: true, mes: '嗨', send_date: 1 },
    { name: 'System', is_user: false, is_system: true, mes: 'HIDDEN-MSG', send_date: 2 },
    { name: 'c', is_user: false, mes: '你好呀', send_date: 3 },
  ]
  const r = buildPrompt({ character, history, userName: 'u', maxContext: 8192 })
  const joined = r.messages.map((m) => m.content).join('\n')
  ok(!joined.includes('HIDDEN-MSG'), '隐藏消息不进 prompt')
  ok(joined.includes('嗨') && joined.includes('你好呀'), '正常消息保留')
}

/* ---------- stscript /sys ---------- */
{
  note('— /sys —')
  eq(parseScript('/sys 夜深了').length, 1, '/sys 解析')
  {
    let sysText = ''
    const ctx = {
      vars: { get: () => undefined, set: () => {}, has: () => false, delete: () => {} },
      expand: (t) => t,
      send: async () => {},
      genraw: async () => '',
      echo: () => {},
      sys: async (t) => { sysText = t },
    }
    await runScript('/sys 夜深了', ctx)
    eq(sysText, '夜深了', '/sys 调用 ctx.sys')
  }
  {
    // 无 sys 上下文 → 报错（如在脚本测试环境中）
    const ctx = {
      vars: { get: () => undefined, set: () => {}, has: () => false, delete: () => {} },
      expand: (t) => t,
      send: async () => {},
      genraw: async () => '',
      echo: () => {},
    }
    try {
      await runScript('/sys x', ctx)
      ok(false, '无 sys 上下文应抛错')
    } catch (e) {
      ok(/sys/.test(e.message), '抛出 /sys 可用性错误')
    }
  }
}

console.log(fail ? `\n${fail} 失败` : '\nB6 自检全部通过')
process.exit(fail ? 1 : 0)
