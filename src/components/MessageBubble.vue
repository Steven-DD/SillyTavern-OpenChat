<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { Streamdown } from 'streamdown-vue3'
import 'streamdown-vue3/styles.css'
import Avatar from '@/components/Avatar.vue'
import { bubbleTime } from '@/utils/time'
import { useThemeStore } from '@/stores/theme'
import { applyDisplayRegex, REGEX_PLACEMENT } from '@/services/st/regex'
import { detectFenceLanguages } from '@/utils/codeDetect'
import { normalizeLegacyHtml } from '@/utils/legacyHtml'
import DOMPurify from 'dompurify'
import type { DisplayMessage } from '@/services/st/chatdoc'

/**
 * 单条消息气泡：Markdown 流式渲染（AI，shiki 代码高亮 + rehype-sanitize 净化）
 * + 显示通路正则脚本 + 推理折叠块 + 操作工具条（编辑/重发/分支/书签/删除/翻译/朗读）
 * + Swipe 切换/picker（末条 AI）。
 */
const props = defineProps<{
  m: DisplayMessage
  index: number
  isLast: boolean
  avatar: string
  streaming: boolean
  translation?: string
  /** 该条消息正在翻译（按钮加载态） */
  translating?: boolean
  speaking?: boolean
  /** 群聊发言人名（非空 = 群聊模式：气泡上方显示发言者，隐藏单会话专属操作） */
  speaker?: string
  groupChat?: boolean
}>()

const emit = defineEmits<{
  edit: [index: number, content: string]
  remove: [index: number]
  resend: [index: number]
  regenerate: []
  continue: [index: number]
  branch: [index: number]
  bookmark: [index: number]
  swipe: [index: number, dir: -1 | 1]
  swipeTo: [index: number, target: number]
  translate: [index: number]
  speak: [index: number]
  hide: [index: number]
}>()

/* ---- 推理折叠：流式中自动展开，结束后自动收起；点击手动切换 ---- */
const reasonManual = ref<boolean | null>(null)
const reasonOpen = computed(() => reasonManual.value ?? !!props.m.pending)

/* ---- 行内编辑 ---- */
const editing = ref(false)
const editText = ref('')
const ta = ref<HTMLTextAreaElement | null>(null)

function startEdit(): void {
  editText.value = props.m.content
  editing.value = true
  void nextTick(() => {
    ta.value?.focus()
    ta.value?.setSelectionRange(editText.value.length, editText.value.length)
  })
}
function cancelEdit(): void {
  editing.value = false
}
function saveEdit(): void {
  const t = editText.value.trim()
  editing.value = false
  if (t && t !== props.m.content) emit('edit', props.index, t)
}

/* ---- 复制 ---- */
const copied = ref(false)
async function copyText(): Promise<void> {
  try {
    await navigator.clipboard.writeText(props.m.content)
    copied.value = true
    setTimeout(() => (copied.value = false), 1200)
  } catch {
    /* 剪贴板不可用则忽略 */
  }
}

/* ---- 翻译：译文替换气泡内容显示，可一键回看原文（对齐 ST 翻译扩展替换式语义） ---- */
const showTrans = ref(false)
/* 译文到达 → 自动切换为译文；译文被清（换会话/重排）→ 回到原文 */
watch(
  () => props.translation,
  (v, ov) => {
    if (v && !ov) showTrans.value = true
    if (!v) showTrans.value = false
  },
)
const transBtnTitle = computed(() =>
  props.translating ? '翻译中…' : props.translation ? (showTrans.value ? '显示原文' : '显示译文') : '翻译',
)
function onTranslateClick(): void {
  if (props.translating) return
  if (!props.translation) {
    emit('translate', props.index)
    return
  }
  showTrans.value = !showTrans.value
}

/** 末条 AI 回复恒显 swipe 条：无 swipes 字段时按 1 条虚拟备选（右滑即生成） */
const swipeCount = computed(() => props.m.swipes?.length || 1)
const swipeAt = computed(() => props.m.swipeId ?? 0)

/* ---- 显示通路正则（正则脚本由 chat store 预加载到模块缓存） ---- */
const displayContent = computed(() => {
  const raw = applyDisplayRegex(
    props.m.content,
    props.m.role === 'user' ? REGEX_PLACEMENT.USER_INPUT : REGEX_PLACEMENT.AI_OUTPUT,
  )
  // 仅 assistant 走 Markdown 通路：先探测围栏语言，再把 font/center/u 等
  // 旧式着色标签改写为 Streamdown 净化白名单内的等价写法（否则颜色全丢）
  return props.m.role === 'assistant' ? normalizeLegacyHtml(detectFenceLanguages(raw)) : raw
})
const displayReasoning = computed(() =>
  normalizeLegacyHtml(applyDisplayRegex(props.m.reasoning ?? '', REGEX_PLACEMENT.REASONING)),
)

