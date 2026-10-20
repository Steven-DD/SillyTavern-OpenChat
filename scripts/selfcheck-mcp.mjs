#!/usr/bin/env node
/**
 * selfcheck-mcp.mjs —— 自动化自检：MCP 工具 Schema 转换与工具调用解析器
 */
import assert from 'node:assert/strict'
import {
  DEFAULT_MCP_SETTINGS,
  resolveToolCalls,
} from '../src/services/st/mcp'

console.log('=== selfcheck-mcp: 开始 MCP 工具链自检 ===')

// 1. 默认设置完整性校验
assert.equal(typeof DEFAULT_MCP_SETTINGS.enabled, 'boolean', 'enabled 需为布尔')
assert.equal(typeof DEFAULT_MCP_SETTINGS.webSearchEnabled, 'boolean', '联网搜索开关需为布尔')
assert.equal(typeof DEFAULT_MCP_SETTINGS.calculatorEnabled, 'boolean', '计算器开关需为布尔')
assert.ok(DEFAULT_MCP_SETTINGS.maxRecursionDepth >= 1, '最大递归调用深度需大于等于 1')
console.log('✓ 1. 默认 MCP 设置完整性校验通过')

// 2. resolveToolCalls 参数解析与回传结构断言 (模拟无后端打桩)
const mockCalls = [
  {
    id: 'call_123',
    function: {
      name: 'calculator',
      arguments: JSON.stringify({ expression: '42 * 2' }),
    },
  },
  {
    id: 'call_456',
    function: {
      name: 'current_time',
      arguments: '{}',
    },
  },
]

// 模拟离线状态下执行工具调用转换
const results = await resolveToolCalls(mockCalls)
assert.equal(results.length, 2, '需返回 2 个工具响应结果')
assert.equal(results[0].role, 'tool', '角色必须为 tool')
assert.equal(results[0].tool_call_id, 'call_123', 'tool_call_id 需严格对齐')
assert.equal(results[1].tool_call_id, 'call_456', 'tool_call_id 需严格对齐')
console.log('✓ 2. resolveToolCalls 多工具请求解析与回传格式转换断言通过')

// 3. 空参数与容错断言
const emptyCalls = await resolveToolCalls([])
assert.equal(emptyCalls.length, 0, '空列表应返回空数组')

const malformedCalls = await resolveToolCalls([
  {
    id: 'call_bad',
    function: {
      name: 'test',
      arguments: 'not valid json',
    },
  },
])
assert.equal(malformedCalls.length, 1)
assert.equal(malformedCalls[0].tool_call_id, 'call_bad')
console.log('✓ 3. 工具调用边界与畸形 JSON 容错保护断言通过')

console.log('=== selfcheck-mcp: 全部通过 ===')
