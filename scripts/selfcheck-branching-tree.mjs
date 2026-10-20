#!/usr/bin/env node
import assert from 'node:assert/strict'

console.log('  — 树状剧情分支图 (Branching Tree) 数据模型与节点拓扑自检 —')

const mockMessages = [
  {
    role: 'user',
    content: '你好，请问你是谁？',
    name: 'User',
    sendDate: 1700000000000,
  },
  {
    role: 'assistant',
    content: '我是爱丽丝，很高兴认识你。',
    name: 'Alice',
    sendDate: 1700000001000,
    swipes: [
      '我是爱丽丝，很高兴认识你。',
      '你好旅行者，我是爱丽丝。',
      '（眨了眨眼）叫我爱丽丝就好。',
    ],
    swipeId: 0,
  },
  {
    role: 'user',
    content: '我们在哪里？',
    name: 'User',
    sendDate: 1700000002000,
  },
  {
    role: 'assistant',
    content: '这里是静谧森林的深处。',
    name: 'Alice',
    sendDate: 1700000003000,
    bookmarkLink: 'Alice - branch checkpoint',
  },
]

function buildTreeNodes(messages, userName = 'User', charFallback = 'AI') {
  return messages
    .filter((m) => !m.pending && !m.error)
    .map((m, idx) => {
      const swipes = m.swipes && m.swipes.length > 0 ? m.swipes : [m.content]
      const activeSwipeId = m.swipeId ?? 0
      const content = m.content || ''
      const snippet = content.replace(/\s+/g, ' ').slice(0, 70) + (content.length > 70 ? '…' : '')
      return {
        index: idx,
        role: m.role,
        name: m.name || (m.role === 'user' ? userName : charFallback),
        content,
        snippet,
        swipesCount: swipes.length,
        activeSwipeId,
        swipes,
        bookmarkLink: m.bookmarkLink,
      }
    })
}

const nodes = buildTreeNodes(mockMessages)

// 1. 节点总数与顺序断言
assert.equal(nodes.length, 4, '应生成 4 个消息节点')
assert.equal(nodes[0].role, 'user')
assert.equal(nodes[1].role, 'assistant')

// 2. Swipes 候选分支断言
const n1 = nodes[1]
assert.equal(n1.swipesCount, 3, '节点 #2 拥有 3 个备选回复')
assert.equal(n1.activeSwipeId, 0, '当前激活分支 0')
assert.equal(n1.content, '我是爱丽丝，很高兴认识你。')

// 模拟切换至分支 2
n1.activeSwipeId = 2
n1.content = n1.swipes[2]
assert.equal(n1.content, '（眨了眨眼）叫我爱丽丝就好。', '分支内容切换成功')

// 3. 书签/分叉链接断言
const n3 = nodes[3]
assert.equal(n3.bookmarkLink, 'Alice - branch checkpoint', '包含书签归档链接')

console.log('✓ 树状会话分支图 (Branching Tree) 数据自检全部通过')
