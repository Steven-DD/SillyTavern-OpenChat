/**
 * TTS 朗读多供应商框架（P10 对齐网页端 TTS 扩展的核心子集）：
 * - speecht5：ST /api/speech/synthesize（服务端本地 transformers 引擎，原有路径）
 * - system：Web Speech API（纯前端，零配置）
 * - edge：ST 服务端 /api/edge-tts/generate（无需密钥）
 * - openai：ST 服务端 /api/openai/generate-voice（用服务端托管的 OpenAI 密钥）
 * - elevenlabs：浏览器直连 api.elevenlabs.io（密钥经 /api/secrets 托管读取）
 * 设置持久化：settings.json extension_settings.stchat_tts / stchat_speech（App 自有键）
 */
import { stGet, stPost } from './client'
import { getSettings, saveSettingsFull } from './data'

export interface SpeechSettings {
  model: string
}

/** ST speech.js 默认本地模型 */
export const SPEECH_DEFAULTS: SpeechSettings = {
  model: 'Xenova/speecht5_tts',
}

export type TtsProvider = 'speecht5' | 'system' | 'edge' | 'openai' | 'elevenlabs'

export const TTS_PROVIDERS: { id: TtsProvider; label: string }[] = [
  { id: 'speecht5', label: '本地（SpeechT5，首次下载模型）' },
  { id: 'system', label: '系统语音（Web Speech API）' },
  { id: 'edge', label: 'Edge（服务端，无需密钥）' },
  { id: 'openai', label: 'OpenAI（服务端，用已配密钥）' },
  { id: 'elevenlabs', label: 'ElevenLabs（直连，密钥经服务端托管）' },
]

export interface TtsProviderSettings {
  provider: TtsProvider
  edge: { voice: string; rate: number }
  openai: { model: string; voice: string; speed: number }
  elevenlabs: { modelId: string; voiceId: string }
}

export const TTS_DEFAULTS: TtsProviderSettings = {
  provider: 'speecht5',
  edge: { voice: 'zh-CN-XiaoxiaoNeural', rate: 0 },
  openai: { model: 'tts-1', voice: 'alloy', speed: 1 },
  elevenlabs: { modelId: 'eleven_multilingual_v2', voiceId: '21m00Tcm4TlvDq8ikWAM' },
}

let cache: SpeechSettings | null = null

export async function loadSpeechSettings(force = false): Promise<SpeechSettings> {
  if (cache && !force) return cache
  const settings = await getSettings()
  const ext = settings.extension_settings as Record<string, unknown> | undefined
  const raw = ext?.stchat_speech as Partial<SpeechSettings> | undefined
  cache = { ...SPEECH_DEFAULTS, ...(raw && typeof raw === 'object' ? raw : {}) }
  return cache
}

export async function saveSpeechSettings(patch: Partial<SpeechSettings>): Promise<SpeechSettings> {
  const next = { ...(cache ?? SPEECH_DEFAULTS), ...patch }
  const settings = await getSettings()
  const ext = (settings.extension_settings as Record<string, unknown> | undefined) ?? {}
  if (JSON.stringify(ext.stchat_speech) === JSON.stringify(next)) {
    cache = next
    return next
  }
  settings.extension_settings = { ...ext, stchat_speech: next }
  await saveSettingsFull(settings)
  cache = next
  return next
}

let ttsCache: TtsProviderSettings | null = null

export async function loadTtsSettings(force = false): Promise<TtsProviderSettings> {
  if (ttsCache && !force) return ttsCache
  const settings = await getSettings()
  const ext = settings.extension_settings as Record<string, unknown> | undefined
  const raw = ext?.stchat_tts as Partial<TtsProviderSettings> | undefined
  ttsCache = {
    ...TTS_DEFAULTS,
    ...(raw && typeof raw === 'object' ? raw : {}),
    edge: { ...TTS_DEFAULTS.edge, ...(raw && typeof raw === 'object' ? raw.edge : {}) },
    openai: { ...TTS_DEFAULTS.openai, ...(raw && typeof raw === 'object' ? raw.openai : {}) },
    elevenlabs: { ...TTS_DEFAULTS.elevenlabs, ...(raw && typeof raw === 'object' ? raw.elevenlabs : {}) },
  }
  return ttsCache
}

export async function saveTtsSettings(patch: Partial<TtsProviderSettings>): Promise<TtsProviderSettings> {
  const next: TtsProviderSettings = {
    ...(ttsCache ?? TTS_DEFAULTS),
    ...patch,
    edge: { ...(ttsCache ?? TTS_DEFAULTS).edge, ...(patch.edge ?? {}) },
    openai: { ...(ttsCache ?? TTS_DEFAULTS).openai, ...(patch.openai ?? {}) },
    elevenlabs: { ...(ttsCache ?? TTS_DEFAULTS).elevenlabs, ...(patch.elevenlabs ?? {}) },
  }
  const settings = await getSettings()
  const ext = (settings.extension_settings as Record<string, unknown> | undefined) ?? {}
  if (JSON.stringify(ext.stchat_tts) === JSON.stringify(next)) {
    ttsCache = next
    return next
  }
  settings.extension_settings = { ...ext, stchat_tts: next }
  await saveSettingsFull(settings)
  ttsCache = next
  return next
}

