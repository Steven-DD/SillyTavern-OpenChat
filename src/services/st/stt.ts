/**
 * STT 语音输入多驱动驱动层 (Speech-to-Text)
 *
 * 双通道架构：
 * 1. Web Speech API（基于 WebView2 / Chromium 原生语音识别，零依赖、零 API 成本）
 * 2. Whisper API（OpenAI / Groq 兼容 /v1/audio/transcriptions 接口，适合复杂专有名词）
 */

export type SttProvider = 'webspeech' | 'whisper'

export interface SttSettings {
  provider: SttProvider
  lang: string
  whisperEndpoint?: string
  whisperModel?: string
}

export const DEFAULT_STT_SETTINGS: SttSettings = {
  provider: 'webspeech',
  lang: 'zh-CN',
  whisperEndpoint: 'https://api.openai.com/v1/audio/transcriptions',
  whisperModel: 'whisper-1',
}

const STT_STORAGE_KEY = 'app.stt_settings'

export function loadSttSettings(): SttSettings {
  try {
    const raw = localStorage.getItem(STT_STORAGE_KEY)
    if (!raw) return { ...DEFAULT_STT_SETTINGS }
    return { ...DEFAULT_STT_SETTINGS, ...JSON.parse(raw) }
  } catch {
    return { ...DEFAULT_STT_SETTINGS }
  }
}

export function saveSttSettings(cfg: SttSettings): void {
  localStorage.setItem(STT_STORAGE_KEY, JSON.stringify(cfg))
}

export interface SttCallbacks {
  onStart?: () => void
  onInterim?: (text: string) => void
  onFinal: (text: string) => void
  onError?: (err: string) => void
  onEnd?: () => void
}

let activeRecognition: any = null
let activeMediaRecorder: MediaRecorder | null = null
let audioChunks: Blob[] = []

/** 检测环境是否支持 Web Speech API */
export function isWebSpeechSupported(): boolean {
  return typeof window !== 'undefined' && ('SpeechRecognition' in window || 'webkitSpeechRecognition' in window)
}

/** 开启录音识别 */
export async function startSpeechRecognition(callbacks: SttCallbacks, customSettings?: Partial<SttSettings>): Promise<void> {
  const settings = { ...loadSttSettings(), ...(customSettings ?? {}) }

  // 通道 A：Web Speech API
  if (settings.provider === 'webspeech') {
    if (!isWebSpeechSupported()) {
      callbacks.onError?.('当前环境不支持 Web Speech API，请在设置中配置 Whisper API')
      return
    }

    const SpeechRec = (window as any).SpeechRecognition || (window as any).webkitSpeechRecognition
    const recognition = new SpeechRec()
    recognition.continuous = false
    recognition.interimResults = true
    recognition.lang = settings.lang || 'zh-CN'

    let finalTranscript = ''

    recognition.onstart = () => {
      activeRecognition = recognition
      callbacks.onStart?.()
    }

    recognition.onresult = (event: any) => {
      let interimTranscript = ''
      for (let i = event.resultIndex; i < event.results.length; ++i) {
        if (event.results[i].isFinal) {
          finalTranscript += event.results[i][0].transcript
        } else {
          interimTranscript += event.results[i][0].transcript
        }
      }
      if (interimTranscript) {
        callbacks.onInterim?.(interimTranscript)
      }
    }

    recognition.onerror = (event: any) => {
      callbacks.onError?.(event.error || '语音识别异常')
    }

    recognition.onend = () => {
      activeRecognition = null
      callbacks.onFinal(finalTranscript.trim())
      callbacks.onEnd?.()
    }

    try {
      recognition.start()
    } catch (e) {
      callbacks.onError?.(e instanceof Error ? e.message : String(e))
    }
    return
  }

  // 通道 B：Whisper API（录制音频 Blob）
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
    const mediaRecorder = new MediaRecorder(stream)
    activeMediaRecorder = mediaRecorder
    audioChunks = []

    mediaRecorder.ondataavailable = (e) => {
      if (e.data.size > 0) audioChunks.push(e.data)
    }

    mediaRecorder.onstart = () => {
      callbacks.onStart?.()
    }

    mediaRecorder.onstop = async () => {
      stream.getTracks().forEach((track) => track.stop())
      activeMediaRecorder = null
      callbacks.onEnd?.()

      const audioBlob = new Blob(audioChunks, { type: 'audio/webm' })
      if (audioBlob.size === 0) return

      // 向 Whisper 接口发送请求
      try {
        const formData = new FormData()
        formData.append('file', audioBlob, 'audio.webm')
        formData.append('model', settings.whisperModel || 'whisper-1')
        if (settings.lang) {
          formData.append('language', settings.lang.split('-')[0] || 'zh')
        }

        const endpoint = settings.whisperEndpoint || 'https://api.openai.com/v1/audio/transcriptions'
        const resp = await fetch(endpoint, {
          method: 'POST',
          body: formData,
        })

        if (!resp.ok) {
          throw new Error(`Whisper API 返回 HTTP ${resp.status}`)
        }

        const data = await resp.json()
        if (data.text) {
          callbacks.onFinal(data.text.trim())
        }
      } catch (err) {
        callbacks.onError?.(err instanceof Error ? err.message : String(err))
      }
    }

    mediaRecorder.start()
  } catch (err) {
    callbacks.onError?.(err instanceof Error ? err.message : '无法访问麦克风设备')
  }
}

/** 停止当前录音识别 */
export function stopSpeechRecognition(): void {
  if (activeRecognition) {
    try {
      activeRecognition.stop()
    } catch {
      // ignore
    }
    activeRecognition = null
  }

  if (activeMediaRecorder && activeMediaRecorder.state !== 'inactive') {
    try {
      activeMediaRecorder.stop()
    } catch {
      // ignore
    }
    activeMediaRecorder = null
  }
}

/** 是否正在录音中 */
export function isSpeechRecording(): boolean {
  return activeRecognition !== null || (activeMediaRecorder !== null && activeMediaRecorder.state === 'recording')
}
