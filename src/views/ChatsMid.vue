<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch, type ComponentPublicInstance } from 'vue'
import MidPane from '@/components/MidPane.vue'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import { useChatStore, type SessionItem } from '@/stores/chat'
import { useCharacterStore } from '@/stores/character'
import { useLayoutStore } from '@/stores/layout'
import { useCtxMenuStore } from '@/stores/ctxmenu'
import { chatExportFile } from '@/services/tauri/plugins'
import { searchChats, type StChatSearchHit } from '@/services/st/data'
import { parseGroupSessionKey } from '@/services/st/group-orchestrate'
import Avatar from '@/components/Avatar.vue'
import { shortTime } from '@/utils/time'

const chat = useChatStore()
const layout = useLayoutStore()
const chars = useCharacterStore()
const ctx = useCtxMenuStore()
const q = ref('')

/** 待删除的会话（非 null = 弹确认窗） */
const delTarget = ref<SessionItem | null>(null)
const deleting = ref(false)
/** 处于「行内重命名」状态的会话 key */
const editingKey = ref('')
const editName = ref('')
const editInput = ref<HTMLInputElement | null>(null)
/** v-for 内模板 ref 必须用函数形式（字符串 ref 会被收集成数组） */
function setEditInput(el: Element | ComponentPublicInstance | null) {
  editInput.value = el instanceof HTMLInputElement ? el : null
}

const filtered = computed(() => {
  const kw = q.value.trim().toLowerCase()
  if (!kw) return chat.sessions
  return chat.sessions.filter(
    (s) =>
      s.characterName.toLowerCase().includes(kw) ||
      s.displayName.toLowerCase().includes(kw) ||
      s.preview.toLowerCase().includes(kw) ||
      s.fileId.toLowerCase().includes(kw),
  )
})

/* ---- 消息正文全文搜索（ST /api/chats/search，按角色目录检索） ---- */
const hits = ref<StChatSearchHit[]>([])
const searching = ref(false)
/** 搜索范围：当前会话角色（无则用联系人选中角色） */
const searchAvatar = ref('')
let searchTimer: number | undefined

watch(q, (v) => {
  window.clearTimeout(searchTimer)
  hits.value = []
  const kw = v.trim()
  const avatar = chat.currentAvatar || chars.currentAvatar
  if (kw.length < 2 || !avatar) return
  searchTimer = window.setTimeout(async () => {
    searching.value = true
    searchAvatar.value = avatar
    try {
      hits.value = await searchChats(kw, avatar)
    } catch {
      hits.value = [] // 搜索失败不打断本地过滤
    } finally {
      searching.value = false
    }
  }, 400)
})

function openHit(h: StChatSearchHit) {
  const s = chat.sessions.find((x) => x.avatar === searchAvatar.value && x.fileId === h.file_name)
  if (s) open(s)
  else {
    void chat.openSession(searchAvatar.value, h.file_name)
    layout.showDetail()
  }
}

/* ---- 新建会话：有目标角色才可用（当前会话角色，或联系人面板选中的角色） ---- */
const canNew = computed(() => !!chat.currentCharacter)

function startNew() {
  if (!canNew.value) return
  chat.newChatOpen = true // 弹窗挂在中栏/主栏共用的 store 开关上（ChatsMain 渲染）
  layout.showDetail() // 窄屏单栏：跳到聊天内容页看弹窗
}

function busy(s: SessionItem): boolean {
  return editingKey.value === s.key
}

function open(s: SessionItem) {
  if (busy(s)) return // 确认/重命名态下点击整行不触发打开
  if (s.isGroup) {
    const parsed = parseGroupSessionKey(s.key)
    if (parsed) void chat.openGroupSession(parsed.groupId, parsed.chatId)
  } else {
    void chat.openSession(s.avatar, s.fileId)
  }
  layout.showDetail() // 窄屏单栏：进入聊天内容页
}

/* ---- 右键菜单 ---- */
function onCtx(s: SessionItem, e: MouseEvent) {
  const pinned = chat.isPinned(s.key)
  ctx.show(
    e,
    [
      { id: 'pin', label: pinned ? '取消置顶' : '置顶' },
      { id: 'rename', label: '重命名' },
      { id: 'export', label: '导出 .jsonl' },
      { id: 'delete', label: '删除', danger: true },
    ],
    (id) => {
      if (id === 'pin') chat.togglePin(s.avatar, s.fileId)
      else if (id === 'rename') void startRename(s)
      else if (id === 'export') void exportChat(s)
      else if (id === 'delete') delTarget.value = s
    },
  )
}

