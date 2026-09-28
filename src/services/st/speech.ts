/**
 * TTS 朗读（ST /api/speech/synthesize，本地 transformers.js 引擎）
 * 首次使用 ST 会下载语音模型（较慢）；返回 wav 二进制。
 * 设置持久化：settings.json extension_settings.stchat_speech（App 自有键）
 */
import { stPost } from './client'
import { getSettings, saveSettingsFull } from './data'

export interface SpeechSettings {
  model: string
}

/** ST speech.js 默认本地模型 */
export const SPEECH_DEFAULTS: SpeechSettings = {
  model: 'Xenova/speecht5_tts',
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

let currentAudio: HTMLAudioElement | null = null
let currentUrl: string | null = null
let currentOnEnded: (() => void) | null = null

/** 释放当前音频：暂停 + revoke blob URL + 触发结束回调（P2：此前换曲时 URL 泄漏） */
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
}

export function stopSpeech(): void {
  releaseCurrent()
}

export function isSpeaking(): boolean {
  return !!currentAudio && !currentAudio.paused
}

/**
 * 合成并播放。`onEnded` 在自然播完、被打断（换曲/stopSpeech）或出错时都会触发 ——
 * 调用方用它复位"正在朗读"指示（此前 play() 一 resolve 就复位，朗读期间无指示）。
 */
export async function speakText(text: string, onEnded?: () => void): Promise<void> {
  const t = text.trim()
  if (!t) return
  const settings = await loadSpeechSettings()
  releaseCurrent()
  const res = await stPost('/api/speech/synthesize', { text: t, model: settings.model })
  if (!res.ok) throw new Error(`语音合成失败（HTTP ${res.status}）`)
  const blob = await res.blob()
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
