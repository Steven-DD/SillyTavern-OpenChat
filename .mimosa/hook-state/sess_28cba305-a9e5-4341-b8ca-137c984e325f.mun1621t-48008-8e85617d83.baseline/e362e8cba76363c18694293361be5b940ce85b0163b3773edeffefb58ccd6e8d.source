/**
 * 向量记忆
 * - 服务端：/api/vector/insert|query|list（嵌入由 ST 服务端 transformers 完成）
 * - 集合：每个会话一个 collectionId（stchat-<avatar>-<file>）
 * - 设置：extension_settings.vectors
 *   + App 开关 extension_settings.stchat_vectors.enabled
 */
import { stPostJson } from './client'
import { getSettings, saveSettingsFull } from './data'

export interface VectorSettings {
  enabled: boolean
  source: string
  top_k: number
  depth: number
  /** 注入模板（{{text}} 替换） */
  template: string
}

export const VECTOR_DEFAULTS: VectorSettings = {
  enabled: false,
  source: 'transformers',
  top_k: 4,
  depth: 4,
  template: 'Related information from earlier in the chat that may be useful:\n{{text}}',
}

let cache: VectorSettings | null = null

export function invalidateVectorsCache(): void {
  cache = null
}

export async function loadVectorSettings(force = false): Promise<VectorSettings> {
  if (cache && !force) return cache
  const settings = await getSettings()
  const ext = settings.extension_settings as Record<string, unknown> | undefined
  const raw = ext?.vectors as Partial<VectorSettings> | undefined
  cache = {
    enabled: raw?.enabled === true,
    source: typeof raw?.source === 'string' && raw.source ? raw.source : VECTOR_DEFAULTS.source,
    top_k: typeof raw?.top_k === 'number' && raw.top_k > 0 ? Math.floor(raw.top_k) : VECTOR_DEFAULTS.top_k,
    depth: typeof raw?.depth === 'number' ? Math.max(0, Math.floor(raw.depth)) : VECTOR_DEFAULTS.depth,
    template:
      typeof raw?.template === 'string' && raw.template.includes('{{text}}')
        ? raw.template
        : VECTOR_DEFAULTS.template,
  }
  return cache
}

export async function saveVectorSettings(patch: Partial<VectorSettings>): Promise<VectorSettings> {
  const next = { ...(cache ?? VECTOR_DEFAULTS), ...patch }
  const settings = await getSettings()
  const ext = (settings.extension_settings as Record<string, unknown> | undefined) ?? {}
  if (JSON.stringify(ext.vectors) === JSON.stringify(next)) {
    cache = next
    return next
  }
  settings.extension_settings = { ...ext, vectors: next }
  await saveSettingsFull(settings)
  cache = next
  return next
}

/** App 开关（extension_settings.stchat_vectors.enabled） */
export async function loadVectorsAppEnabled(): Promise<boolean> {
  const settings = await getSettings()
  const ext = settings.extension_settings as Record<string, unknown> | undefined
  const raw = ext?.stchat_vectors as { enabled?: boolean } | undefined
  return raw?.enabled === true
}

export async function saveVectorsAppEnabled(enabled: boolean): Promise<void> {
  const settings = await getSettings()
  const ext = (settings.extension_settings as Record<string, unknown> | undefined) ?? {}
  settings.extension_settings = { ...ext, stchat_vectors: { ...(ext.stchat_vectors as object | undefined), enabled } }
  await saveSettingsFull(settings)
}

/** 会话集合 ID（sanitize 成安全字符） */
export function vectorsCollectionId(avatar: string, fileId: string): string {
  const raw = `stchat-${avatar}-${fileId}`.toLowerCase()
  return raw.replace(/[^a-z0-9-]/g, '-').replace(/-+/g, '-').slice(0, 96)
}

/** 32 位数值 hash（ST hashString 风格） */
export function hashMessage(text: string): number {
  let hash = 0
  for (let i = 0; i < text.length; i++) {
    hash = (hash << 5) - hash + text.charCodeAt(i)
    hash |= 0
  }
  return Math.abs(hash)
}

export interface VectorItem {
  hash: number
  text: string
  index: number
}

export interface VectorQueryMeta {
  hash?: number
  index?: number
  [k: string]: unknown
}

/** 已索引 hash 列表（去重用） */
export async function listVectorHashes(collectionId: string, source: string): Promise<number[]> {
  const hashes = await stPostJson<unknown[]>('/api/vector/list', { collectionId, source })
  return Array.isArray(hashes) ? hashes.map((h) => Number(h)).filter(Number.isFinite) : []
}

/** 插入向量项（服务端负责嵌入） */
export async function insertVectorItems(
  collectionId: string,
  source: string,
  items: VectorItem[],
): Promise<void> {
  if (!items.length) return
  await stPostJson('/api/vector/insert', { collectionId, source, items })
}

/** 相似检索：返回按相关度排序的 metadata（含 index） */
export async function queryVectors(
  collectionId: string,
  searchText: string,
  topK: number,
  source: string,
): Promise<VectorQueryMeta[]> {
  const r = await stPostJson<{ metadata?: VectorQueryMeta[] } | VectorQueryMeta[]>(
    '/api/vector/query',
    { collectionId, searchText, topK, source, threshold: 0 },
  )
  if (Array.isArray(r)) return r
  return r?.metadata ?? []
}
