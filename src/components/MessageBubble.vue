<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { MarkdownRender, setCustomComponents } from 'markstream-vue'
import 'markstream-vue/index.css'
import ThinkingBlock from '@/components/ThinkingBlock.vue'
import Avatar from '@/components/Avatar.vue'
import { bubbleTime } from '@/utils/time'
import { useThemeStore } from '@/stores/theme'
import { applyDisplayRegex, REGEX_PLACEMENT } from '@/services/st/regex'
import { adaptCardMarkup, splitEmbedSegments } from '@/services/st/embed'
import HtmlEmbedFrame from '@/components/HtmlEmbedFrame.vue'
import { detectFenceLanguages } from '@/utils/codeDetect'
import { substituteMacros } from '@/services/st/prompt'
import type { DisplayMessage } from '@/services/st/chatdoc'

/**
 * 单条消息气泡：markstream-vue 流式 Markdown 渲染（AI 消息，shiki 代码高亮，
 * htmlPolicy=trusted 放行卡内 HTML/<style>，仅拦 script 与事件属性）
 * + 显示通路正则脚本 + 推理折叠块 + <thinking> 思维链折叠（自定义标签映射）
 * + 操作工具条（编辑/重发/分支/书签/删除/翻译/朗读）。
 */

/** 自定义 HTML 类标签：解析器收拢完整 <thinking>…</thinking> 为节点，映射到折叠组件 */
const CUSTOM_HTML_TAGS: readonly string[] = ['thinking']
const HTML_POLICY = 'trusted' as const
/** markstream 自定义组件注册 id（MessageBubble 与 ThinkingBlock 嵌套渲染共用） */
const MD_CUSTOM_ID = 'sillytavern-openchat'

setCustomComponents(MD_CUSTOM_ID, { thinking: ThinkingBlock })

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
  /** 作品内置 CSS 是否生效（风月 data-has-css 语义） */
  hasCss?: boolean
  /** 用户名（显示期 {{user}} 宏替换用） */
  userName?: string
  /** 角色名（显示期 {{char}} 宏替换用） */
  charName?: string
}>()

