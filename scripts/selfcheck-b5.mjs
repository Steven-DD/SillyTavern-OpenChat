/**
 * B5 自检：STscript 解析/管道/变量/if + Instruct 拼接（回归脚本，用法同 selfcheck-macros.mjs）
 */
const { parseScript, runScript } = await import('../src/services/st/stscript.ts')
const { formatInstructChat } = await import('../src/services/st/instruct.ts')

let fail = 0
const eq = (a, e, n) => { if (JSON.stringify(a) !== JSON.stringify(e)) { fail++; console.error(`✗ ${n}\n  期望 ${JSON.stringify(e)} 实得 ${JSON.stringify(a)}`) } }
const ok = (c, n) => { if (!c) { fail++; console.error(`✗ ${n}`) } }
const note = (s) => console.log(`  ${s}`)

/* ---------- parseScript ---------- */
{
  note('— parseScript —')
  eq(parseScript('/echo a | /send b').length, 2, '管道分段')
  eq(parseScript('/echo a | /send b')[0].name, 'echo', '命令名剥离斜杠')
  eq(parseScript('/echo "a|b"').length, 1, '引号内管道不分割')
  eq(parseScript('/random a,b,c')[0].args, 'a,b,c', '参数保留')
  eq(parseScript('echo x')[0].name, 'echo', '斜杠可省')
}

/* ---------- runScript：echo/变量/管道/if/random ---------- */
{
  note('— runScript —')
  const vars = new Map()
  const mkCtx = (extra = {}) => ({
    vars: {
      get: (k) => vars.get(k),
      set: (k, v) => vars.set(k, v),
      has: (k) => vars.has(k),
      delete: (k) => vars.delete(k),
    },
    expand: (t) => t.replace(/\{\{\s*getvar::([\w:-]+)\s*\}\}/gi, (_, k) => String(vars.get(k) ?? '')),
    send: async () => {},
    genraw: async (p) => `GENRAW:${p}`,
    echo: () => {},
    ...extra,
  })

  // setvar → getvar 管道
  let out = ''
  {
    const ctx = mkCtx()
    out = await runScript('/setvar gold=100 | /getvar gold', ctx)
    eq(out, '100', 'setvar 后 getvar 经管道返回')
    eq(ctx.vars.get('gold'), '100', '变量已写入')
  }
  // addvar 数值加
  {
    const ctx = mkCtx()
    await runScript('/setvar n=5 | /addvar n=10', ctx)
    eq(ctx.vars.get('n'), 15, 'addvar 数值累加')
  }
  // genraw 输出进入管道
  {
    let echoOut = ''
    await runScript('/genraw 写首诗 | /echo 收到：{{pipe}}', mkCtx({
      echo: (t) => { echoOut = t },
    }))
    eq(echoOut, '收到：GENRAW:写首诗', 'genraw → pipe → echo')
  }
  // if 条件真/假（getvar 展开）
  {
    const ctx = mkCtx()
    let echoText = ''
    ctx.echo = (t) => { echoText = t }
    await runScript('/setvar gold=200', ctx)
    await runScript('/if {{getvar::gold}} == 200 | /echo 富', ctx)
    eq(echoText, '富', 'if 条件真执行 then 链')
    echoText = ''
    await runScript('/if {{getvar::gold}} == 1 | /echo 不该出现', ctx)
    eq(echoText, '', 'if 条件假跳过')
  }
  // random 数值范围
  {
    for (let i = 0; i < 20; i++) {
      const v = await runScript('/random 1,6', mkCtx())
      const n = Number(v)
      ok(Number.isInteger(n) && n >= 1 && n <= 6, `random 1,6 范围内（got ${v}）`)
      if (fail) break
    }
  }
  // 未知命令
  {
    try {
      await runScript('/notacmd', mkCtx())
      ok(false, '未知命令应抛错')
    } catch (e) {
      ok(/未知命令/.test(e.message), '未知命令抛错')
    }
  }
}

/* ---------- Instruct 拼接 ---------- */
{
  note('— formatInstructChat —')
  const messages = [
    { role: 'system', content: 'SYS' },
    { role: 'user', content: 'U1' },
    { role: 'assistant', content: 'A1' },
    { role: 'user', content: 'U2' },
  ]
  // 基本模板（对齐 ST mistral 风格）
  const ins = {
    enabled: true,
    wrap: true,
    names_behavior: 'none',
    input_sequence: '[INST] ',
    input_suffix: ' [/INST]\n',
    output_sequence: '',
    output_suffix: '</s>\n',
    system_sequence: '',
    system_suffix: '\n',
  }
  const r = formatInstructChat(messages, ins, '小明', '雪儿')
  // ST wrap 语义：prefix 与 body 之间 '\n' 连接（formatInstructModeChat:453-454）
  ok(r.text.includes('小明: U1') === false, 'names=none 不加名')
  ok(r.text.includes('[INST] \nU1 [/INST]'), 'user 前后缀包裹（wrap 换行连接）')
  ok(r.text.includes('A1</s>'), 'output 后缀')
  ok(r.text.endsWith(' [/INST]\n'), '末尾停在 user 段（output 前缀为空不追加）')
  // names_behavior always + {{name}} 宏
  const ins2 = { ...ins, names_behavior: 'always', input_sequence: '{{name}}: ' }
  const r2 = formatInstructChat(messages, ins2, '小明', '雪儿')
  ok(r2.text.includes('小明: U1'), 'always + {{name}} → 用户名前缀')
  // stop 序列：只含各 sequence（ST combined_sequence 不含 suffix）
  ok(r.stop.includes('[INST]'), 'stop 含 input 序列')
  ok(!r.stop.includes('</s>'), 'stop 不含 output 后缀（对齐 ST）')
  // first/last 变体
  const ins3 = {
    ...ins,
    first_input_sequence: '<<FIRST>> ',
    last_input_sequence: '<<LAST>> ',
    first_output_sequence: '<<FOUT>> ',
  }
  const r3 = formatInstructChat(messages, ins3, '小明', '雪儿')
  ok(r3.text.includes('<<FIRST>> \nU1'), '首条 user 用 first_input')
  ok(r3.text.includes('<<LAST>> \nU2'), '末条 user 用 last_input')
  // P1-2 对齐 ST（script.js:4784-4787）：first_* 只作用于历史第 0 条。
  // 此处第 0 条（system 之后的首条）是 user U1，assistant A1 不该带 first_output
  ok(!r3.text.includes('<<FOUT>>'), 'first_output 不落在非第 0 条的 assistant（对齐 ST）')
  // 历史第 0 条是 assistant（开场白场景）→ first_output 生效于它
  const r5 = formatInstructChat(
    [{ role: 'assistant', content: 'GREET' }, { role: 'user', content: 'U1' }],
    ins3, '小明', '雪儿',
  )
  ok(r5.text.includes('<<FOUT>> \nGREET'), '第 0 条为 assistant（开场白）时 first_output 生效')
  // wrap=false 直接连接（suffix 不含换行时整段无 \n）
  const r4 = formatInstructChat(
    [{ role: 'user', content: 'X' }],
    { ...ins, wrap: false, input_suffix: ' [/INST]', output_suffix: '</s>' },
    'a', 'b',
  )
  ok(!r4.text.includes('\n'), 'wrap=false 无换行')
}

console.log(fail ? `\n${fail} 失败` : '\nB5 自检全部通过')
process.exit(fail ? 1 : 0)
