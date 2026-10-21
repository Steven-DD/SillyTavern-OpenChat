/**
/**
 * 预设服务（Presets Service）
 *
 * 覆盖：
 * 1. 文本生成与采样预设（Text Completion / OpenAI Settings）
 * 2. 预设字段双向转换（ST preset JSON ↔ GenSettings）
 * 3. 预设文件导入（.json 拖拽/上传）与导出（.json 下载）
 * 4. 内置经典预设库（Default, Storywriter, Creative, Precise, Roleplay, Verbose）
 */
import { stPostJson } from './client'
import type { GenSettings } from '@/stores/settings'

export type PresetType = 'text' | 'openai' | 'instruct' | 'context'

export interface BuiltinPresetItem {
  id: string
  name: string
  description: string
  type: 'text' | 'openai'
  settings: Partial<GenSettings>
}

/** 内置标准生成预设库（ST 经典取值对齐） */
export const BUILTIN_GEN_PRESETS: BuiltinPresetItem[] = [
  {
    id: 'default',
    name: 'Default (标准平衡)',
    description: '标准平衡采样，适用于大多数日常对话与通用模型',
    type: 'openai',
    settings: {
      temperature: 1.0,
      topP: 1.0,
      topK: 0,
      minP: 0,
      frequencyPenalty: 0,
      presencePenalty: 0,
      repetitionPenalty: 1.0,
    },
  },
  {
    id: 'storywriter',
    name: 'Storywriter (小说续写)',
    description: '丰富多样的描写与低复读率，适合长篇角色扮演与故事展开',
    type: 'text',
    settings: {
      temperature: 0.9,
      topP: 0.95,
      topK: 40,
      minP: 0.05,
      frequencyPenalty: 0.2,
      presencePenalty: 0.15,
      repetitionPenalty: 1.12,
    },
  },
  {
    id: 'creative',
    name: 'Creative (生动创新)',
    description: '较高随机性，鼓励意料之外的剧情走向与丰富词汇',
    type: 'openai',
    settings: {
      temperature: 1.15,
      topP: 0.98,
      topK: 80,
      minP: 0.02,
      frequencyPenalty: 0.3,
      presencePenalty: 0.25,
      repetitionPenalty: 1.08,
    },
  },
  {
    id: 'precise',
    name: 'Precise (严谨逻辑)',
    description: '较低随机性，注重事实、上下文严格遵循与逻辑一致',
    type: 'openai',
    settings: {
      temperature: 0.4,
      topP: 0.85,
      topK: 20,
      minP: 0.1,
      frequencyPenalty: 0,
      presencePenalty: 0,
      repetitionPenalty: 1.02,
    },
  },
  {
    id: 'roleplay',
    name: 'Roleplay (沉浸二次元)',
    description: '强化角色性格色彩，对白生动且不易出戏',
    type: 'text',
    settings: {
      temperature: 0.85,
      topP: 0.92,
      topK: 50,
      minP: 0.06,
      frequencyPenalty: 0.1,
      presencePenalty: 0.1,
      repetitionPenalty: 1.15,
    },
  },
  {
    id: 'verbose',
    name: 'Verbose (详实丰满)',
    description: '单次输出充实饱满，注重环境与心境微观描写',
    type: 'openai',
    settings: {
      temperature: 0.95,
      topP: 0.96,
      topK: 60,
      minP: 0.04,
      frequencyPenalty: 0.15,
      presencePenalty: 0.1,
      repetitionPenalty: 1.05,
    },
  },
]

/** 获取后端持久化的预设列表 */
export async function listPresets(type: PresetType): Promise<string[]> {
  try {
    const res = await stPostJson<{ presets?: string[]; ok?: boolean }>('/api/presets/list', { type })
    return Array.isArray(res?.presets) ? res.presets : []
  } catch (e) {
    console.warn(`[presets] 获取 ${type} 预设列表失败：`, e)
    return []
  }
}

/** 读取具体预设文件内容 */
export async function getPreset(name: string, type: PresetType): Promise<Record<string, unknown> | null> {
  try {
    return await stPostJson<Record<string, unknown>>('/api/presets/get', { name, type })
  } catch (e) {
    console.warn(`[presets] 读取预设 ${name} (${type}) 失败：`, e)
    return null
  }
}

/** 保存预设到后端文件 */
export async function savePreset(name: string, data: Record<string, unknown>, type: PresetType): Promise<void> {
  await stPostJson<{ ok?: boolean }>('/api/presets/save', { name, preset: data, type })
}

/** 删除预设文件 */
export async function deletePreset(name: string, type: PresetType): Promise<void> {
  await stPostJson<{ ok?: boolean }>('/api/presets/delete', { name, type })
}

