/**
 * TTS 朗读多供应商框架（P10 对齐网页端 TTS 扩展的核心子集）：
 * - speecht5：ST /api/speech/synthesize（服务端本地 transformers 引擎，原有路径）
 * - system：Web Speech API（纯前端，零配置）
 * - edge：native 模式不可用（原 ST 端 edge-tts 插件路径已移除；不依赖 ST 端插件，需自研 TTS）
 * - openai：ST 服务端 /api/openai/generate-voice（用服务端托管的 OpenAI 密钥）
 * - elevenlabs：ST 服务端 /api/speech/elevenlabs/synthesize 代理（密钥不出服务端）
 * 设置持久化：settings.json extension_settings.stoc_tts / stoc_speech（App 自有键）
 */
import { stPost } from './client'
import { getSettings } from './data'
import { mutateSettings } from './settings-writequeue'

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
  { id: 'edge', label: 'Edge（native 模式不可用，需自研 TTS）' },
  { id: 'openai', label: 'OpenAI（服务端，用已配密钥）' },
  { id: 'elevenlabs', label: 'ElevenLabs（服务端代理，用已配密钥）' },
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
  const raw = ext?.stoc_speech as Partial<SpeechSettings> | undefined
  cache = { ...SPEECH_DEFAULTS, ...(raw && typeof raw === 'object' ? raw : {}) }
  return cache
}

export async function saveSpeechSettings(patch: Partial<SpeechSettings>): Promise<SpeechSettings> {
  const next = { ...(cache ?? SPEECH_DEFAULTS), ...patch }
  await mutateSettings((settings) => {
    const ext = (settings.extension_settings as Record<string, unknown> | undefined) ?? {}
    if (JSON.stringify(ext.stoc_speech) === JSON.stringify(next)) {
      return { next: settings, skip: true }
    }
    return { next: { ...settings, extension_settings: { ...ext, stoc_speech: next } } }
  })
  cache = next
  return next
}

let ttsCache: TtsProviderSettings | null = null

export async function loadTtsSettings(force = false): Promise<TtsProviderSettings> {
  if (ttsCache && !force) return ttsCache
  const settings = await getSettings()
  const ext = settings.extension_settings as Record<string, unknown> | undefined
  const raw = ext?.stoc_tts as Partial<TtsProviderSettings> | undefined
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
  await mutateSettings((settings) => {
    const ext = (settings.extension_settings as Record<string, unknown> | undefined) ?? {}
    if (JSON.stringify(ext.stoc_tts) === JSON.stringify(next)) {
      return { next: settings, skip: true }
    }
    return { next: { ...settings, extension_settings: { ...ext, stoc_tts: next } } }
  })
  ttsCache = next
  return next
}

/**
 * Edge 语音列表：native 模式不可用（D4 降级）。
 * 不再请求 ST 端 edge-tts 插件端点，直接返回空列表；调用方据此显示 native 降级说明。
 */
export async function loadEdgeVoices(): Promise<string[]> {
  return []
}

/** 各供应商合成 → 音频 Blob（system 走 Web Speech API，不经此函数） */
async function synthesize(text: string, tts: TtsProviderSettings): Promise<Blob> {
  switch (tts.provider) {
    // D4 降级：native 模式不接 ST 端 edge-tts 插件端点（不依赖 ST 端插件，需自研 TTS 后端）
    case 'edge':
      throw new Error('Edge 语音在 native 模式不可用（需自研 TTS），不依赖 ST 端插件')
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
      // 密钥不出服务端：走 ST /api/speech/elevenlabs/synthesize（服务端 readSecret 后带 xi-api-key）。
      // 此前"前端读 /api/secrets/view 拉全量密钥再浏览器直连"路径已在 2026 评审中判定为安全隐患并移除
      // （该端点默认 403，见 ST secrets.js allowKeysExposure；新后端也不会提供读回全量密钥的端点）。
      // 契约对齐 ST speech.js:234-258：body = { voiceId, request: { text, model_id } }。
      const res = await stPost('/api/speech/elevenlabs/synthesize', {
        voiceId: tts.elevenlabs.voiceId || TTS_DEFAULTS.elevenlabs.voiceId,
        request: {
          text,
          model_id: tts.elevenlabs.modelId || TTS_DEFAULTS.elevenlabs.modelId,
        },
      })
      if (!res.ok) throw new Error(`ElevenLabs 合成失败（HTTP ${res.status}）`)
      return res.blob()
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
