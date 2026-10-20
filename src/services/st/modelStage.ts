/**
 * Live2D / VRM 3D 角色模型视口与表情动作/口型联动管理器 (P2 级能力)
 */
import { ref } from 'vue'
import { classifyEmotion } from './expressions'
import { eventBus } from './eventBus'
import { log } from '@/services/log'

export type StageLayout = 'docked' | 'floating' | 'background' | 'hidden'
export type ModelType = 'live2d' | 'vrm' | 'sprite'

export interface ModelStageSettings {
  layout: StageLayout
  modelType: ModelType
  modelUrl: string
  scale: number
  posX: number
  posY: number
  lipSyncEnabled: boolean
  emotionSyncEnabled: boolean
  mouseFollowEnabled: boolean
}

const STORAGE_KEY = 'app.model_stage'

export const DEFAULT_STAGE_SETTINGS: ModelStageSettings = {
  layout: 'hidden',
  modelType: 'sprite',
  modelUrl: '',
  scale: 1.0,
  posX: 0,
  posY: 0,
  lipSyncEnabled: true,
  emotionSyncEnabled: true,
  mouseFollowEnabled: true,
}

export function loadStageSettings(): ModelStageSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return { ...DEFAULT_STAGE_SETTINGS }
    return { ...DEFAULT_STAGE_SETTINGS, ...JSON.parse(raw) }
  } catch {
    return { ...DEFAULT_STAGE_SETTINGS }
  }
}

export function saveStageSettings(patch: Partial<ModelStageSettings>): void {
  const current = loadStageSettings()
  const next = { ...current, ...patch }
  localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
}

/** 运行时响应式状态 */
export const stageLayout = ref<StageLayout>(loadStageSettings().layout)
export const stageModelType = ref<ModelType>(loadStageSettings().modelType)
export const stageModelUrl = ref<string>(loadStageSettings().modelUrl)
export const currentEmotion = ref<string>('neutral')
export const currentLipSync = ref<number>(0.0)

/** 情绪映射表：将文本情感标签映射到 Live2D/VRM 表情动作 */
export const EMOTION_MAP: Record<string, { live2d: string; vrm: string; params?: Record<string, number> }> = {
  joy: { live2d: 'happy', vrm: 'happy', params: { ParamEyeSmile: 1.0, ParamCheek: 0.4 } },
  happy: { live2d: 'happy', vrm: 'happy', params: { ParamEyeSmile: 1.0, ParamCheek: 0.4 } },
  sadness: { live2d: 'sad', vrm: 'sad', params: { ParamBrowAngle: 0.6, ParamEyeForm: -0.4 } },
  sad: { live2d: 'sad', vrm: 'sad', params: { ParamBrowAngle: 0.6, ParamEyeForm: -0.4 } },
  anger: { live2d: 'angry', vrm: 'angry', params: { ParamBrowAngle: -0.8, ParamEyeForm: -0.5 } },
  angry: { live2d: 'angry', vrm: 'angry', params: { ParamBrowAngle: -0.8, ParamEyeForm: -0.5 } },
  surprise: { live2d: 'surprised', vrm: 'surprised', params: { ParamEyeOpen: 1.4, ParamBrowY: 0.7 } },
  blush: { live2d: 'blush', vrm: 'happy', params: { ParamCheek: 1.0, ParamEyeSmile: 0.8 } },
  fear: { live2d: 'fear', vrm: 'sad', params: { ParamEyeOpen: 1.2, ParamBrowAngle: 0.4 } },
  neutral: { live2d: 'idle', vrm: 'neutral', params: { ParamBreath: 0.5 } },
}

/** 计算音频数据的 RMS 能量包络作为口型张合度 (0.0 ~ 1.0) */
export function calculateLipSyncLevel(timeData: Uint8Array): number {
  if (!timeData || timeData.length === 0) return 0
  let sum = 0
  for (let i = 0; i < timeData.length; i++) {
    const norm = (timeData[i]! - 128) / 128
    sum += norm * norm
  }
  const rms = Math.sqrt(sum / timeData.length)
  // 放大增益并限制在 0.0 ~ 1.0
  return Math.min(1.0, Math.max(0.0, rms * 3.8))
}

/** 触发情绪联动：分析最新回复文本并切换模型表情 */
export async function triggerEmotionFromText(text: string): Promise<string> {
  const cfg = loadStageSettings()
  if (!cfg.emotionSyncEnabled || !text.trim()) return 'neutral'

  try {
    const emotion = (await classifyEmotion(text)) || 'neutral'
    const cleanEmotion = emotion.toLowerCase().trim()
    currentEmotion.value = cleanEmotion
    log.info('modelStage', `模型表情联动更新: ${cleanEmotion}`)
    return cleanEmotion
  } catch (e) {
    log.warn('modelStage', `情绪识别联动跳过: ${e}`)
    currentEmotion.value = 'neutral'
    return 'neutral'
  }
}

// 监听会话消息回复生命周期，自动触发角色表情动作联动
eventBus.on('after_message_receive', (ev) => {
  if (ev?.text) {
    void triggerEmotionFromText(ev.text)
  }
})
