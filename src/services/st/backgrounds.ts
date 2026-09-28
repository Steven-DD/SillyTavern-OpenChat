/**
 * 聊天背景图（ST /api/backgrounds 体系）
 * - 列表：POST /api/backgrounds/all → [{filename, isAnimated}]
 * - 图片：静态路径 backgrounds/<encoded>（走中继，同源 Cookie 无需签名）
 */
import { stBase, stPostJson } from './client'

export interface BackgroundItem {
  filename: string
  isAnimated?: boolean
}

export async function listBackgrounds(): Promise<BackgroundItem[]> {
  const data = await stPostJson<{ images?: BackgroundItem[] } | BackgroundItem[]>(
    '/api/backgrounds/all',
  )
  const arr = Array.isArray(data) ? data : (data?.images ?? [])
  return arr.filter((b) => b && typeof b.filename === 'string')
}

export function backgroundUrl(name: string): string {
  return `${stBase()}/backgrounds/${encodeURIComponent(name)}`
}
