/**
 * 端到端验证：完全复刻 App「导入角色卡」的调用序列
 * （services/st/data.ts + stores/character.ts importCard）
 *
 * 1. /csrf-token 拿令牌
 * 2. /api/characters/import（multipart avatar + file_type=json）
 * 3. /api/characters/get 读回 → data.character_book 应有 17 条
 * 4. convertCharacterBook → /api/worldinfo/edit {name, data}
 * 5. /api/characters/edit 全字段 + world（绑定）
 * 6. 复核：/api/worldinfo/get 条目数 / characters/get 的 extensions.world / worldinfo/list
 */
const BASE = 'http://127.0.0.1:8000'
const CARD_PATH = 'C:/Users/Administrator/Downloads/林晚棠_全剧本修罗场终极版.json'

let cookie = ''
let csrf = ''

async function api(path, body) {
  const res = await fetch(BASE + path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, Cookie: cookie },
    body: JSON.stringify(body ?? {}),
  })
  cookie = mergeCookies(cookie, res)
  const text = await res.text()
  try {
    return { status: res.status, json: JSON.parse(text) }
  } catch {
    return { status: res.status, text }
  }
}

async function apiForm(path, form) {
  const res = await fetch(BASE + path, {
    method: 'POST',
    headers: { 'X-CSRF-Token': csrf, Cookie: cookie },
    body: form,
  })
  cookie = mergeCookies(cookie, res)
  const text = await res.text()
  try {
    return { status: res.status, json: JSON.parse(text) }
  } catch {
    return { status: res.status, text }
  }
}

/** 合并 set-cookie（session 值 + 签名两个都要带，浏览器自动做的事这里手动做） */
function mergeCookies(prev, res) {
  const set = typeof res.headers.getSetCookie === 'function' ? res.headers.getSetCookie() : [res.headers.get('set-cookie')].filter(Boolean)
  const map = new Map()
  for (const c of prev ? prev.split('; ') : []) {
    const i = c.indexOf('=')
    if (i > 0) map.set(c.slice(0, i), c.slice(i + 1))
  }
  for (const c of set) {
    const pair = c.split(';')[0]
    const i = pair.indexOf('=')
    if (i > 0) map.set(pair.slice(0, i), pair.slice(i + 1))
  }
  return [...map.entries()].map(([k, v]) => `${k}=${v}`).join('; ')
}

// ---- convertCharacterBook（与 data.ts 同款）----
function convertCharacterBook(book) {
  const result = { entries: {}, originalData: book }
  const list = Array.isArray(book.entries) ? book.entries : []
  list.forEach((entry, index) => {
    const id = entry.id ?? index
    const e = entry.extensions ?? {}
    result.entries[String(id)] = {
      uid: id,
      key: entry.keys ?? [],
      keysecondary: entry.secondary_keys ?? [],
      comment: entry.comment ?? '',
      content: entry.content ?? '',
      constant: entry.constant ?? false,
      selective: entry.selective ?? false,
      order: entry.insertion_order ?? 100,
      position: e.position ?? (entry.position === 'after_char' ? 1 : 0),
      excludeRecursion: e.exclude_recursion ?? false,
      preventRecursion: e.prevent_recursion ?? false,
      delayUntilRecursion: e.delay_until_recursion ?? false,
      disable: !entry.enabled,
      addMemo: !!entry.comment,
      displayIndex: e.display_index ?? index,
      probability: e.probability ?? 100,
      useProbability: e.useProbability ?? true,
      depth: e.depth ?? 4,
      selectiveLogic: e.selectiveLogic ?? 0,
      group: e.group ?? '',
      groupOverride: e.group_override ?? false,
      groupWeight: e.group_weight ?? 100,
      scanDepth: e.scan_depth ?? null,
      caseSensitive: e.case_sensitive ?? null,
      matchWholeWords: e.match_whole_words ?? null,
      useGroupScoring: e.use_group_scoring ?? null,
      automationId: e.automation_id ?? '',
      role: e.role ?? 0,
      vectorized: e.vectorized ?? false,
      sticky: e.sticky ?? null,
      cooldown: e.cooldown ?? null,
      delay: e.delay ?? null,
      matchPersonaDescription: false,
      matchCharacterDescription: false,
      matchCharacterPersonality: false,
      matchCharacterDepthPrompt: false,
      matchScenario: false,
      matchCreatorNotes: false,
      extensions: entry.extensions ?? {},
      triggers: e.triggers || [],
      ignoreBudget: e.ignore_budget ?? false,
    }
  })
  return result
}

