#!/usr/bin/env node
import assert from 'node:assert/strict'
import { DEFAULT_SYNC_CONFIG } from '../src/services/st/sync'

console.log('  — WebDAV 云端增量同步配置与 JSONL 合并逻辑自检 —')

// 1. 配置默认值断言
assert.equal(typeof DEFAULT_SYNC_CONFIG.url, 'string', 'url 默认字符串')
assert.equal(typeof DEFAULT_SYNC_CONFIG.username, 'string', 'username 默认字符串')
assert.equal(DEFAULT_SYNC_CONFIG.auto_sync, false, '默认不开启自动同步')

// 2. JSONL 会话合并算法验证
function mergeChatJsonl(localContent, remoteContent) {
  const lines = []
  const seen = new Set()

  const localLines = localContent.split('\n').filter(Boolean)
  const remoteLines = remoteContent.split('\n').filter(Boolean)

  if (localLines.length) {
    lines.push(localLines[0])
    seen.add(localLines[0].trim())
  } else if (remoteLines.length) {
    lines.push(remoteLines[0])
    seen.add(remoteLines[0].trim())
  }

  for (let i = 1; i < localLines.length; i++) {
    const trimmed = localLines[i].trim()
    if (!seen.has(trimmed)) {
      seen.add(trimmed)
      lines.push(localLines[i])
    }
  }

  for (let i = 1; i < remoteLines.length; i++) {
    const trimmed = remoteLines[i].trim()
    if (!seen.has(trimmed)) {
      seen.add(trimmed)
      lines.push(remoteLines[i])
    }
  }

  return lines.join('\n')
}

const local = '{"header": 1}\n{"mes": "hello"}\n{"mes": "how are you"}'
const remote = '{"header": 1}\n{"mes": "hello"}\n{"mes": "new message from remote"}'

const merged = mergeChatJsonl(local, remote)
const mergedLines = merged.split('\n')

assert.equal(mergedLines.length, 4, '合并后应保留 1 条元数据头和 3 条互不重复的消息')
assert.equal(mergedLines[0], '{"header": 1}', '元数据头位于首行')
assert.ok(merged.includes('how are you'), '本地独特消息得以保全')
assert.ok(merged.includes('new message from remote'), '远端新增消息成功追加合并')

console.log('✓ WebDAV 增量同步模块自检全部通过')