/**
 * 将 ST 原始预设 JSON 转换为 App 的 GenSettings 字段
 */
export function presetToGenSettings(raw: Record<string, unknown>, current?: Partial<GenSettings>): Partial<GenSettings> {
  const result: Partial<GenSettings> = { ...(current ?? {}) }

  // 1. 温度 (temperature / temp)
  const temp = raw.temperature ?? raw.temp
  if (typeof temp === 'number' && !Number.isNaN(temp)) result.temperature = temp

  // 2. 核采样 (top_p)
  const topP = raw.top_p ?? raw.topP
  if (typeof topP === 'number' && !Number.isNaN(topP)) result.topP = topP

  // 3. Top K (top_k)
  const topK = raw.top_k ?? raw.topK
  if (typeof topK === 'number' && !Number.isNaN(topK)) result.topK = topK

  // 4. Min P (min_p)
  const minP = raw.min_p ?? raw.minP
  if (typeof minP === 'number' && !Number.isNaN(minP)) result.minP = minP

  // 5. Top A (top_a)
  const topA = raw.top_a ?? raw.topA
  if (typeof topA === 'number' && !Number.isNaN(topA)) result.topA = topA

  // 6. 频率惩罚 (frequency_penalty)
  const freq = raw.frequency_penalty ?? raw.freq_pen
  if (typeof freq === 'number' && !Number.isNaN(freq)) result.frequencyPenalty = freq

  // 7. 存在惩罚 (presence_penalty)
  const pres = raw.presence_penalty ?? raw.presence_pen
  if (typeof pres === 'number' && !Number.isNaN(pres)) result.presencePenalty = pres

  // 8. 重复惩罚 (repetition_penalty / rep_pen)
  const rep = raw.repetition_penalty ?? raw.rep_pen
  if (typeof rep === 'number' && !Number.isNaN(rep)) result.repetitionPenalty = rep

  // 9. 单次回复 token 上限 (max_tokens / n_predict / amount_gen)
  const maxTok = raw.max_tokens ?? raw.n_predict ?? raw.amount_gen
  if (typeof maxTok === 'number' && maxTok > 0) result.maxTokens = maxTok

  // 10. 上下文窗口 (max_context / context_length)
  const maxCtx = raw.max_context ?? raw.context_length
  if (typeof maxCtx === 'number' && maxCtx > 0) result.maxContext = maxCtx

  // 11. 模型指定 (model / openai_model)
  const model = raw.model ?? raw.openai_model
  if (typeof model === 'string' && model.trim()) result.model = model.trim()

  return result
}

/**
 * 将 App 的 GenSettings 导出为 ST 规范的预设 JSON
 */
export function genSettingsToPreset(settings: GenSettings, type: 'text' | 'openai'): Record<string, unknown> {
  if (type === 'text') {
    return {
      temp: settings.temperature,
      temperature: settings.temperature,
      top_p: settings.topP,
      top_k: settings.topK,
      top_a: settings.topA,
      min_p: settings.minP,
      typical_p: 1.0,
      tfs: 1.0,
      rep_pen: settings.repetitionPenalty,
      repetition_penalty: settings.repetitionPenalty,
      frequency_penalty: settings.frequencyPenalty,
      presence_penalty: settings.presencePenalty,
      n_predict: settings.maxTokens,
      max_tokens: settings.maxTokens,
      max_context: settings.maxContext,
      model: settings.model || undefined,
    }
  }

  return {
    temperature: settings.temperature,
    top_p: settings.topP,
    frequency_penalty: settings.frequencyPenalty,
    presence_penalty: settings.presencePenalty,
    max_tokens: settings.maxTokens,
    max_context: settings.maxContext,
    model: settings.model || undefined,
    openai_model: settings.model || undefined,
    stream: true,
  }
}

/**
 * 解析用户上传/拖拽的 ST .json 预设文件
 */
export async function importPresetFile(file: File, type: PresetType): Promise<{ name: string; data: Record<string, unknown> }> {
  const text = await file.text()
  let parsed: unknown
  try {
    parsed = JSON.parse(text)
  } catch {
    throw new Error('预设文件不是合法的 JSON 格式')
  }

  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('预设文件内容必须是 JSON 对象')
  }

  const stem = file.name.replace(/\.json$/i, '').trim() || 'Imported_Preset'
  const data = parsed as Record<string, unknown>

  // 持久化到后端
  await savePreset(stem, data, type)

  return { name: stem, data }
}

/**
 * 将预设导出为标准 .json 文件供浏览器下载
 */
export function exportPresetFile(name: string, data: Record<string, unknown>): void {
  const str = JSON.stringify(data, null, 4)
  const blob = new Blob([str], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `${name.trim() || 'preset'}.json`
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(url)
}