async function main() {
  // 0. CSRF
  const cr = await fetch(BASE + '/csrf-token')
  csrf = (await cr.json()).token
  cookie = mergeCookies('', cr)
  console.log('0. CSRF 令牌 OK')

  // 清理上次运行的产物（幂等，可重复跑）
  await api('/api/characters/delete', { avatar_url: '林晚棠.png', delete_chats: false })
  await api('/api/worldinfo/delete', { name: '青江大学校园世界书' })
  console.log('0b. 旧产物已清理（若存在）')

  // 1. 导入角色卡
  const fs = await import('node:fs')
  const buf = fs.readFileSync(CARD_PATH)
  // 去掉可能的 UTF-8 BOM
  const clean = buf[0] === 0xef ? buf.subarray(3) : buf
  const fd = new FormData()
  fd.set('avatar', new Blob([clean], { type: 'application/json' }), '林晚棠_全剧本修罗场终极版.json')
  fd.set('file_type', 'json')
  const imp = await apiForm('/api/characters/import', fd)
  if (!imp.json?.file_name) throw new Error('导入失败: ' + JSON.stringify(imp).slice(0, 300))
  // ⚠ ST 返回的 file_name 可能不带 .png（App 侧 data.ts 已做同样归一化）
  const avatar = imp.json.file_name.endsWith('.png') ? imp.json.file_name : imp.json.file_name + '.png'
  console.log('1. 角色卡导入成功 →', avatar)

  // 2. 读回，检查内嵌世界书
  const get1 = await api('/api/characters/get', { avatar_url: avatar })
  const data = get1.json?.data ?? {}
  const book = data.character_book
  if (!book || !Array.isArray(book.entries)) throw new Error('内嵌世界书缺失')
  console.log(`2. 内嵌世界书「${book.name}」共 ${book.entries.length} 条`)

  // 3. 转换 + 写入世界书
  const bookName = book.name || `${get1.json.name}'s Lorebook`
  const converted = convertCharacterBook(book)
  const we = await api('/api/worldinfo/edit', { name: bookName, data: converted })
  if (we.status !== 200) throw new Error('worldinfo/edit 失败: ' + we.status)
  console.log('3. 世界书已写入:', bookName)

  // 4. 绑定到角色（全字段 + world）
  const c = get1.json
  const edit = await api('/api/characters/edit', {
    avatar_url: avatar,
    ch_name: c.name,
    description: c.description ?? '',
    personality: c.personality ?? '',
    scenario: c.scenario ?? '',
    first_mes: c.first_mes ?? '',
    mes_example: c.mes_example ?? '',
    creator_notes: String(c.creatorcomment ?? ''),
    tags: c.tags ?? [],
    talkativeness: c.talkativeness ?? 0.5,
    fav: String(c.fav ?? false),
    chat: c.chat,
    create_date: c.create_date,
    json_data: c.json_data,
    world: bookName,
  })
  if (edit.status !== 200) throw new Error('characters/edit 失败: ' + edit.status)
  console.log('4. 世界书已绑定到角色卡')

  // 5. 复核
  const w = await api('/api/worldinfo/get', { name: bookName })
  const entryCount = Object.keys(w.json?.entries ?? {}).length
  console.log('5a. 世界书读回条目数:', entryCount)
  if (entryCount !== book.entries.length) throw new Error(`条目数不符: ${entryCount} != ${book.entries.length}`)

  const get2 = await api('/api/characters/get', { avatar_url: avatar })
  const bound = get2.json?.data?.extensions?.world
  console.log('5b. 角色卡 extensions.world =', JSON.stringify(bound))
  if (bound !== bookName) throw new Error('绑定失败: ' + bound)

  const list = await api('/api/worldinfo/list')
  const found = (list.json ?? []).find((x) => x.name === bookName)
  console.log('5c. 世界书列表包含:', found ? found.name + ' (' + found.file_id + ')' : '未找到!')

  const all = await api('/api/characters/all')
  const card = (all.json ?? []).find((x) => x.name === '林晚棠')
  console.log('5d. 角色卡列表:', card ? card.name + ' · tags=' + (card.tags ?? []).length : '未找到!')

  // 抽查一条转换结果
  const e0 = w.json.entries['0']
  console.log('5e. 条目0抽查:', JSON.stringify({
    uid: e0.uid, comment: e0.comment, key: e0.key, constant: e0.constant,
    position: e0.position, order: e0.order, disable: e0.disable,
  }))
  console.log('\n✅ 端到端全部通过')
}

main().catch((e) => {
  console.error('❌', e.message)
  process.exit(1)
})