/** Edge 语音列表（ST 服务端 /api/edge-tts/list；需要 ST 端启用 edge-tts 插件） */
export async function loadEdgeVoices(): Promise<string[]> {
  const data = await stGet<{ ShortName?: string; shortName?: string }[] | { voices?: unknown }>(
    '/api/edge-tts/list',
  )
  const list = Array.isArray(data) ? data : ((data as { voices?: unknown }).voices as typeof data) ?? []
  const names = (list as { ShortName?: string; shortName?: string }[])
    .map((v) => String(v?.ShortName ?? v?.shortName ?? '').trim())
    .filter(Boolean)
  return [...new Set(names)].sort((a, b) => a.localeCompare(b))
}

/** 各供应商合成 → 音频 Blob（system 走 Web Speech API，不经此函数） */
async function synthesize(text: string, tts: TtsProviderSettings): Promise<Blob> {
  switch (tts.provider) {
    case 'edge': {
      const res = await stPost('/api/edge-tts/generate', {
        text,
        voice: tts.edge.voice || TTS_DEFAULTS.edge.voice,
        rate: Number(tts.edge.rate) || 0,
      })
      if (!res.ok) throw new Error(`Edge TTS 合成失败（HTTP ${res.status}）`)
      return res.blob()
    }
    case 'openai': {
      const res = await stPost('/api/openai/generate-voice', {
        text,
        voice: tts.openai.voice || TTS_DEFAULTS.openai.voice,
        model: tts.openai.model || TTS_DEFAULTS.openai.model,
        speed: Number(tts.openai.speed) || 1,
      })
      if (!res.ok) throw new Error(`OpenAI TTS 合成失败（HTTP ${res.status}）`)
      return res.blob()
    }
    case 'elevenlabs': {
      const secrets = await stGet<Record<string, string>>('/api/secrets/view')
      const key = secrets.api_key_elevenlabs
      if (!key) throw new Error('ElevenLabs 未配置密钥：请到 ST 网页端「API 连接」填入')
      const r = await fetch(
        `https://api.elevenlabs.io/v1/text-to-speech/${encodeURIComponent(
          tts.elevenlabs.voiceId || TTS_DEFAULTS.elevenlabs.voiceId,
        )}`,
        {
          method: 'POST',
          headers: { 'xi-api-key': key, 'Content-Type': 'application/json', Accept: 'audio/mpeg' },
          body: JSON.stringify({ text, model_id: tts.elevenlabs.modelId || TTS_DEFAULTS.elevenlabs.modelId }),
        },
      )
      if (!r.ok) throw new Error(`ElevenLabs 合成失败（HTTP ${r.status}）`)
      return r.blob()
    }
    default: {
      const settings = await loadSpeechSettings()
      const res = await stPost('/api/speech/synthesize', { text, model: settings.model })
      if (!res.ok) throw new Error(`语音合成失败（HTTP ${res.status}）`)
      return res.blob()
    }
  }
}

let currentAudio: HTMLAudioElement | null = null
let currentUrl: string | null = null
let currentOnEnded: (() => void) | null = null

/** 释放当前音频：暂停 + revoke blob URL + 触发结束回调（此前换曲时 URL 泄漏） */
function releaseCurrent(): void {
  currentOnEnded?.()
  currentOnEnded = null
  if (currentAudio) {
    currentAudio.pause()
    currentAudio = null
  }
  if (currentUrl) {
    URL.revokeObjectURL(currentUrl)
    currentUrl = null
  }
  window.speechSynthesis?.cancel()
}

export function stopSpeech(): void {
  releaseCurrent()
}

export function isSpeaking(): boolean {
  return (!!currentAudio && !currentAudio.paused) || !!window.speechSynthesis?.speaking
}

/**
 * 合成并播放。`onEnded` 在自然播完、被打断（换曲/stopSpeech）或出错时都会触发 ——
 * 调用方用它复位"正在朗读"指示（此前 play() 一 resolve 就复位，朗读期间无指示）。
 */
export async function speakText(text: string, onEnded?: () => void): Promise<void> {
  const t = text.trim()
  if (!t) return
  const tts = await loadTtsSettings()
  releaseCurrent()

  // system 供应商：Web Speech API，无 blob/Audio 生命周期
  if (tts.provider === 'system') {
    const synth = window.speechSynthesis
    if (!synth) throw new Error('当前环境不支持系统语音合成')
    const done = () => {
      currentOnEnded = null
      onEnded?.()
    }
    currentOnEnded = onEnded ?? null
    const utter = new SpeechSynthesisUtterance(t)
    utter.addEventListener('end', done)
    utter.addEventListener('error', done)
    synth.speak(utter)
    return
  }

  const blob = await synthesize(t, tts)
  const url = URL.createObjectURL(blob)
  const audio = new Audio(url)
  currentAudio = audio
  currentUrl = url
  currentOnEnded = onEnded ?? null
  const done = () => {
    if (currentAudio === audio) {
      currentAudio = null
      currentUrl = null
      currentOnEnded = null
    }
    URL.revokeObjectURL(url)
    onEnded?.()
  }
  audio.addEventListener('ended', done)
  audio.addEventListener('error', done)
  await audio.play()
}
