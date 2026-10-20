/**
 * MCP (Model Context Protocol) 协议客户端与工具箱服务 (P2 级能力)
 */
import { stPostJson } from './client'
import { log } from '@/services/log'

export interface McpToolDefinition {
  type: 'function'
  function: {
    name: string
    description: string
    parameters: {
      type: string
      properties?: Record<string, unknown>
      required?: string[]
    }
  }
}

export interface McpToolCallResult {
  name: string
  content: string
  is_error: boolean
}

export interface McpSettings {
  enabled: boolean
  webSearchEnabled: boolean
  calculatorEnabled: boolean
  currentTimeEnabled: boolean
  fetchWebpageEnabled: boolean
  maxRecursionDepth: number
}

const STORAGE_KEY = 'app.mcp_settings'

export const DEFAULT_MCP_SETTINGS: McpSettings = {
  enabled: true,
  webSearchEnabled: true,
  calculatorEnabled: true,
  currentTimeEnabled: true,
  fetchWebpageEnabled: true,
  maxRecursionDepth: 3,
}

export function loadMcpSettings(): McpSettings {
  if (typeof localStorage === 'undefined') return { ...DEFAULT_MCP_SETTINGS }
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return { ...DEFAULT_MCP_SETTINGS }
    return { ...DEFAULT_MCP_SETTINGS, ...JSON.parse(raw) }
  } catch {
    return { ...DEFAULT_MCP_SETTINGS }
  }
}

export function saveMcpSettings(patch: Partial<McpSettings>): void {
  if (typeof localStorage === 'undefined') return
  const current = loadMcpSettings()
  const next = { ...current, ...patch }
  localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
}

/** 获取当前后端暴露的所有可用工具定义列表 (OpenAI/Claude 格式) */
export async function listMcpTools(): Promise<McpToolDefinition[]> {
  const cfg = loadMcpSettings()
  if (!cfg.enabled) return []

  try {
    const res = await stPostJson<{ tools?: McpToolDefinition[] }>('/api/mcp/tools', {})
    const all = res?.tools ?? []
    // 按用户独立开关过滤
    return all.filter((t) => {
      const name = t.function?.name
      if (name === 'web_search') return cfg.webSearchEnabled
      if (name === 'calculator') return cfg.calculatorEnabled
      if (name === 'current_time') return cfg.currentTimeEnabled
      if (name === 'fetch_webpage') return cfg.fetchWebpageEnabled
      return true
    })
  } catch (e) {
    log.warn('mcp', `获取工具清单失败: ${e}`)
    return []
  }
}

/** 调度执行指定工具调用 */
export async function callMcpTool(
  name: string,
  args: Record<string, unknown>,
): Promise<McpToolCallResult> {
  log.info('mcp', `执行工具调用 [${name}]: ${JSON.stringify(args)}`)
  try {
    const res = await stPostJson<McpToolCallResult>('/api/mcp/call', {
      name,
      arguments: args,
    })
    if (!res) {
      throw new Error('未收到工具执行响应')
    }
    return res
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    log.warn('mcp', `工具调用失败 [${name}]: ${msg}`)
    return {
      name,
      content: `工具执行异常: ${msg}`,
      is_error: true,
    }
  }
}

/**
 * 批量执行模型下发的 tool_calls 并转换为回传给大模型的标准 tool 角色消息体
 */
export async function resolveToolCalls(
  toolCalls: Array<{
    id?: string
    function?: { name?: string; arguments?: string | Record<string, unknown> }
  }>,
): Promise<Array<{ role: 'tool'; tool_call_id: string; content: string }>> {
  const results: Array<{ role: 'tool'; tool_call_id: string; content: string }> = []

  for (const call of toolCalls) {
    const callId = call.id || `call_${Math.random().toString(36).slice(2, 9)}`
    const name = call.function?.name || 'unknown'
    let argsObj: Record<string, unknown> = {}

    const rawArgs = call.function?.arguments
    if (typeof rawArgs === 'string') {
      try {
        argsObj = JSON.parse(rawArgs)
      } catch {
        argsObj = { raw: rawArgs }
      }
    } else if (rawArgs && typeof rawArgs === 'object') {
      argsObj = rawArgs as Record<string, unknown>
    }

    const res = await callMcpTool(name, argsObj)
    results.push({
      role: 'tool',
      tool_call_id: callId,
      content: res.content,
    })
  }

  return results
}