/** 导出会话原版 jsonl（保存对话框选位置） */
async function exportChat(s: SessionItem): Promise<void> {
  const { save } = await import('@tauri-apps/plugin-dialog')
  const p = await save({
    title: '导出会话',
    defaultPath: `${s.fileId}.jsonl`,
    filters: [{ name: '会话文件', extensions: ['jsonl'] }],
  })
  if (!p) return
  await chatExportFile(s.avatar, s.fileId, p)
}

/* ---- 行内重命名 ---- */
async function startRename(s: SessionItem) {
  editingKey.value = s.key
  editName.value = chat.aliasOf(s.key) ?? s.fileId
  await nextTick()
  editInput.value?.focus()
  editInput.value?.select()
}

function cancelRename() {
  editingKey.value = ''
}

async function confirmRename(s: SessionItem) {
  if (editingKey.value !== s.key) return // Enter 后跟 blur 的双触发防抖
  editingKey.value = ''
  const name = editName.value.trim()
  if (!name || name === s.fileId) return
  await chat.renameSession(s.avatar, s.fileId, name)
}

async function doDelete() {
  const s = delTarget.value
  if (!s) return
  deleting.value = true
  const ok = await chat.deleteSession(s.avatar, s.fileId)
  deleting.value = false
  if (ok) delTarget.value = null
}

onMounted(async () => {
  await chars.load()
  await chat.loadSessions()
  void chat.loadStUserName()
  void chat.refreshConn()
  void chat.loadExtensionData()
})
</script>

<template>
  <MidPane title="会话" :subtitle="`${chat.sessions.length} 个会话`">
    <template #head>
      <input v-model="q" class="search" type="text" placeholder="搜索会话" />
      <button
        class="add"
        :disabled="!canNew"
        :title="canNew ? `给 ${chat.currentCharacter?.name} 新建会话` : '先打开一个会话或在联系人里选择角色'"
        @click="startNew"
      >
        ＋ 新建会话
      </button>
    </template>

    <div v-if="chat.sessionsLoading && !chat.sessions.length" class="empty-hint">加载会话中…</div>
    <div v-else-if="!filtered.length" class="empty-hint">
      {{ chat.sessions.length ? '没有匹配的会话' : '暂无会话' }}
    </div>

    <div
      v-for="s in filtered"
      :key="s.key"
      class="sess"
      :class="{ on: chat.sessionKeyOf === s.key }"
      role="button"
      tabindex="0"
      @click="open(s)"
      @keydown.enter="open(s)"
      @contextmenu.prevent="onCtx(s, $event)"
    >
      <!-- 行内重命名态 -->
      <template v-if="editingKey === s.key">
        <Avatar class="ava" :avatar="s.avatar" :name="s.displayName" />
        <div class="col" @click.stop>
          <input
            :ref="setEditInput"
            v-model="editName"
            class="rename"
            type="text"
            maxlength="60"
            @keydown.enter.prevent="confirmRename(s)"
            @keydown.esc.prevent="cancelRename"
            @blur="confirmRename(s)"
          />
          <div class="rename-tip">回车确认 · Esc 取消</div>
        </div>
      </template>

      <!-- 正常列表项 -->
      <template v-else>
        <Avatar class="ava" :avatar="s.avatar" :name="s.displayName" />
        <div class="col">
          <div class="row1">
            <span class="nm" :class="{ aliased: !!chat.aliasOf(s.key) }">{{ s.displayName }}</span>
            <span class="tm">{{ shortTime(s.lastMes) }}</span>
          </div>
          <div class="row2">
            <span class="pv">{{ s.preview || '—' }}</span>
            <span v-if="s.personaId" class="chip" title="本会话锁定人设">人设</span>
            <span v-if="s.genCustom" class="chip" title="本会话自定义生成参数">参数</span>
            <span v-if="s.messageCount > 1" class="cnt">{{ s.messageCount }}</span>
          </div>
        </div>
      </template>
    </div>

    <!-- 消息正文全文搜索（≥2 字符触发，范围为当前角色） -->
    <template v-if="q.trim().length >= 2">
      <div class="fts-t">
        {{ searching ? '搜索消息正文…' : `消息正文命中 ${hits.length} 条` }}
      </div>
      <div
        v-for="h in hits"
        :key="h.file_name"
        class="sess"
        role="button"
        tabindex="0"
        @click="openHit(h)"
        @keydown.enter="openHit(h)"
      >
        <div class="col">
          <div class="row1"><span class="nm">{{ h.file_name }}</span></div>
          <div class="row2"><span class="pv">{{ h.preview_message || '—' }}</span></div>
        </div>
      </div>
      <div v-if="!searching && !hits.length" class="fts-none">
        当前角色的会话中没有包含该词的消息
      </div>
    </template>

    <!-- 删除会话确认 -->
    <ConfirmDialog
      :open="!!delTarget"
      title="删除会话"
      :message="delTarget ? `删除「${delTarget.displayName}」的会话？\n删除后不可恢复。` : ''"
      confirm-text="删除"
      :busy="deleting"
      @confirm="doDelete"
      @cancel="delTarget = null"
    />
  </MidPane>