/**
 * 消息 HTML 渲染（P11 对齐：角色卡/HTML 消息按净化后的 HTML 渲染，其余走 Markdown）。
 * 判定：内容以 HTML 标签开头且含块级闭合标签（卡片式整段 HTML）；
 * 净化用 DOMPurify 默认白名单（script/事件属性恒拒收）。
 */
const HTML_OPEN = /^\s*<[a-zA-Z!][^>]*>/
const HTML_BLOCK_CLOSE = /<\/(div|p|table|span|style|pre|section|article|details|h[1-6]|ul|ol|dl|blockquote|center|font|body|html)>/i
const htmlContent = computed(() => {
  if (props.m.role !== 'assistant') return ''
  const raw = displayContent.value
  if (!HTML_OPEN.test(raw) || !HTML_BLOCK_CLOSE.test(raw)) return ''
  // 整段 HTML 通路同样改写：DOMPurify 虽放行 font，但改写成 span+style 与 Markdown 通路视觉一致
  return DOMPurify.sanitize(normalizeLegacyHtml(raw), { FORCE_BODY: true })
})

/* ---- 代码高亮主题跟随应用主题（shiki 双主题对） ---- */
const theme = useThemeStore()
const shikiThemes = computed<[string, string]>(() => {
  const dark =
    theme.mode === 'dark' ? 'github-dark' : theme.mode === 'dusk' ? 'one-dark-pro' : 'github-dark'
  return ['github-light', dark]
})

/* ---- Swipe picker：点 n/N 弹出备选预览列表 ---- */
const pickerOpen = ref(false)

function swipePreview(s: string): string {
  const t = s.replace(/\s+/g, ' ').trim()
  return t.length > 64 ? `${t.slice(0, 64)}…` : t || '（空）'
}

function pickSwipe(i: number): void {
  pickerOpen.value = false
  if (i !== swipeAt.value) emit('swipeTo', props.index, i)
}

const nextTitle = computed(() =>
  swipeAt.value >= swipeCount.value - 1 ? '生成新备选（overswipe）' : '下一备选',
)
</script>

