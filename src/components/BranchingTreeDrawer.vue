<script setup lang="ts">
import { computed } from 'vue'
import { useChatStore } from '@/stores/chat'

defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()

const chat = useChatStore()

export interface TreeNode {
  index: number
  role: 'user' | 'assistant' | 'system'
  name: string
  content: string
  snippet: string
  swipesCount: number
  activeSwipeId: number
  swipes: string[]
  bookmarkLink?: string
}

/** 提炼会话中的消息节点列表 */
const nodes = computed<TreeNode[]>(() => {
  return chat.messages
    .filter((m) => !m.pending && !m.error)
    .map((m, idx) => {
      const swipes = m.swipes && m.swipes.length > 0 ? m.swipes : [m.content]
      const activeSwipeId = m.swipeId ?? 0
      const content = m.content || ''
      const snippet = content.replace(/\s+/g, ' ').slice(0, 70) + (content.length > 70 ? '…' : '')
      return {
        index: idx,
        role: m.role,
        name: m.name || (m.role === 'user' ? chat.userName : (chat.currentCharacter?.name ?? 'AI')),
        content,
        snippet,
        swipesCount: swipes.length,
        activeSwipeId,
        swipes,
        bookmarkLink: m.bookmarkLink,
      }
    })
})

/** 切换 Swipe 分支 */
async function selectSwipe(nodeIndex: number, swipeId: number): Promise<void> {
  await chat.switchSwipe(nodeIndex, swipeId)
}

/** 从选定节点分叉出新会话 */
async function forkFromNode(nodeIndex: number): Promise<void> {
  await chat.branchFrom(nodeIndex)
  emit('close')
}

/** 定位滚动到指定消息 */
function scrollToNode(nodeIndex: number): void {
  const el = document.getElementById(`msg-${nodeIndex}`)
  if (el) {
    el.scrollIntoView({ behavior: 'smooth', block: 'center' })
  }
}
</script>

