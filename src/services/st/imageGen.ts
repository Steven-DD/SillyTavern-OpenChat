/**
/**
 * 文生图服务（P2 级能力 · 统一调度 NovelAI / Stable Diffusion / ComfyUI）
 */
import { stPostJson } from './client'
import { log } from '@/services/log'

export interface ImageGenConfig {
  provider: 'novelai' | 'sd' | 'comfyui'
  endpoint: string
  model: string
  negativePrompt: string
  width: number
  height: number
  steps: number
  cfgScale: number
  sampler: string
  autoDrawOnTag: boolean
}

const STORAGE_KEY = 'app.image_gen'

export const DEFAULT_IMAGE_GEN_CONFIG: ImageGenConfig = {
  provider: 'novelai',
  endpoint: 'http://127.0.0.1:7860',
  model: 'nai-diffusion-3',
  negativePrompt:
    'lowres, {bad}, error, fewer, extra, missing, worst quality, jpeg artifacts, bad anatomy, bad hands, text, blurry',
  width: 832,
  height: 1216,
  steps: 28,
  cfgScale: 5.0,
  sampler: 'k_euler',
  autoDrawOnTag: true,
}

export function getImageGenConfig(): ImageGenConfig {
  if (typeof localStorage === 'undefined') return { ...DEFAULT_IMAGE_GEN_CONFIG }
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return { ...DEFAULT_IMAGE_GEN_CONFIG }
    return { ...DEFAULT_IMAGE_GEN_CONFIG, ...JSON.parse(raw) }
  } catch {
    return { ...DEFAULT_IMAGE_GEN_CONFIG }
  }
}

export function setImageGenConfig(patch: Partial<ImageGenConfig>): void {
  if (typeof localStorage === 'undefined') return
  const current = getImageGenConfig()
  const updated = { ...current, ...patch }
  localStorage.setItem(STORAGE_KEY, JSON.stringify(updated))
}

export interface ImageGenOptions {
  prompt: string
  negativePrompt?: string
  provider?: 'novelai' | 'sd' | 'comfyui'
  width?: number
  height?: number
  steps?: number
  cfgScale?: number
  sampler?: string
  endpoint?: string
  token?: string
  model?: string
}

export interface ImageGenResult {
  ok: boolean
  url: string
  filename: string
  provider?: string
}

/** 发起文生图请求 */
export async function generateImage(opts: ImageGenOptions): Promise<ImageGenResult> {
  const cfg = getImageGenConfig()
  const payload = {
    provider: opts.provider ?? cfg.provider,
    prompt: opts.prompt,
    negative_prompt: opts.negativePrompt ?? cfg.negativePrompt,
    width: opts.width ?? cfg.width,
    height: opts.height ?? cfg.height,
    steps: opts.steps ?? cfg.steps,
    cfg_scale: opts.cfgScale ?? cfg.cfgScale,
    sampler: opts.sampler ?? cfg.sampler,
    endpoint: opts.endpoint ?? cfg.endpoint,
    model: opts.model ?? cfg.model,
    token: opts.token,
  }

  log.info('imageGen', `开始生图 [${payload.provider}]: ${payload.prompt.slice(0, 40)}...`)
  const res = await stPostJson<ImageGenResult>('/api/images/generate', payload)
  if (!res || !res.ok) {
    throw new Error('生图未返回有效结果')
  }
  log.info('imageGen', `生图成功: ${res.url}`)
  return res
}

/**
 * 解析并提取文本中的生图宏标签：
 * - `{draw::<prompt>}` 或 `{draw:<prompt>}`
 * - `{image::<prompt>}`
 */
export function extractDrawPrompts(text: string): { cleanText: string; prompts: string[] } {
  if (!text) return { cleanText: '', prompts: [] }
  const regex = /\{(?:draw|image)::(.*?)\}|\{(?:draw|image):(.*?)\}/gi
  const prompts: string[] = []
  const cleanText = text.replace(regex, (_, p1, p2) => {
    const p = (p1 || p2 || '').trim()
    if (p) prompts.push(p)
    return ''
  })
  return { cleanText, prompts }
}
