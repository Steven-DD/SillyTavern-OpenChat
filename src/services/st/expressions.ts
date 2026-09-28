/**
 * 表情立绘（M-10 最小版；sprites.js + classify.js）
 * - 立绘：GET /api/sprites/get?name=<charName> → [{label, path:'/characters/...'}]
 * - 情感分类：POST /api/extra/classify {text} → [{label, score}]（需 ST extra 模块启用；
 *   不可用时静默失败，UI 降级为手动选择）
 */
import { stGet, stPostJson, stBase } from './client'

export interface Sprite {
  label: string
  path: string
}

/** 角色的立绘列表（无立绘 → 空数组；name 传角色名，与 ST sprites 目录命名一致） */
export async function listSprites(charName: string): Promise<Sprite[]> {
  const data = await stGet<Sprite[] | unknown>(
    `/api/sprites/get?name=${encodeURIComponent(charName)}`,
  )
  return Array.isArray(data) ? (data as Sprite[]) : []
}

/** 立绘图片 URL（中继基址 + path；path 形如 /characters/<dir>/<file>） */
export function spriteUrl(relPath: string): string {
  return `${stBase()}${relPath}`
}

/**
 * 情感分类（extra 模块的 classify 管线；不可用/超时 → null，调用方降级）
 * 返回 top1 的规范化 label（ST 的 emotion labels，如 neutral/joy/sadness/anger/...）
 */
export async function classifyEmotion(text: string): Promise<string | null> {
  if (!text.trim()) return null
  try {
    const r = await stPostJson<{ label?: string } | { classification?: { label?: string } }[] | unknown[]>(
      '/api/extra/classify',
      { text: text.slice(0, 512) },
    )
    // ST 返回数组（topk）或 {classification:{label}} 两种历史形状，都兼容
    if (Array.isArray(r)) {
      const top = r[0] as { label?: string } | undefined
      return top?.label ?? null
    }
    const c = (r as { classification?: { label?: string } })?.classification
    return c?.label ?? null
  } catch {
    return null
  }
}

/** label 规范化（对齐 ST includesIgnoreCaseAndAccents：忽略大小写与重音符号） */
export function normalizeSpriteLabel(label: string): string {
  return label
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .trim()
}

/**
 * 按 label 选立绘（对齐 ST chooseSpriteForExpression:1563-1573）：
 * 先忽略大小写/重音精确匹配；未命中时回退 fallback 表情（ST extension_settings.
 * expressions.fallback_expression，扩展未配置则无回退）。
 */
export function matchSprite(
  sprites: Sprite[],
  emotion: string,
  fallback?: string,
): Sprite | null {
  if (!sprites.length) return null
  const hit = sprites.find((sp) => normalizeSpriteLabel(sp.label) === normalizeSpriteLabel(emotion))
  if (hit) return hit
  if (fallback && normalizeSpriteLabel(fallback)) {
    const fb = sprites.find(
      (sp) => normalizeSpriteLabel(sp.label) === normalizeSpriteLabel(fallback),
    )
    if (fb) return fb
  }
  return null
}

/** 读 ST 表情扩展的 fallback 表情设置（extension_settings.expressions.fallback_expression） */
export async function loadSpriteFallback(): Promise<string> {
  try {
    const { getSettings } = await import('./data')
    const settings = await getSettings()
    const ext = (settings as Record<string, unknown>).extension_settings as
      | Record<string, unknown>
      | undefined
    const expr = ext?.expressions as Record<string, unknown> | undefined
    const fb = expr?.fallback_expression
    return typeof fb === 'string' ? fb : ''
  } catch {
    return ''
  }
}