const emit = defineEmits<{
  edit: [index: number, content: string]
  remove: [index: number]
  resend: [index: number]
  regenerate: []
  continue: [index: number]
  branch: [index: number]
  bookmark: [index: number]
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

/* ---- 显示通路正则（正则脚本由 chat store 预加载到模块缓存） ---- */
const displayContent = computed(() => {
  const raw = applyDisplayRegex(
    props.m.content,
    props.m.role === 'user' ? REGEX_PLACEMENT.USER_INPUT : REGEX_PLACEMENT.AI_OUTPUT,
  )
  // 显示期宏替换（对齐 ST messageFormatting 的 substituteParams）：{{user}}/{{char}} 等
  // 用户消息里 {{char}} = 角色名；AI 消息以发言者名（群聊为发言人）优先
  const charForSub =
    props.m.role === 'user' ? props.charName ?? '' : props.m.name || props.charName || ''
  const subbed = substituteMacros(raw, charForSub, props.userName ?? '')
  // 卡片标记适配（<maintext>/<Status_block>/围栏尾巴）→ 围栏语言探测
  return props.m.role === 'assistant'
    ? detectFenceLanguages(adaptCardMarkup(subbed))
    : subbed
})

/** 前端界面分段：embed 段走 iframe 沙箱，文本段走 markdown（两者可在一条消息内共存） */
const embedSegs = computed(() =>
  props.m.role === 'assistant' && !props.m.pending
    ? splitEmbedSegments(displayContent.value)
    : [{ type: 'text' as const, content: displayContent.value }],
)
/** 含前端界面（iframe 卡片）的消息：气泡放开到整行宽 —— 卡 HTML 通常是
 *  .container{max-width:1200px;margin:0 auto} 这类响应式居中布局，被 78% 的
 *  文本气泡宽度约束会显得「没撑开」（实测：卡被压窄、内部换行错位）。 */
const hasEmbed = computed(() => embedSegs.value.some((s) => s.type === 'embed'))
const displayReasoning = computed(() =>
  substituteMacros(
    applyDisplayRegex(props.m.reasoning ?? '', REGEX_PLACEMENT.REASONING),
    props.m.name || props.charName || '',
    props.userName ?? '',
  ),
)

/* ---- 代码高亮主题跟随应用主题（markstream themes 顺序为 [dark, light]） ---- */
const theme = useThemeStore()
const darkTheme = computed(() =>
  theme.mode === 'dark' ? 'github-dark' : theme.mode === 'dusk' ? 'one-dark-pro' : 'github-dark',
)
const mdThemes = computed<[string, string]>(() => [darkTheme.value, 'github-light'])
const isDark = computed(() => theme.mode === 'dark' || theme.mode === 'dusk')
</script>

<template>
  <!-- id 钩子对齐风月 #customized-* 表（每条消息同 id，卡内 CSS 以 #id 选择器批量命中） -->
  <div
    class="msg"
    :class="[m.role, { hiddenMsg: m.isSystem, 'has-embed': hasEmbed }]"
    :id="m.role === 'user' ? 'customized-question' : 'customized-answer'"
  >
    <Avatar
      v-if="m.role === 'assistant'"
      id="customized-answer-avatar"
      class="mava"
      :avatar="avatar"
      :name="m.name"
    />
    <div id="customized-answer-container" class="wrap">
      <div class="meta">
        <span class="mn">{{ m.name }}</span>
        <span v-if="m.isSystem" class="hidden-badge">已隐藏 · 不进上下文</span>
        <span
          :id="m.role === 'user' ? 'customized-question-content-created-time' : 'customized-answer-content-created-time'"
          class="msg-time"
          >{{ bubbleTime(m.sendDate) }}</span
        >
      </div>

      <!-- 推理折叠块（extra.reasoning） -->
      <div v-if="m.reasoning" class="reason">
        <button id="customized-answer-content-thinking-line" class="reason-head" type="button" @click="reasonManual = !reasonOpen">
          <span class="rh-icon">✦</span>
          <span>{{ m.pending && streaming ? '深度思考中…' : '已深度思考' }}</span>
          <span class="rh-arrow" :class="{ open: reasonOpen }">▾</span>
        </button>
        <div v-show="reasonOpen" class="reason-body">
          <MarkdownRender
            class="txt md"
            :content="displayReasoning"
            mode="chat"
            final
            :themes="mdThemes"
            :is-dark="isDark"
          />
        </div>
      </div>

      <!-- 气泡：AI 走 markstream 流式渲染（trusted 策略放行卡内 HTML/<style>，
           <thinking> 块映射为折叠组件），用户保持纯文本；译文态整体替换内容 -->
      <div
        v-if="!editing"
        class="bubble"
        :class="{ err: m.error }"
        :id="m.role === 'user' ? 'customized-question-content' : 'customized-answer-content'"
        :data-has-css="hasCss ? 'true' : 'false'"
      >
        <div v-if="showTrans && translation" class="txt trans-txt">{{ translation }}</div>
        <template v-else>
          <!-- 分段渲染：embed 段（酒馆助手语义 HTML 应用）走 iframe 沙箱，
               文本段走 markdown——两类段可在一条消息内共存 -->
          <template v-for="(seg, si) in embedSegs" :key="si">
            <HtmlEmbedFrame v-if="seg.type === 'embed'" :html="seg.content" />
            <MarkdownRender
              v-else-if="m.role === 'assistant'"
              :id="si === 0 ? 'customized-answer-content-markdown' : undefined"
              class="txt md"
              :content="seg.content"
              mode="chat"
              :final="!(m.pending && streaming)"
              :html-policy="HTML_POLICY"
              :custom-id="MD_CUSTOM_ID"
              :custom-html-tags="CUSTOM_HTML_TAGS"
              :themes="mdThemes"
              :is-dark="isDark"
              :node-virtual="false"
            />
            <span v-else class="txt">{{ seg.content }}</span>
          </template>
          <span v-if="m.pending && streaming" class="caret-el">▍</span>
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

      <!-- 操作工具条：气泡下方，悬停显示（参考 ST 消息操作布局） -->
      <div
        :id="m.role === 'user' ? 'customized-message-question-actions' : 'customized-message-answer-actions'"
        class="ops"
        :class="{ user: m.role === 'user' }"
      >
        <span
          v-if="m.bookmarkLink"
          class="op bm"
          :title="`已书签 → ${m.bookmarkLink}`"
        >
          ⚑
        </span>
        <button id="customized-copy-button" class="op" type="button" :title="copied ? '已复制' : '复制'" @click="copyText">
          {{ copied ? '✓' : '⧉' }}
        </button>
        <button v-if="!editing && !groupChat" id="customized-edit-button" class="op" type="button" title="编辑" @click="startEdit">✎</button>
        <button
          v-if="m.role === 'user' && !groupChat"
          id="customized-resend-button"
          class="op"
          type="button"
          title="重发（删除此后的消息并重新生成）"
          @click="emit('resend', index)"
        >
          ↻
        </button>
        <button
          v-else-if="!groupChat && m.role === 'assistant' && isLast && !m.pending && !m.error"
          id="customized-reload-button"
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
        <button v-if="!groupChat" id="customized-branch-and-share-button" class="op" type="button" title="从此分支" @click="emit('branch', index)">⑂</button>
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
        <button v-if="!groupChat" id="customized-delete-button" class="op danger" type="button" title="删除" @click="emit('remove', index)">✕</button>
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
  /* 气泡占行宽上限 90%（原 78%）：普通消息与卡片消息统一口径。
     普通消息仍按内容自适应，只是允许占到 90%；短消息不会被迫拉满。 */
  max-width: 90%;
  min-width: 0;
}
/* 前端界面（卡片）消息：需要"撑满到上限"而不只是"允许到上限" ——
   iframe 是 replaced element，width:100% 需要一个确定的父宽，
   否则会退化成它的固有宽度（约 300px）。max-width 沿用 90%，与普通消息一致。 */
.msg.has-embed .wrap {
  flex: 1;
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

/* Markdown 内容（覆盖 markstream 默认边距，适配气泡） */
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