<template>
  <div class="msg" :class="[m.role, { hiddenMsg: m.isSystem }]">
    <Avatar v-if="m.role === 'assistant'" class="mava" :avatar="avatar" :name="m.name" />
    <div class="wrap">
      <div class="meta">
        <span class="mn">{{ m.name }}</span>
        <span v-if="m.isSystem" class="hidden-badge">已隐藏 · 不进上下文</span>
        <span class="msg-time">{{ bubbleTime(m.sendDate) }}</span>
      </div>

      <!-- 推理折叠块（extra.reasoning） -->
      <div v-if="m.reasoning" class="reason">
        <button class="reason-head" type="button" @click="reasonManual = !reasonOpen">
          <span class="rh-icon">✦</span>
          <span>{{ m.pending && streaming ? '深度思考中…' : '已深度思考' }}</span>
          <span class="rh-arrow" :class="{ open: reasonOpen }">▾</span>
        </button>
        <div v-show="reasonOpen" class="reason-body">
          <Streamdown :content="displayReasoning" :shiki-theme="shikiThemes" />
        </div>
      </div>

      <!-- 气泡：AI 走 Markdown 流式渲染（整段 HTML 消息走净化后直渲染），用户保持纯文本；译文态整体替换内容 -->
      <div v-if="!editing" class="bubble" :class="{ err: m.error }">
        <div v-if="showTrans && translation" class="txt trans-txt">{{ translation }}</div>
        <template v-else>
          <div
            v-if="m.role === 'assistant' && htmlContent"
            class="txt md cardhtml"
            v-html="htmlContent"
          />
          <Streamdown v-else-if="m.role === 'assistant'" class="txt md" :content="displayContent" :shiki-theme="shikiThemes" />
          <span v-else class="txt">{{ displayContent }}</span
          ><span v-if="m.pending && streaming" class="caret-el">▍</span>
        </template>
      </div>

      <!-- 原文/译文切换（仅已有译文时出现） -->
      <div v-if="translation && !editing" class="trans-bar">
        <button class="tbtn" type="button" @click="showTrans = !showTrans">
          {{ showTrans ? '显示原文' : '显示译文' }}
        </button>
      </div>

      <!-- 行内编辑态 -->
      <div v-if="editing" class="edit-box" @keydown.esc="cancelEdit">
        <textarea
          ref="ta"
          v-model="editText"
          class="ta"
          rows="4"
          @keydown.enter.exact.prevent="saveEdit"
        ></textarea>
        <div class="eb-btns">
          <button class="mbtn" type="button" @click="cancelEdit">取消</button>
          <button class="mbtn ok" type="button" @click="saveEdit">保存</button>
        </div>
      </div>

      <!-- 群聊：发言者名 -->
      <div v-if="groupChat && speaker && !editing" class="speaker">{{ speaker }}</div>

      <!-- Swipe 切换 + picker（末条 AI 恒显，群聊不适用） -->
      <div
        v-if="!groupChat && m.role === 'assistant' && isLast && !m.pending && !m.error && !editing"
        class="swipe-bar"
      >
        <button
          class="sw"
          type="button"
          :disabled="streaming || swipeAt <= 0"
          title="上一备选"
          @click="emit('swipe', index, -1)"
        >
          ‹
        </button>
        <div class="sw-mid">
          <button
            class="sw sw-num"
            type="button"
            :disabled="streaming"
            :title="`备选列表（共 ${swipeCount} 条）`"
            @click="pickerOpen = !pickerOpen"
          >
            {{ swipeAt + 1 }}/{{ swipeCount }}
          </button>
          <!-- 备选预览列表 -->
          <div v-if="pickerOpen" class="sw-picker">
            <button
              v-for="(s, i) in m.swipes ?? [m.content]"
              :key="i"
              class="sw-item"
              :class="{ on: i === swipeAt }"
              type="button"
              @click="pickSwipe(i)"
            >
              <span class="sw-i">{{ i + 1 }}</span>
              <span class="sw-pv">{{ swipePreview(s) }}</span>
            </button>
          </div>
        </div>
        <button
          class="sw"
          type="button"
          :disabled="streaming"
          :title="nextTitle"
          @click="emit('swipe', index, 1)"
        >
          ›
        </button>
      </div>

      <!-- 操作工具条：气泡下方，悬停显示（参考 ST 消息操作布局） -->
      <div class="ops" :class="{ user: m.role === 'user' }">
        <span
          v-if="m.bookmarkLink"
          class="op bm"
          :title="`已书签 → ${m.bookmarkLink}`"
        >
          ⚑
        </span>
        <button class="op" type="button" :title="copied ? '已复制' : '复制'" @click="copyText">
          {{ copied ? '✓' : '⧉' }}
        </button>
        <button v-if="!editing && !groupChat" class="op" type="button" title="编辑" @click="startEdit">✎</button>
        <button
          v-if="m.role === 'user' && !groupChat"
          class="op"
          type="button"
          title="重发（删除此后的消息并重新生成）"
          @click="emit('resend', index)"
        >
          ↻
        </button>
        <button
          v-else-if="!groupChat && m.role === 'assistant' && isLast && !m.pending && !m.error"
          class="op"
          type="button"
          title="继续生成（从当前内容续写）"
          @click="emit('continue', index)"
        >
          ⏵
        </button>
        <button
          v-if="!groupChat && m.role === 'assistant' && isLast && !m.pending && !m.error"
          class="op"
          type="button"
          title="重新生成（保留旧回复为备选）"
          @click="emit('regenerate')"
        >
          ⟳
        </button>
        <button v-if="!groupChat" class="op" type="button" title="从此分支" @click="emit('branch', index)">⑂</button>
        <button v-if="!groupChat" class="op" type="button" title="添加书签" @click="emit('bookmark', index)">⚑</button>
        <button
          class="op"
          type="button"
          :class="{ active: !!translation && showTrans, busy: translating }"
          :title="transBtnTitle"
          @click="onTranslateClick"
        >
          {{ translating ? '◌' : '译' }}
        </button>
        <button
          class="op"
          type="button"
          :class="{ active: speaking }"
          :title="speaking ? '停止朗读' : '朗读'"
          @click="emit('speak', index)"
        >
          {{ speaking ? '■' : '🔊' }}
        </button>
        <button
          v-if="!groupChat"
          class="op"
          type="button"
          :class="{ active: m.isSystem }"
          :title="m.isSystem ? '取消隐藏（恢复进 prompt）' : '隐藏（不进 prompt）'"
          @click="emit('hide', index)"
        >
          {{ m.isSystem ? '👁' : '🚫' }}
        </button>
        <button v-if="!groupChat" class="op danger" type="button" title="删除" @click="emit('remove', index)">✕</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.msg {
  display: flex;
  gap: 9px;
  padding: 3px 0;
}
.msg.user {
  flex-direction: row-reverse;
}
/* 隐藏消息（is_system）：半透明 + 角标，不进 prompt */
.msg.hiddenMsg .bubble {
  opacity: 0.55;
}
.hidden-badge {
  font-size: 9px;
  padding: 0 5px;
  border-radius: 8px;
  background: var(--c-panel);
  color: var(--c-text-3);
}
/* 群聊发言人名 */
.speaker {
  margin: 0 4px 2px;
  font-size: 10px;
  color: var(--c-text-3);
}
.mava {
  width: 34px;
  height: 34px;
  flex: none;
  border-radius: 50%;
  object-fit: cover;
  background: var(--c-panel);
  margin-top: 20px;
}
.wrap {
  display: flex;
  flex-direction: column;
  max-width: 78%;
  min-width: 0;
}
.msg.user .wrap {
  align-items: flex-end;
}
.meta {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin: 0 2px 3px;
}
.mn {
  font-size: 11px;
  font-weight: 600;
  color: var(--c-text-2);
}
.msg-time {
  font-size: 10px;
  color: var(--c-text-3);
}

