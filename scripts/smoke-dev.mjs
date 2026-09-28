#!/usr/bin/env node
/**
 * 开发环境全链路冒烟（经 Vite 代理，验证 dev server + 代理 + ST）
 *
 * 为什么要用 node 而不是 curl：
 * 本环境的 bash 沙箱**禁止向磁盘写文件**（curl 的 -c/-o 会静默失败 → cookie 丢失 → 伪 403）。
 * node 在内存里维护 cookie，不落盘，结论才可信。
 *
 * 用法：node app/scripts/smoke-dev.mjs [baseUrl]
 *   默认 baseUrl = http://127.0.0.1:1420
 */
const BASE = process.argv[2] ?? 'http://127.0.0.1:1420'

let cookie = ''
let csrf = ''
let fails = 0

function ok(name, pass, detail = '') {
  if (!pass) fails++
  console.log(`  ${pass ? '✅' : '❌'} ${name}${detail ? `  — ${detail}` : ''}`)
}

async function post(path, body = {}) {
  const res = await fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-CSRF-Token': csrf,
      ...(cookie ? { Cookie: cookie } : {}),
    },
    body: JSON.stringify(body),
  })
  const text = await res.text()
  let json = null
  try {
    json = JSON.parse(text)
  } catch {
    /* 非 JSON */
  }
  return { status: res.status, json, text }
}

console.log(`\n冒烟目标：${BASE}\n`)

// 1) 页面
{
  const res = await fetch(`${BASE}/`)
  const html = await res.text()
  ok('页面 / 可访问', res.status === 200 && /<div id="app"/.test(html), `HTTP ${res.status}, ${html.length}B html`)
}

// 2) CSRF（Cookie 在内存里持有）
{
  const res = await fetch(`${BASE}/csrf-token`)
  const setCookies = res.headers.getSetCookie?.() ?? []
  cookie = setCookies.map((c) => c.split(';')[0]).join('; ')
  const data = await res.json()
  csrf = data.token
  ok('CSRF 取 token + Cookie', res.status === 200 && !!csrf && setCookies.length > 0,
    `token=${csrf?.slice(0, 16)}… cookies=${setCookies.length}`)
}

// 3) 只读 API（POST + 双提交）
{
  const s = await post('/api/settings/get')
  ok('POST /api/settings/get', s.status === 200 && !!s.json, `HTTP ${s.status}`)

  const c = await post('/api/characters/all')
  const n = Array.isArray(c.json) ? c.json.length : 0
  ok('POST /api/characters/all', c.status === 200 && n > 0, `HTTP ${c.status}, 角色 ${n} 个`)

  const r = await post('/api/chats/recent', { max: 5 })
  ok('POST /api/chats/recent', r.status === 200 && Array.isArray(r.json),
    `HTTP ${r.status}, 会话 ${Array.isArray(r.json) ? r.json.length : '?'} 个`)
}

// 4) 静态资源经代理（本次新增的 /thumbnail、/characters 代理规则）
{
  // 注意：ST 的缩略图**实际是 JPEG 字节**（ff d8 ff e0 … JFIF）却声明 content-type: image/png
  // （ST thumbnails.format 默认 jpg）。浏览器按内容嗅探渲染，不影响 <img>；此处只校验是合法图片。
  const t = await fetch(`${BASE}/thumbnail?type=avatar&file=default_Seraphina.png`)
  const tb = new Uint8Array(await t.arrayBuffer())
  const isPng = tb[0] === 0x89 && tb[1] === 0x50
  const isJpeg = tb[0] === 0xff && tb[1] === 0xd8
  ok(
    '代理 /thumbnail 缩略图',
    t.status === 200 && (isPng || isJpeg) && tb.length > 1000,
    `HTTP ${t.status} ${t.headers.get('content-type')} ${tb.length}B 实际格式=${isPng ? 'PNG' : isJpeg ? 'JPEG(ST 声明为 png)' : '未知'}`,
  )

  const o = await fetch(`${BASE}/characters/default_Seraphina.png`)
  const ob = new Uint8Array(await o.arrayBuffer())
  ok('代理 /characters 原图', o.status === 200 && ob.length > 10000, `HTTP ${o.status} ${ob.length}B`)
}

console.log(`\n${fails === 0 ? '🎉 冒烟全通' : `⚠️ ${fails} 项失败`}\n`)
process.exit(fails === 0 ? 0 : 1)
