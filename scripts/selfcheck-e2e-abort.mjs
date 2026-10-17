/**
 * T15 e2e（UI 侧语义面 · 无头）：生成链路 abort/重试/HTTP 错误体契约
 *
 * 覆盖（整改清单 T15 的 UI 侧断言；文件面 4 条断言已在 cargo 落地）：
 * - ④ abort 不产生错误态：客户端 abort 后错误**必须**携带 `signal.aborted`
 *   语义（调用方据此不再弹错误气泡），已回传内容保留在调用方累积器里，
 *   且**不再发起重试请求**（abort → 立即上抛，不进退避循环）
 * - 重试点：503/UNAVAILABLE 文案 → 可重试（退避一次后成功）；
 *   非 retryable（400 结构化错误体）→ 不重试，错误体 message 直达调用方
 * - 成功流：OpenAI chunk 逐帧解析 + [DONE] 收尾
 *
 * 真实 GUI 全量回归（Vue 组件/气泡渲染）归 Phase 6 冒烟清单 —— 本脚本证明
 * 服务层契约，是气泡不抛错的充分条件。
 *
 * 传输面 = 本进程内 mock 后端（node http：csrf + SSE 流控），gen 走
 * `custom` 源的 requestBody（与生产 buildBody 同构）。
 */
globalThis.localStorage = (() => {
  const m = new Map()
  return {
    getItem: (k) => (m.has(k) ? m.get(k) : null),
    setItem: (k, v) => m.set(k, String(v)),
    removeItem: (k) => m.delete(k),
  }
})()
globalThis.sessionStorage = (() => {
  const m = new Map()
  return { getItem: () => null, setItem: () => {}, removeItem: () => {} }
})()

const { setStBase, stBase } = await import('../src/services/st/client.ts')
const { generate, setConnConfig } = await import('../src/services/st/api.ts')
import http from 'node:http'

let fail = 0
const ok = (c, n) => { if (!c) { fail++; console.error(`✗ ${n}`) } }
const hit = (tag, extra = '') => console.error(`[SRV] ${tag} ${extra}`)

let scenario = 'success'
let genRequests = 0

const server = http.createServer((req, res) => {
  hit('HIT', `${req.method} ${req.url}`)
  if (req.url === '/csrf-token') {
    res.setHeader('content-type', 'application/json')
    res.end(JSON.stringify({ token: 't' }))
    return
  }
  if (req.url === '/api/backends/chat-completions/generate') {
    genRequests++
    let body = ''
    req.on('data', (d) => (body += d))
    req.on('end', () => {
      if (scenario === 'retry503' && genRequests === 1) {
        res.writeHead(503, { 'content-type': 'text/plain' })
        res.end('Service Unavailable: high demand, please retry')
        return
      }
      if (scenario === 'nonretryable') {
        res.writeHead(400, { 'content-type': 'application/json' })
        res.end(JSON.stringify({ error: { message: 'invalid_request_direct' } }))
        return
      }
      res.writeHead(200, { 'content-type': 'text/event-stream' })
      const frames = [
        'data: {"choices":[{"delta":{"content":"A"}}]}\n\n',
        'data: {"choices":[{"delta":{"content":"B"}}]}\n\n',
        scenario === 'abort'
          ? 'data: {"choices":[{"delta":{"content":"C-abort-frame"}}]}\n\n'
          : 'data: {"choices":[{"delta":{"content":"C"}}]}\n\n',
      ]
      let i = 0
      const tick = setInterval(() => {
        if (i < frames.length) {
          res.write(frames[i++])
          if (scenario === 'abort' && i >= frames.length) {
            clearInterval(tick)
            // 挂起等 abort（3s 兜底）
            const hang = setTimeout(() => res.end(), 3000)
            hang.unref()
          }
        } else if (scenario !== 'abort') {
          res.write('data: [DONE]\n\n')
          res.end()
          clearInterval(tick)
        }
      }, 30)
      // 客户端断开看不见 'close'（Node ≥15 该事件在正常 end 后也触发）—— 挂到 socket 断连/响应早关
      res.on('close', () => { clearInterval(tick); if (!res.writableEnded) res.destroy() })
    })
    return
  }
  res.writeHead(404)
  res.end()
})

await new Promise((r) => server.listen(0, '127.0.0.1', r))
const port = server.address().port
setStBase(`http://127.0.0.1:${port}`)
setConnConfig('custom', { customUrl: `http://127.0.0.1:${port}` })
console.error(`[BASE] ${stBase()} port=${port}`)

/* ---------------- 场景 A：成功流 ---------------- */
scenario = 'success'
genRequests = 0
{
  let text = ''
  const usedModel = await generate(
    { source: 'custom', model: 'mock-model', messages: [{ role: 'user', content: 'hi' }], temperature: 0.7 },
    (d) => { text += d },
  )
  ok(usedModel === 'mock-model', `A: 返回实际模型（got ${usedModel}）`)
  ok(text.includes('A') && text.includes('C'), `A: 增量文本完整（got ${JSON.stringify(text)}）`)
  ok(genRequests === 1, `A: 单请求（got ${genRequests}）`)
}

/* ---------------- 场景 B：503 → 退避重试成功 ---------------- */
scenario = 'retry503'
genRequests = 0
{
  let text = ''
  await generate(
    { source: 'custom', model: 'mock-model', messages: [{ role: 'user', content: 'hi' }], stream: true },
    (d) => { text += d },
  )
  ok(genRequests === 2, `B: 503 后重试一次（requests=${genRequests}）`)
  ok(text.includes('C'), 'B: 重试成功后正文完整')
}

/* ---------------- 场景 C：非 retryable 400 → 不重试 ---------------- */
scenario = 'nonretryable'
genRequests = 0
{
  let threw = ''
  try {
    await generate(
      { source: 'custom', model: 'mock-model', messages: [{ role: 'user', content: 'hi' }], stream: true },
      () => {},
    )
  } catch (e) {
    threw = e instanceof Error ? e.message : String(e)
  }
  ok(genRequests === 1, `C: 400 不重试（requests=${genRequests}）`)
  ok(threw.includes('invalid_request_direct'), `C: 上游错误体 message 直达调用方（got ${JSON.stringify(threw)}）`)
}

/* ---------------- 场景 D：abort → 语义 ④ ---------------- */
scenario = 'abort'
genRequests = 0
{
  const emitted = []
  const ac = new AbortController()
  const run = generate(
    { source: 'custom', model: 'mock-model', messages: [{ role: 'user', content: 'hi' }], stream: true, signal: ac.signal },
    (d) => emitted.push(d),
  ).then(
    () => 'completed',
    (e) => `thrown:${e instanceof Error ? e.name : String(e)}`,
  )
  // 等 2 帧落地后 abort
  await new Promise((r) => {
    const watch = setInterval(() => {
      if (emitted.length >= 2) { clearInterval(watch); r() }
    }, 20)
  })
  ac.abort()
  const outcome = await run
  ok(outcome.startsWith('thrown:'), `D: abort 以异常上抛（got ${outcome}）——调用方按 signal.aborted 判定，不上错误气泡`)
  ok(ac.signal.aborted === true, 'D: aborted 标志对调用方可见')
  ok(emitted.length === 2, `D: 已生成内容保留不覆盖（emitted=${emitted.length}）`)
  await new Promise((r) => setTimeout(r, 300))
  ok(genRequests === 1, `D: abort 后零重试（requests=${genRequests}）`)
}

server.close()
console.log(fail ? `${fail} 失败` : 'T15 e2e 全部通过')
process.exit(fail ? 1 : 0)