/* 推理折叠块 */
.reason {
  width: 100%;
  margin-bottom: 4px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-panel-2, var(--c-panel));
  overflow: hidden;
}
.reason-head {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 6px 10px;
  font-size: 11px;
  color: var(--c-text-2);
  text-align: left;
}
.reason-head:hover {
  color: var(--c-text);
}
.rh-icon {
  color: var(--p-400, var(--c-text-3));
}
.rh-arrow {
  margin-left: auto;
  transition: transform 0.15s;
}
.rh-arrow.open {
  transform: rotate(180deg);
}
.reason-body {
  max-height: 260px;
  overflow-y: auto;
  padding: 2px 12px 8px;
  font-size: 12px;
  line-height: 1.7;
  color: var(--c-text-2);
  border-top: 1px dashed var(--c-border);
}
.reason-body :deep(p) {
  margin: 6px 0;
}

/* 气泡 */
.bubble {
  padding: 8px 12px;
  border-radius: var(--radius-lg);
  background: var(--bubble-ai);
  font-size: 13px;
  line-height: 1.65;
  color: var(--c-text);
  word-break: break-word;
  min-width: 0;
}
.msg.user .bubble {
  background: var(--bubble-user);
}
.bubble.err {
  background: var(--s-error-bg, var(--bubble-ai));
  color: var(--s-error);
}
.txt {
  white-space: pre-wrap;
}
.caret-el {
  display: inline-block;
  margin-left: 1px;
  color: var(--c-text-3);
  animation: blink 0.9s infinite;
}

/* Markdown 内容（覆盖 streamdown 默认边距，适配气泡） */
.md :deep(p) {
  margin: 0 0 6px;
}
.md :deep(p:last-child) {
  margin-bottom: 0;
}
.md :deep(pre) {
  margin: 6px 0;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--c-text) 7%, transparent);
  overflow-x: auto;
  font-size: 12px;
}
.md :deep(code) {
  font-family: var(--font-mono, ui-monospace, monospace);
  font-size: 12px;
}
.md :deep(:not(pre) > code) {
  padding: 1px 5px;
  border-radius: 4px;
  background: color-mix(in srgb, var(--c-text) 9%, transparent);
}
.md :deep(ul),
.md :deep(ol) {
  margin: 6px 0;
  padding-left: 20px;
}
/* ST 主题文本着色（对齐网页端 .mes_text em/i、u、q、blockquote）：
   变量由 ChatsMain 从 ST settings.json power_user 当前主题注入，
   浅色主题不注入时回落 inherit，保持可读 */
.md :deep(em),
.md :deep(i),
.reason-body :deep(em),
.reason-body :deep(i) {
  color: var(--st-c-em, inherit);
}
.md :deep(u),
.reason-body :deep(u) {
  color: var(--st-c-underline, inherit);
}
.md :deep(q),
.reason-body :deep(q) {
  color: var(--st-c-quote, inherit);
}
.md :deep(blockquote) {
  margin: 6px 0;
  padding: 2px 10px;
  border-left: 3px solid var(--st-c-quote, var(--c-border));
  color: var(--c-text-2);
}
.md :deep(table) {
  border-collapse: collapse;
  margin: 6px 0;
  font-size: 12px;
}
.md :deep(th),
.md :deep(td) {
  border: 1px solid var(--c-border);
  padding: 4px 8px;
}
.md :deep(a) {
  color: var(--p-500, var(--c-text));
  text-decoration: underline;
}

