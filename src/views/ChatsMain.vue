<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useChatStore } from '@/stores/chat'
import { useSettingsStore } from '@/stores/settings'
import Avatar from '@/components/Avatar.vue'
import MessageBubble from '@/components/MessageBubble.vue'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import NewChatDialog from '@/components/NewChatDialog.vue'
import ChatPanel from '@/components/ChatPanel.vue'
import { dayLabel } from '@/utils/time'
import { fmtCost, fmtTok, priceOf } from '@/services/usage'
import type { DisplayMessage } from '@/services/st/chatdoc'
import type { NewChatOptions } from '@/stores/chat'

const chat = useChatStore()
const gen = useSettingsStore()

const draft = ref('')
const listEl = ref<HTMLElement | null>(null)

const char = computed(() => chat.currentCharacter)

/** 是否已打开具体会话（未打开时主区显示中性空状态，不显示自动选中的角色占位） */
const inSession = computed(() => !!chat.currentFile)

/* ---- 会话配置面板（顶栏 ··· 唤起，推挤式） ---- */
const panelOpen = ref(false)

/* ---- 新建会话：先配置、后开始（弹窗开关在 chat store，中栏按钮共用） ---- */
function onStartNewChat(opts: NewChatOptions): void {
  chat.newChatOpen = false
  if (char.value) void chat.newChatWithConfig(char.value, opts)
}

/* ---- 消息多选（批量删除） ---- */
const selectMode = ref(false)
const selected = ref(new Set<number>())
function toggleSelectMode(): void {
  selectMode.value = !selectMode.value
  selected.value = new Set()
}
function toggleSelect(i: number): void {
  const next = new Set(selected.value)
  if (next.has(i)) next.delete(i)
  else next.add(i)
  selected.value = next
}
const batchConfirm = ref(false)
async function confirmBatchDelete(): Promise<void> {
  batchConfirm.value = false
  await chat.removeMessages([...selected.value])
  selectMode.value = false
  selected.value = new Set()
}

/* ---- 消息操作（气泡下方工具条） ---- */
const removeConfirmAt = ref(-1)

function onEditMsg(i: number, content: string): void {
  void chat.editMessage(i, content)
}
function onRemoveMsg(i: number): void {
  removeConfirmAt.value = i
}
async function confirmRemoveMsg(): Promise<void> {
  const i = removeConfirmAt.value
  removeConfirmAt.value = -1
  if (i >= 0) await chat.removeMessage(i)
}
function onResendMsg(i: number): void {
  void chat.resendFrom(i)
}
function onBranchMsg(i: number): void {
  void chat.branchFrom(i)
}
function onBookmarkMsg(i: number): void {
  void chat.bookmarkMessage(i)
}
function onSwipeMsg(i: number, dir: -1 | 1): void {
  void chat.switchSwipe(i, dir)
}
function onSwipeTo(i: number, target: number): void {
  void chat.switchSwipeTo(i, target)
}
function onRegenerate(): void {
  if (chat.group) void chat.regenerateGroup()
  else void chat.regenerate()
}
function sendQuick(q: { label: string; message: string; mode?: 'send' | 'insert' }): void {
  if (q.mode === 'insert') {
    // 注入输入框：换行拼接，不自动发送
    draft.value = draft.value.trim() ? `${draft.value.trimEnd()}\n${q.message}` : q.message
    return
  }
  void chat.send(q.message)
}
async function onImpersonate(): Promise<void> {
  const text = await chat.impersonate()
  if (text) draft.value = draft.value.trim() ? `${draft.value.trimEnd()}\n${text}` : text
}

/** 消息列表 + 日期分隔（微信风格） */
type Row = { kind: 'day'; id: string; label: string } | { kind: 'msg'; id: string; m: DisplayMessage; i: number }
const rows = computed<Row[]>(() => {
  const out: Row[] = []
  let lastDay = ''
  chat.messages.forEach((m, i) => {
    const dl = dayLabel(m.sendDate)
    if (dl !== lastDay) {
      out.push({ kind: 'day', id: `d${i}`, label: dl })
      lastDay = dl
    }
    out.push({ kind: 'msg', id: `m${i}`, m, i })
  })
  return out
})

