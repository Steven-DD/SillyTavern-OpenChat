/**
 * 消息翻译（ST /api/translate/*，8 家供应商，密钥全部在 ST 服务端）
 * 设置持久化：settings.json extension_settings.stchat_translate（App 自有键）
 */
import { stPostJson } from './client'
import { getSettings, saveSettingsFull } from './data'

export const TRANSLATE_PROVIDERS = [
  { id: 'google', label: 'Google（免密钥）' },
  { id: 'bing', label: 'Bing（免密钥）' },
  { id: 'lingva', label: 'Lingva（免密钥）' },
  { id: 'libre', label: 'LibreTranslate（需密钥）' },
  { id: 'deepl', label: 'DeepL（需密钥）' },
  { id: 'deeplx', label: 'DeepLX（需密钥）' },
  { id: 'yandex', label: 'Yandex（需密钥）' },
  { id: 'onering', label: 'OneRing（需密钥）' },
] as const

export interface TranslateSettings {
  provider: string
  /** 目标语言（ST translate 扩展惯例：中文 zh / 简中 zh-CN，各 API 语义略有差异） */
  target_language: string
}

export const TRANSLATE_DEFAULTS: TranslateSettings = {
  provider: 'google',
  target_language: 'zh',
}

let cache: TranslateSettings | null = null

export function getTranslateSettingsCached(): TranslateSettings {
  return cache ?? { ...TRANSLATE_DEFAULTS }
}

export async function loadTranslateSettings(force = false): Promise<TranslateSettings> {
  if (cache && !force) return cache
  const settings = await getSettings()
  const ext = settings.extension_settings as Record<string, unknown> | undefined
  const raw = ext?.stchat_translate as Partial<TranslateSettings> | undefined
  cache = { ...TRANSLATE_DEFAULTS, ...(raw && typeof raw === 'object' ? raw : {}) }
  return cache
}

export async function saveTranslateSettings(patch: Partial<TranslateSettings>): Promise<TranslateSettings> {
  const next = { ...getTranslateSettingsCached(), ...patch }
  const settings = await getSettings()
  const ext = (settings.extension_settings as Record<string, unknown> | undefined) ?? {}
  const before = JSON.stringify(ext.stchat_translate)
  if (JSON.stringify(next) === before) {
    cache = next
    return next
  }
  settings.extension_settings = { ...ext, stchat_translate: next }
  await saveSettingsFull(settings)
  cache = next
  return next
}

/** 翻译文本（onering 支持 from/to，其余由服务端自动检测源语言） */
export async function translateText(text: string, targetLang: string, provider: string): Promise<string> {
  const t = text.trim()
  if (!t) return ''
  if (provider === 'onering') {
    const r = await stPostJson<{ data?: { result?: string } }>('/api/translate/onering', {
      text: t,
      from_lang: 'auto',
      to_lang: targetLang,
    })
    return r.data?.result ?? ''
  }
  const r = await stPostJson<string | { translatedText?: string }>(`/api/translate/${provider}`, {
    text: t,
    lang: targetLang,
  })
  if (typeof r === 'string') return r
  return r.translatedText ?? ''
}
