/**
 * Token 计数（服务端精确版，替代本地粗估）
 *
 * ST 后端 `/api/tokenizers/openai/count?model=<model>` 按模型选择 tokenizer 计数
 * （gemma 系走 SentencePiece，claude 走 web tokenizer，其余 tiktoken）。
 * 请求体是消息数组。
 *
 * 失败语义：返回 null 而非抛错 —— 调用方回落本地估算（estimateTokens）
 * 计数精度不足不应阻断生成链路。
 */
import { stPostJson } from './client'

/**
 * 计数缓存：同一段文本（+模型）只向服务端请求一次。
 * auto-continue 每轮都对同一条增长的回复计数，未命中新增内容时整段可直接命中。
 */
const countCache = new Map<string, number>()
const COUNT_CACHE_MAX = 512

/** 服务端精确计数；接口不可用/超时 → null */
export async function countTokensRemote(text: string, model?: string): Promise<number | null> {
  if (!text) return 0
  const key = `${model ?? ''}\u0000${text}`
  const hit = countCache.get(key)
  if (hit !== undefined) return hit
  try {
    const q = model ? `?model=${encodeURIComponent(model)}` : ''
    const r = await stPostJson<{ token_count?: number }>(
      `/api/tokenizers/openai/count${q}`,
      [{ role: 'system', content: text }],
    )
    if (typeof r.token_count !== 'number' || r.token_count < 0) return null
    if (countCache.size >= COUNT_CACHE_MAX) {
      // 简单 FIFO 淘汰，缓存只做加速不作持久
      countCache.delete(countCache.keys().next().value!)
    }
    countCache.set(key, r.token_count)
    return r.token_count
  } catch {
    return null
  }
}