const connText = computed(
  () => ({ unknown: '检测连接…', online: '已连接 SillyTavern', offline: '后端未连接' })[chat.conn],
)

const usage = computed(() => {
  const p = chat.promptInfo
  if (!p) return ''
  const parts = []
  if (p.trimmed) parts.push(`裁剪 ${p.trimmed} 条历史`)
  if (p.examplesDropped) parts.push('示例未注入')
  if (p.wiActivated) parts.push(`世界书 ${p.wiActivated} 条`)
  return parts.join(' · ')
})

/** 用量胶囊（常驻）：优先本轮 ↑↓/费用，未生成时显示会话累计 */
const usagePill = computed(() => {
  const u = chat.lastUsage
  const input = u ? u.input : chat.sessionUsage.input
  const output = u ? u.output : chat.sessionUsage.output
  const cost = u?.costUsd ?? (chat.sessionUsage.cost || null)
  return {
    up: fmtTok(input),
    down: fmtTok(output),
    cost: cost != null ? `≈ ${fmtCost(cost)}` : '',
  }
})

/** 悬停明细：本轮 / 会话累计 / 单价 */
const usageTip = computed(() => {
  const su = chat.sessionUsage
  const u = chat.lastUsage
  const lines = [
    u ? `本轮：↑ ${u.input} · ↓ ${u.output}${u.measured ? '' : '（估算）'}` : '本轮：尚未生成',
    `本会话累计：↑ ${su.input} · ↓ ${su.output}${su.cost ? ` · ≈ ${fmtCost(su.cost)}` : ''}`,
  ]
  const model = u?.model ?? gen.model
  const p = model ? priceOf(model) : null
  lines.push(
    p
      ? `${p.label} 单价 $${p.inPrice}/$${p.outPrice} 每 1M tokens`
      : '该模型无价格数据，不估算费用',
  )
  return lines.join('\n')
})

async function send() {
  const text = draft.value
  if (!text.trim() || chat.streaming) return
  draft.value = ''
  await chat.send(text)
}

function scrollToEnd(force = false) {
  void nextTick(() => {
    const el = listEl.value
    if (!el) return
    // 粘底判断：流式期间用户向上翻历史时不被强行拽回底部；
    // 距底 120px 以内视为"在底部"。切会话时强制滚底。
    if (!force) {
      const fromBottom = el.scrollHeight - el.scrollTop - el.clientHeight
      if (fromBottom > 120) return
    }
    el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
  })
}

watch(
  () => chat.messages.map((m) => m.content.length).join(','),
  () => scrollToEnd(),
)
watch(
  () => chat.currentFile,
  () => scrollToEnd(true),
)
</script>