<template>
  <aside v-if="open" class="tree-drawer" aria-label="剧情分支树面板">
    <div class="tree-header">
      <div class="title-wrap">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="6" y1="3" x2="6" y2="15"></line>
          <circle cx="18" cy="6" r="3"></circle>
          <circle cx="6" cy="18" r="3"></circle>
          <path d="M18 9a9 9 0 0 1-9 9"></path>
        </svg>
        <h3>剧情分支树 (Chat Tree)</h3>
      </div>
      <button class="btn-close" title="关闭" @click="emit('close')">
        <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="18" y1="6" x2="6" y2="18"></line>
          <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
      </button>
    </div>

    <div class="tree-content">
      <div v-if="nodes.length === 0" class="empty-hint">
        当前会话暂无消息节点
      </div>

      <div v-else class="timeline-tree">
        <div v-for="(node, i) in nodes" :key="node.index" class="tree-tier">
          <!-- 连接竖线 -->
          <div v-if="i > 0" class="stem-line" />

          <!-- 节点卡片主体 -->
          <div class="node-card" :class="[node.role, { 'has-swipes': node.swipesCount > 1 }]">
            <div class="node-meta">
              <span class="badge-role" :class="node.role">
                {{ node.role === 'user' ? '用户' : (node.role === 'system' ? '旁白' : '角色') }}
              </span>
              <span class="speaker-name">{{ node.name }}</span>
              <span class="turn-index">#{{ node.index + 1 }}</span>
            </div>

            <!-- 正文摘要 -->
            <div class="node-snippet" title="点击定位到该消息" @click="scrollToNode(node.index)">
              {{ node.snippet || '（空消息）' }}
            </div>

            <!-- 多 Swipes 分支选择器 -->
            <div v-if="node.swipesCount > 1" class="swipes-bar">
              <span class="swipes-title">备选分支:</span>
              <button
                v-for="(_, sIdx) in node.swipes"
                :key="sIdx"
                class="btn-swipe"
                :class="{ active: sIdx === node.activeSwipeId }"
                :title="`切换至分支 ${sIdx + 1}`"
                @click="selectSwipe(node.index, sIdx)"
              >
                {{ sIdx + 1 }}
              </button>
            </div>

            <!-- 操作动作条 -->
            <div class="node-actions">
              <button class="act-btn" title="从此处新建独立分叉会话" @click="forkFromNode(node.index)">
                <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2">
                  <line x1="6" y1="3" x2="6" y2="15"></line>
                  <circle cx="18" cy="6" r="3"></circle>
                  <circle cx="6" cy="18" r="3"></circle>
                  <path d="M18 9a9 9 0 0 1-9 9"></path>
                </svg>
                <span>从此分叉</span>
              </button>

              <span v-if="node.bookmarkLink" class="bookmark-tag" title="书签归档">
                🔖 {{ node.bookmarkLink }}
              </span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.tree-drawer {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  width: 340px;
  max-width: 90vw;
  background: var(--bg-surface, #ffffff);
  border-left: 1px solid var(--border-color, #e5e7eb);
  box-shadow: -4px 0 16px rgba(0, 0, 0, 0.08);
  display: flex;
  flex-direction: column;
  z-index: 40;
}

.tree-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color, #e5e7eb);
  background: var(--bg-canvas, #fafafa);
}

.title-wrap {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-primary, #111827);
}

.title-wrap h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}

.btn-close {
  background: transparent;
  border: none;
  cursor: pointer;
  padding: 4px;
  border-radius: 4px;
  color: var(--text-secondary, #6b7280);
}

.btn-close:hover {
  background: var(--bg-hover, #f3f4f6);
}

.tree-content {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}

.empty-hint {
  text-align: center;
  color: var(--text-tertiary, #9ca3af);
  font-size: 13px;
  margin-top: 40px;
}

.timeline-tree {
  display: flex;
  flex-direction: column;
  align-items: center;
}

.tree-tier {
  width: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
}

.stem-line {
  width: 2px;
  height: 16px;
  background: var(--brand-primary, #3b82f6);
  opacity: 0.4;
}

.node-card {
  width: 100%;
  box-sizing: border-box;
  padding: 10px 12px;
  background: var(--bg-card, #ffffff);
  border: 1px solid var(--border-color, #e5e7eb);
  border-radius: 8px;
  font-size: 12px;
  transition: all 0.15s ease;
}

.node-card:hover {
  border-color: var(--brand-primary, #3b82f6);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.04);
}

.node-card.user {
  border-left: 3px solid #10b981;
}

.node-card.assistant {
  border-left: 3px solid #3b82f6;
}

.node-card.system {
  border-left: 3px solid #9ca3af;
  background: #f9fafb;
}

.node-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 6px;
}

.badge-role {
  padding: 1px 6px;
  border-radius: 4px;
  font-size: 10px;
  font-weight: 500;
}

.badge-role.user {
  background: #d1fae5;
  color: #065f46;
}

.badge-role.assistant {
  background: #dbeafe;
  color: #1e40af;
}

.badge-role.system {
  background: #e5e7eb;
  color: #374151;
}

.speaker-name {
  font-weight: 600;
  color: var(--text-primary, #111827);
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.turn-index {
  color: var(--text-tertiary, #9ca3af);
  font-size: 10px;
}

.node-snippet {
  color: var(--text-secondary, #4b5563);
  line-height: 1.4;
  cursor: pointer;
  word-break: break-all;
}

.node-snippet:hover {
  color: var(--brand-primary, #3b82f6);
}

.swipes-bar {
  margin-top: 8px;
  display: flex;
  align-items: center;
  gap: 4px;
  background: var(--bg-canvas, #f9fafb);
  padding: 4px 6px;
  border-radius: 4px;
}

.swipes-title {
  font-size: 10px;
  color: var(--text-tertiary, #6b7280);
}

.btn-swipe {
  border: 1px solid var(--border-color, #d1d5db);
  background: #ffffff;
  padding: 1px 6px;
  border-radius: 3px;
  font-size: 11px;
  cursor: pointer;
}

.btn-swipe.active {
  background: var(--brand-primary, #3b82f6);
  color: #ffffff;
  border-color: var(--brand-primary, #3b82f6);
}

.node-actions {
  margin-top: 8px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-top: 1px dashed var(--border-color, #f3f4f6);
  padding-top: 6px;
}

.act-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  background: transparent;
  border: none;
  color: var(--brand-primary, #3b82f6);
  font-size: 11px;
  cursor: pointer;
  padding: 2px 4px;
  border-radius: 3px;
}

.act-btn:hover {
  background: var(--bg-hover, #eff6ff);
}

.bookmark-tag {
  font-size: 10px;
  color: #8b5cf6;
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