/* 行内编辑 */
.edit-box {
  width: 100%;
}
.edit-box .ta {
  width: 100%;
  padding: 8px 10px;
  border: 1px solid var(--p-400, var(--c-border));
  border-radius: var(--radius-lg);
  background: var(--c-bg);
  color: var(--c-text);
  font-size: 13px;
  line-height: 1.6;
  resize: vertical;
}
.eb-btns {
  display: flex;
  justify-content: flex-end;
  gap: 6px;
  margin-top: 4px;
}
.mbtn {
  padding: 3px 12px;
  border-radius: var(--radius-md);
  border: 1px solid var(--c-border);
  font-size: 11px;
  color: var(--c-text-2);
}
.mbtn.ok {
  background: var(--p-500);
  border-color: var(--p-500);
  color: #fff;
}

/* Swipe */
.swipe-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 4px;
}
.sw {
  width: 22px;
  height: 20px;
  display: grid;
  place-items: center;
  border-radius: var(--radius-sm);
  border: 1px solid var(--c-border);
  color: var(--c-text-2);
  font-size: 13px;
  line-height: 1;
}
.sw:hover:not(:disabled) {
  border-color: var(--p-400);
  color: var(--c-text);
}
.sw:disabled {
  opacity: 0.35;
}
.sw-num {
  width: auto;
  min-width: 34px;
  padding: 0 6px;
  font-size: 10px;
  color: var(--c-text-3);
}
.sw-mid {
  position: relative;
}
/* 备选预览列表（swipe picker） */
.sw-picker {
  position: absolute;
  bottom: calc(100% + 6px);
  left: 50%;
  transform: translateX(-50%);
  z-index: 20;
  min-width: 220px;
  max-width: 320px;
  max-height: 220px;
  overflow-y: auto;
  padding: 4px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
  box-shadow: 0 6px 24px rgb(0 0 0 / 18%);
}
.sw-item {
  display: flex;
  align-items: baseline;
  gap: 8px;
  width: 100%;
  padding: 6px 8px;
  text-align: left;
  border-radius: var(--radius-sm);
  font-size: 11px;
}
.sw-item:hover {
  background: var(--c-panel);
}
.sw-item.on {
  background: var(--p-50);
}
.sw-item.on .sw-i {
  color: var(--p-600);
}
.sw-i {
  flex: none;
  min-width: 16px;
  font-size: 10px;
  color: var(--c-text-3);
}
.sw-pv {
  flex: 1;
  min-width: 0;
  color: var(--c-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sw-item.on .sw-pv {
  color: var(--c-text);
}

/* 操作工具条 */
.ops {
  display: flex;
  gap: 2px;
  margin-top: 4px;
  opacity: 0;
  transition: opacity 0.12s;
}
.msg:hover .ops {
  opacity: 1;
}
.msg.user .ops {
  justify-content: flex-end;
}
.op {
  width: 24px;
  height: 22px;
  display: grid;
  place-items: center;
  border-radius: var(--radius-sm);
  color: var(--c-text-3);
  font-size: 12px;
  line-height: 1;
}
.op:hover {
  background: var(--c-panel);
  color: var(--c-text);
}
.op.danger:hover {
  background: color-mix(in srgb, var(--s-error) 12%, transparent);
  color: var(--s-error);
}
.op.bm {
  color: var(--s-warning, #d99a1f);
  cursor: default;
}
.op.active {
  background: var(--p-50);
  color: var(--p-600);
}

/* 翻译：译文态直接替换气泡内容；下方只留轻量切换 */
.trans-txt {
  color: var(--c-text);
}
.trans-bar {
  display: flex;
  margin-top: 2px;
}
.msg.user .trans-bar {
  justify-content: flex-end;
}
.tbtn {
  padding: 1px 4px;
  font-size: 10px;
  color: var(--c-text-3);
  border-radius: var(--radius-sm);
  transition: color 0.12s;
}
.tbtn:hover {
  color: var(--p-500, var(--c-text));
}
.op.busy {
  color: var(--p-500, var(--c-text-2));
  animation: busy-pulse 1s ease-in-out infinite;
}
@keyframes busy-pulse {
  0%,
  100% {
    opacity: 0.35;
  }
  50% {
    opacity: 1;
  }
}
</style>
