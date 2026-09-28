/**
 * Token 计数（服务端精确版，替代本地粗估）
 *
 * ST 后端 `/api/tokenizers/openai/count?model=<model>` 按模型选择 tokenizer 计数
 * （gemma 系走 SentencePiece、claude 走 web tokenizer、其余 tiktoken）。
 * 请求体是消息数组（与 ST 前端 countTokensOpenAIAsync 的调用一致）。
 *
 * 失败语义：返回 null 而非抛错 —— 调用方回落本地估算（estimateTokens），
 * 计数精度不足不应阻断生成链路。
 */
import { stPostJson } from './client'

/** 服务端精确计数；接口不可用/超时 → null */
export async function countTokensRemote(text: string, model?: string): Promise<number | null> {
  if (!text) return 0
  try {
    const q = model ? `?model=${encodeURIComponent(model)}` : ''
    const r = await stPostJson<{ token_count?: number }>(
      `/api/tokenizers/openai/count${q}`,
      [{ role: 'system', content: text }],
    )
    return typeof r.token_count === 'number' && r.token_count >= 0 ? r.token_count : null
  } catch {
    return null
  }
}