<template>
  <main class="main page" data-page="chats">
    <div class="chat-col">
      <header class="head">
        <template v-if="inSession && char">
          <Avatar class="ava" :avatar="char.avatar" :name="char.name" />
          <div class="who">
            <span class="nm">{{ char.name }}</span>
            <span class="sub">{{ chat.currentFile }}</span>
          </div>
        </template>
        <template v-else>
          <span class="nm">聊天</span>
        </template>

        <div class="acts">
          <button
            v-if="inSession && !chat.streaming"
            class="dots select-toggle"
            :class="{ on: selectMode }"
            :title="selectMode ? '退出多选' : '多选消息'"
            @click="toggleSelectMode"
          >
            <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 11l3 3L22 4" /><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11" /></svg>
          </button>
          <button
            class="dots"
            :class="{ on: panelOpen }"
            title="会话设置"
            @click="panelOpen = !panelOpen"
          >
            <i /><i /><i />
          </button>
        </div>
      </header>

    <div ref="listEl" class="msgs">
      <!-- 未打开会话：中性空状态（不显示自动选中的角色占位） -->
      <div v-if="!inSession" class="empty">
        <div class="big">
          <svg
            viewBox="0 0 24 24"
            width="30"
            height="30"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <path d="M21 12a8 8 0 0 1-8 8H4l1.5-3A8 8 0 1 1 21 12z" />
          </svg>
        </div>
        <p>未选择会话</p>
        <button v-if="char" class="btn btn-primary" @click="chat.newChatOpen = true">
          新建会话
        </button>
      </div>

      <!-- 已打开会话但无消息（新建空会话的过渡态） -->
      <div v-else-if="!chat.messages.length" class="empty">
        <div class="big">
          <svg
            viewBox="0 0 24 24"
            width="30"
            height="30"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <path d="M21 12a8 8 0 0 1-8 8H4l1.5-3A8 8 0 1 1 21 12z" />
          </svg>
        </div>
        <p>{{ char?.name }} 还没有会话记录</p>
        <button class="btn btn-primary" @click="chat.newChatOpen = true">开始新对话</button>
      </div>

      <template v-else>
        <div v-for="r in rows" :key="r.id">
          <div v-if="r.kind === 'day'" class="daysep">{{ r.label }}</div>

          <div v-else class="msg-row" :class="{ selectable: selectMode }">
            <label v-if="selectMode" class="pick">
              <input
                type="checkbox"
                :checked="selected.has(r.i)"
                @change="toggleSelect(r.i)"
              />
            </label>
            <MessageBubble
            :m="r.m"
            :index="r.i"
            :is-last="r.i === chat.messages.length - 1"
            :avatar="char?.avatar ?? ''"
            :streaming="chat.streaming"
            :translation="chat.translations[r.i]"
            :speaking="chat.speakingIndex === r.i"
            :speaker="chat.group ? r.m.name : undefined"
            :group-chat="!!chat.group"
            @edit="onEditMsg"
            @remove="onRemoveMsg"
            @resend="onResendMsg"
            @regenerate="onRegenerate"
            @branch="onBranchMsg"
            @bookmark="onBookmarkMsg"
            @swipe="onSwipeMsg"
            @swipe-to="onSwipeTo"
            @hide="(i: number) => chat.toggleHideMessage(i)"
            @translate="chat.translateMessage(r.i)"
            @speak="chat.speakMessage(r.i)"
            @continue="chat.continueReply()"
          />
          </div>
        </div>
      </template>
    </div>

    <!-- 新建会话配置弹窗：先配置（名称/开场白/人设/参数），再开始 -->
    <NewChatDialog
      :open="chat.newChatOpen"
      :character="char"
      @cancel="chat.newChatOpen = false"
      @start="onStartNewChat"
    />

    <!-- 多选操作条 -->
    <div v-if="selectMode" class="batchbar">
      <span>已选 {{ selected.size }} 条</span>
      <button class="btn" @click="toggleSelectMode">取消</button>
      <button
        class="btn btn-primary"
        :disabled="!selected.size || chat.streaming"
        @click="batchConfirm = true"
      >删除所选</button>
    </div>

    <!-- 批量删除二次确认 -->
    <ConfirmDialog
      :open="batchConfirm"
      title="批量删除消息"
      :message="'将删除所选 ' + selected.size + ' 条消息，删除后不可恢复'"
      confirm-text="删除"
      @confirm="confirmBatchDelete"
      @cancel="batchConfirm = false"
    />

    <!-- 删除消息二次确认 -->
    <ConfirmDialog
      :open="removeConfirmAt >= 0"
      title="删除消息"
      message="删除后不可恢复"
      confirm-text="删除"
      @confirm="confirmRemoveMsg"
      @cancel="removeConfirmAt = -1"
    />

    <!-- 状态条：连接 · 模型 · 用量胶囊（本轮 tokens/费用，悬停看会话累计） -->
    <div class="statusbar">
      <span class="dot-mini" :class="chat.conn" :title="connText" />
      <span class="model">{{ gen.model || '未选模型' }}</span>
      <span
        v-if="usagePill"
        class="pill"
        :title="usageTip"
      >
        <span class="up">↑ {{ usagePill.up }}</span>
        <span class="psep" />
        <span class="down">↓ {{ usagePill.down }}</span>
        <template v-if="usagePill.cost">
          <span class="psep" />
          <span class="cost">{{ usagePill.cost }}</span>
        </template>
      </span>
      <span v-if="chat.saving" class="tag">保存中…</span>
      <span v-if="chat.scriptEcho" class="tag">🗒 {{ chat.scriptEcho }}</span>
      <span v-else-if="chat.dirty" class="tag">未保存</span>
      <span class="spacer" />
      <span v-if="usage" class="usage">{{ usage }}</span>
    </div>

    <div class="input-area">
      <!-- 快捷回复按钮条 -->
      <div v-if="chat.quickReplies.length" class="qr-bar">
        <button
          v-for="(q, k) in chat.quickReplies"
          :key="k"
          class="qr"
          type="button"
          :disabled="chat.streaming"
          :title="q.message"
          @click="sendQuick(q)"
        >
          {{ q.label }}
        </button>
      </div>
      <div class="input-box">
        <textarea
          v-model="draft"
          class="ta"
          :placeholder="
            !inSession
              ? '选择或新建一个会话后开始聊天'
              : `发消息给 ${char?.name ?? ''}…（Enter 发送 / Shift+Enter 换行）`
          "
          rows="2"
          :disabled="!inSession || chat.streaming"
          @keydown.enter.exact.prevent="send"
        />
        <div class="tools">
          <span v-if="chat.lastError" class="err-tip" :title="chat.lastError">{{ chat.lastError }}</span>
          <!-- 群聊：再来一轮（按当前编排重掷发言人） -->
          <button
            v-if="chat.group && !chat.streaming"
            class="btn-impersonate"
            type="button"
            title="不发送消息，直接让群成员再发言一轮"
            @click="onRegenerate"
          >
            再来一轮
          </button>
          <!-- 代写：以用户身份生成建议输入（结果进草稿，不自动发送） -->
          <button
            v-if="inSession && !chat.group"
            class="btn-impersonate"
            type="button"
            :disabled="chat.streaming"
            title="代写：让模型以你的口吻起草一条消息（写入输入框）"
            @click="onImpersonate"
          >
            代写
          </button>
          <!-- 流式期间变「停止」：中断 SSE，已生成部分保留落盘 -->
          <button
            class="btn-send"
            :class="{ stop: chat.streaming }"
            :disabled="!inSession || (!chat.streaming && !draft.trim())"
            @click="chat.streaming ? chat.stopGeneration() : send()"
          >
            {{ chat.streaming ? '停 止' : '发 送' }}
          </button>
        </div>
      </div>
    </div>
    </div>

    <!-- 会话配置面板：推挤式二级内容区（展开/收起不中断流式） -->
    <ChatPanel :open="panelOpen" @close="panelOpen = false" />
  </main>