</template>

<style scoped>
.add {
  width: 100%;
  margin-top: 6px;
  padding: 6px 9px;
  font-size: 12px;
  border: 1px dashed var(--c-border);
  border-radius: var(--radius-md);
  color: var(--p-600);
  background: transparent;
}
.add:disabled {
  color: var(--c-text-3);
  border-style: solid;
  cursor: not-allowed;
}

.search {
  width: 100%;
  margin-top: 8px;
  padding: 6px 9px;
  font-size: 12px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
  outline: none;
}
.search:focus {
  border-color: var(--p-400);
}
.empty-hint {
  margin: 10px 4px;
  padding: 16px 10px;
  border: 1px dashed var(--c-border);
  border-radius: var(--radius-md);
  text-align: center;
  font-size: 12px;
  line-height: 1.7;
  color: var(--c-text-3);
}

.sess {
  display: flex;
  gap: 9px;
  width: 100%;
  padding: 9px 8px;
  text-align: left;
  border-radius: var(--radius-md);
  transition: background 0.15s;
  cursor: pointer;
}
.sess:hover {
  background: var(--c-panel);
}
.sess.on {
  background: var(--p-50);
}
.ava {
  width: 36px;
  height: 36px;
  flex: none;
  border-radius: 50%;
  object-fit: cover;
  background: var(--c-panel);
}
.col {
  flex: 1;
  min-width: 0;
}
.row1,
.row2 {
  display: flex;
  align-items: center;
  gap: 6px;
}
.nm {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--c-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 用户重命名过的会话：别名以常规字重呈现，与角色名区分 */
.nm.aliased {
  font-weight: 500;
  color: var(--c-text-2);
}
.tm {
  flex: none;
  font-size: 10px;
  color: var(--c-text-3);
}
.pv {
  flex: 1;
  min-width: 0;
  font-size: 11px;
  color: var(--c-text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cnt {
  flex: none;
  min-width: 15px;
  padding: 0 4px;
  font-size: 10px;
  line-height: 15px;
  text-align: center;
  border-radius: 8px;
  background: var(--c-border);
  color: var(--c-text-2);
}
/* 配置摘要徽标：人设锁 / 自定义参数 */
.chip {
  flex: none;
  padding: 0 5px;
  font-size: 9px;
  line-height: 15px;
  border-radius: 8px;
  background: var(--c-panel);
  color: var(--p-600);
  border: 1px solid var(--p-400);
  opacity: 0.85;
}

/* 行内重命名 */
.rename {
  width: 100%;
  padding: 4px 7px;
  font-size: 12px;
  border: 1px solid var(--p-400);
  border-radius: var(--radius-sm);
  background: var(--c-bg);
  color: var(--c-text);
  outline: none;
}
.rename-tip {
  margin-top: 3px;
  font-size: 10px;
  color: var(--c-text-3);
}

/* 消息正文全文搜索 */
.fts-t {
  margin: 10px 4px 2px;
  font-size: 10px;
  color: var(--c-text-3);
}
.fts-none {
  margin: 4px;
  padding: 8px 10px;
  font-size: 11px;
  color: var(--c-text-3);
}
</style>
