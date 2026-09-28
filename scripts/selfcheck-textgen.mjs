/** textgen body/prompt 拼接自检（回归脚本；用法同 selfcheck-macros.mjs） */
globalThis.localStorage = { getItem: () => null, setItem: () => {}, removeItem: () => {} }
const { buildBody, messagesToPrompt, TEXTGEN_BACKENDS } = await import('../src/services/st/textgen.ts')

let fail = 0
const eq = (a, e, n) => { if (JSON.stringify(a) !== JSON.stringify(e)) { fail++; console.error(`✗ ${n}\n  期望 ${JSON.stringify(e)} 实得 ${JSON.stringify(a)}`) } }

// 15 后端齐全
eq(TEXTGEN_BACKENDS.length, 15, '后端数量 15')

// generic 本地后端：body 组装
const b1 = buildBody({
  prompt: 'PROMPT', model: 'm1', backend: 'generic', server: 'http://127.0.0.1:5001/',
  maxTokens: 256, sampler: { temperature: 0.8, topP: 0.9, repetitionPenalty: 1.1 },
})
eq(b1.api_type, 'generic', 'generic: api_type')
eq(b1.api_server, 'http://127.0.0.1:5001/v1/completions', 'generic: 端点拼接（去尾斜杠）')
eq(b1.prompt, 'PROMPT', 'generic: prompt 单字符串')
eq(b1.max_tokens, 256, 'generic: max_tokens')
eq(b1.temperature, 0.8, 'generic: temperature')
eq(b1.repetition_penalty, 1.1, 'generic: repetition_penalty')
eq(b1.stream, true, 'generic: stream')

// llamacpp：/completion + n_predict + repeat_penalty 别名
const b2 = buildBody({ prompt: 'P', model: 'm', backend: 'llamacpp', server: 'http://l:8080', maxTokens: 100, sampler: { repetitionPenalty: 1.2 } })
eq(b2.api_server, 'http://l:8080/completion', 'llamacpp: /completion')
eq(b2.n_predict, 100, 'llamacpp: n_predict')
eq(b2.repeat_penalty, 1.2, 'llamacpp: repeat_penalty 别名')
ok(!('repetition_penalty' in b2), 'llamacpp: 不带通用字段')

// ollama：/api/generate + raw + options
const b3 = buildBody({ prompt: 'P', model: 'llama3', backend: 'ollama', server: 'http://o:11434', maxTokens: 128, sampler: { temperature: 0.5 } })
eq(b3.api_server, 'http://o:11434/api/generate', 'ollama: /api/generate')
eq(b3.raw, true, 'ollama: raw')
eq(b3.options.num_predict, 128, 'ollama: options.num_predict')

// openrouter text：chat/completions 端点
const b4 = buildBody({ prompt: 'P', model: 'm', backend: 'openrouter', server: '' })
eq(b4.api_server, 'https://openrouter.ai/api/v1/chat/completions', 'openrouter: 固定服务器 + chat/completions')

// dreamgen / mancer 特例路径
eq(buildBody({ prompt: 'P', model: 'm', backend: 'dreamgen', server: '' }).api_server, 'https://api.dreamgen.com/api/openai/v1/completions', 'dreamgen 路径')
eq(buildBody({ prompt: 'P', model: 'm', backend: 'mancer', server: '' }).api_server, 'https://neuro.mancer.tech/webui/api/oai/v1/completions', 'mancer 路径')

// 缺服务器报错
try { buildBody({ prompt: 'P', model: 'm', backend: 'koboldcpp', server: '' }); fail++; console.error('✗ 本地后端缺地址应抛错') } catch { /* ok */ }

// messagesToPrompt
const p = messagesToPrompt([
  { role: 'system', content: 'SYS' },
  { role: 'user', content: '你好' },
  { role: 'assistant', content: '你好呀' },
  { role: 'user', content: '再见' },
], '爱丽丝', '老板')
eq(p, 'SYS\n\n老板: 你好\n\n爱丽丝: 你好呀\n\n老板: 再见\n\n爱丽丝:', 'prompt 拼接格式')

function ok(c, n) { if (!c) { fail++; console.error(`✗ ${n}`) } }
console.log(fail ? `${fail} 失败` : '全部通过')
process.exit(fail ? 1 : 0)