</template>

<style scoped>
.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: row; /* 聊天列 + 会话配置面板（推挤式二级内容区） */
}
.chat-col {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.head {
  height: 56px;
  flex: none;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 16px;
  border-bottom: 1px solid var(--c-border);
}
.head .ava {
  width: 32px;
  height: 32px;
  border-radius: 50%;
  object-fit: cover;
  background: var(--c-panel);
}
.who {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}
.nm {
  font-size: 14px;
  font-weight: 600;
}
.sub {
  font-size: 10px;
  color: var(--c-text-3);
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.acts {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 12px;
}
/* 微信风格「···」会话设置按钮（三个圆点用 <i> 画，不依赖字体） */
.dots {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 7px 9px;
  border-radius: var(--radius-md);
  transition: background 0.15s;
}
.dots i {
  width: 3.5px;
  height: 3.5px;
  border-radius: 50%;
  background: var(--c-text-2);
}
.dots:hover,
.dots.on {
  background: var(--c-panel);
}
.dots.on i,
.dots:hover i {
  background: var(--c-text);
}
.ghost:hover:not(:disabled) {
  border-color: var(--p-400);
  color: var(--p-600);
}
.ghost:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.msgs {
  flex: 1;
  overflow-y: auto;
  padding: 16px 20px 6px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.empty {
  margin: auto;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  color: var(--c-text-3);
  font-size: 12px;
  text-align: center;
}
.empty .big {
  font-size: 30px;
}
.daysep {
  margin: 6px auto 2px;
  padding: 2px 9px;
  font-size: 10px;
  color: var(--c-text-3);
  background: var(--c-panel);
  border-radius: 9px;
  width: fit-content;
}

.statusbar {
  flex: none;
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 5px 18px;
  font-size: 10px;
  color: var(--c-text-3);
  border-top: 1px dashed var(--c-border);
}
.dot-mini {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--c-text-3);
}
.dot-mini.online {
  background: var(--s-success);
}
.dot-mini.offline {
  background: var(--s-error);
}
.model {
  font-family: var(--font-mono);
  color: var(--c-text-2);
}
/* 用量胶囊：本轮 ↑输入 ↓输出 ≈费用（悬停看会话累计与单价） */
.pill {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 2px 9px;
  border-radius: 99px;
  background: var(--c-panel);
  font-family: var(--font-mono);
  cursor: default;
}
.pill .up {
  color: var(--p-600);
}
.pill .down {
  color: var(--s-success);
}
.pill .cost {
  color: var(--s-warning);
}
.psep {
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: var(--c-border-strong);
  opacity: 0.6;
}
.tag {
  padding: 0 5px;
  border-radius: 6px;
  background: var(--c-panel);
}
.spacer {
  margin-left: auto;
}
.usage {
  font-family: var(--font-mono);
}

.input-area {
  flex: none;
  padding: 8px 16px 12px;
}
.qr-bar {
  display: flex;
  gap: 6px;
  overflow-x: auto;
  padding-bottom: 6px;
}
.qr {
  flex: none;
  padding: 3px 10px;
  font-size: 11px;
  border: 1px solid var(--p-400, var(--c-border));
  border-radius: 999px;
  color: var(--p-600);
  background: var(--c-panel);
  white-space: nowrap;
}
.qr:hover:not(:disabled) {
  background: var(--p-50);
}
.qr:disabled {
  opacity: 0.5;
}
.input-box {
  border: 1px solid var(--c-border);
  border-radius: var(--radius-lg);
  background: var(--c-bg);
  transition: border-color 0.2s;
}
.input-box:focus-within {
  border-color: var(--p-400);
}
.ta {
  width: 100%;
  border: none;
  outline: none;
  resize: none;
  background: transparent;
  color: var(--c-text);
  padding: 11px 14px 6px;
  font-size: 13px;
  line-height: 1.6;
}
.ta:disabled {
  cursor: not-allowed;
}
.tools {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 10px 8px;
}
.err-tip {
  flex: 1;
  min-width: 0;
  font-size: 11px;
  color: var(--s-error);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.btn-send {
  margin-left: auto;
  background: var(--p-500);
  color: var(--c-on-primary);
  border-radius: var(--radius-md);
  font-size: 12px;
  font-weight: 600;
  padding: 6px 18px;
  transition:
    transform 0.15s ease,
    background 0.2s;
}
/* 代写按钮（输入框工具条） */
.btn-impersonate {
  padding: 6px 12px;
  font-size: 12px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  color: var(--c-text-2);
  background: var(--c-bg);
}
.btn-impersonate:hover:not(:disabled) {
  border-color: var(--p-400);
  color: var(--p-600);
}
.btn-impersonate:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.btn-send:hover:not(:disabled) {
  background: var(--p-600);
  transform: scale(1.04);
}
.btn-send:active:not(:disabled) {
  transform: scale(0.96);
}
.btn-send:disabled {
  background: var(--c-border-strong);
  transform: none;
  cursor: not-allowed;
}
/* 停止态：中性底色，与发送区分 */
.btn-send.stop {
  background: var(--c-border-strong);
  color: var(--c-text);
}
.btn-send.stop:hover:not(:disabled) {
  background: var(--c-border);
  transform: none;
}

/* ---- 消息多选 ---- */
.msg-row.selectable {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding-inline-start: 6px;
}
.msg-row .pick {
  flex: none;
  margin-top: 14px;
  display: flex;
  align-items: center;
  cursor: pointer;
}
.batchbar {
  position: fixed;
  left: 50%;
  transform: translateX(-50%);
  bottom: 52px;
  z-index: 60;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px;
  border-radius: 10px;
  background: var(--c-panel, #fff);
  border: 1px solid var(--c-line, #ddd);
  box-shadow: 0 6px 20px rgb(0 0 0 / 15%);
  font-size: 13px;
}
.select-toggle.on {
  color: var(--c-primary, #4a8fd4);
}

</style>
