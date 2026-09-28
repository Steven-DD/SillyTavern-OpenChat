<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import MidPane from '@/components/MidPane.vue'
import Avatar from '@/components/Avatar.vue'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import { useCharacterStore } from '@/stores/character'
import { useChatStore } from '@/stores/chat'
import { useLayoutStore } from '@/stores/layout'
import { useCtxMenuStore } from '@/stores/ctxmenu'
import { characterExportCard } from '@/services/tauri/plugins'
import { appendCharacterTags, renameCharacter } from '@/services/st/data'
import { sidecar } from '@/services/tauri/bridge'
import type { StCharacter } from '@/services/st/types'
import type { StGroup as StGroupData } from '@/services/st/groups'

const chars = useCharacterStore()
const chat = useChatStore()
const layout = useLayoutStore()
const ctx = useCtxMenuStore()
const router = useRouter()
const q = ref('')
const tag = ref('')

/* ---------- 群组 ---------- */
const grpOpen = ref(false)
const grpName = ref('')
const grpMembers = ref<string[]>([])
const grpCreating = ref(false)
const grpDel = ref<StGroupData | null>(null)

function openGroup(g: StGroupData) {
  const chatId = g.chat_id || g.chats?.[0]
  if (!chatId) return
  void chat.openGroupSession(g.id, chatId)
  layout.showDetail()
}

async function doCreateGroup() {
  if (grpCreating.value) return
  grpCreating.value = true
  try {
    const ok = await chat.createGroup(grpName.value.trim(), [...grpMembers.value])
    if (ok) {
      grpOpen.value = false
      grpName.value = ''
      grpMembers.value = []
    }
  } finally {
    grpCreating.value = false
  }
}

function onGroupCtx(g: StGroupData, e: MouseEvent) {
  ctx.show(
    e,
    [
      { id: 'open', label: '打开群聊' },
      { id: 'rename', label: '重命名' },
      { id: 'delete', label: '删除', danger: true },
    ],
    (id) => {
      if (id === 'open') openGroup(g)
      else if (id === 'rename') grpRename.value = { id: g.id, name: g.name }
      else if (id === 'delete') grpDel.value = g
    },
  )
}

/** 重命名态（简易行内弹窗） */
const grpRename = ref<{ id: string; name: string } | null>(null)

async function doRenameGroup() {
  const r = grpRename.value
  if (!r?.name.trim()) return
  await chat.renameGroup(r.id, r.name.trim())
  grpRename.value = null
}

async function doDeleteGroup() {
  const g = grpDel.value
  if (!g) return
  await chat.removeGroup(g.id, true)
  grpDel.value = null
}

const filtered = computed(() => {
  const kw = q.value.trim().toLowerCase()
  return chars.list.filter((c) => {
    if (tag.value && !(c.tags ?? []).includes(tag.value)) return false
    if (!kw) return true
    return (
      c.name.toLowerCase().includes(kw) ||
      (c.tags ?? []).join(' ').toLowerCase().includes(kw) ||
      c.description.toLowerCase().includes(kw)
    )
  })
})

function pick(avatar: string) {
  chars.select(avatar)
  layout.showDetail() // 窄屏单栏：进入内容页
}

/* ---------- 右键菜单：发消息 / 编辑 / 导出 / 删除 ---------- */
function startChatFor(c: StCharacter) {
  void chat.openWithCharacter(c).then(() => router.push('/chats'))
}

function editFor(c: StCharacter) {
  chars.select(c.avatar)
  layout.showDetail()
  chars.editRequested = true // 详情页 watch 后进入编辑态
}

async function exportFor(c: StCharacter) {
  if (!sidecar.isDesktop) return
  try {
    const { save } = await import('@tauri-apps/plugin-dialog')
    const p = await save({
      title: '导出角色卡',
      defaultPath: c.avatar,
      filters: [{ name: '角色卡', extensions: ['png', 'json', 'charx'] }],
    })
    if (!p) return
    await characterExportCard(c.avatar, p)
  } catch {
    /* 导出失败静默（与列表级操作一致，不打断浏览） */
  }
}

function onCtx(c: StCharacter, e: MouseEvent) {
  ctx.show(
    e,
    [
      { id: 'chat', label: '发消息' },
      { id: 'edit', label: '编辑' },
      { id: 'rename', label: '重命名' },
      { id: 'export', label: '导出' },
      { id: 'delete', label: '删除', danger: true },
    ],
    (id) => {
      if (id === 'chat') startChatFor(c)
      else if (id === 'edit') editFor(c)
      else if (id === 'rename') charRename.value = { avatar: c.avatar, name: c.name }
      else if (id === 'export') void exportFor(c)
      else if (id === 'delete') delTarget.value = c
    },
  )
}

/** 角色卡重命名（服务端连聊天目录一起迁移） */
const charRename = ref<{ avatar: string; name: string } | null>(null)
const renameBusy = ref(false)

async function doRenameCharacter() {
  const r = charRename.value
  if (!r?.name.trim() || renameBusy.value) return
  renameBusy.value = true
  try {
    await renameCharacter(r.avatar, r.name.trim())
    charRename.value = null
    await chars.load(true)
    msgOk.value = true
    msg.value = '已重命名（聊天记录一并迁移）'
  } catch (e) {
    msgOk.value = false
    msg.value = e instanceof Error ? e.message : String(e)
  } finally {
    renameBusy.value = false
  }
}

/* ---- 批量操作：多选 → 批量打标签 / 删除 / 导出 ---- */
const multiMode = ref(false)
const checked = ref<string[]>([])
const batchTag = ref('')
const batchBusy = ref(false)
const batchDelOpen = ref(false)

function toggleMulti(): void {
  multiMode.value = !multiMode.value
  checked.value = []
}

function toggleCheck(avatar: string): void {
  const i = checked.value.indexOf(avatar)
  if (i >= 0) checked.value.splice(i, 1)
  else checked.value.push(avatar)
}

async function doBatchTag(): Promise<void> {
  const t = batchTag.value.trim()
  if (!t || batchBusy.value || !checked.value.length) return
  batchBusy.value = true
  let done = 0
  try {
    for (const a of checked.value) {
      try {
        await appendCharacterTags(a, t.split(',').map((x) => x.trim()).filter(Boolean))
        done++
      } catch { /* 单卡失败跳过，不中断批量 */ }
    }
    await chars.load(true)
    msgOk.value = true
    msg.value = `已为 ${done}/${checked.value.length} 张卡追加标签「${t}」`
    checked.value = []
  } finally {
    batchBusy.value = false
  }
}

async function doBatchDelete(): Promise<void> {
  if (batchBusy.value || !checked.value.length) return
  batchBusy.value = true
  let done = 0
  try {
    for (const a of checked.value) {
      try {
        await chars.deleteCard(a)
        done++
      } catch { /* 同上 */ }
    }
    checked.value = []
    msgOk.value = true
    msg.value = `已删除 ${done} 张卡`
  } finally {
    batchBusy.value = false
  }
}

async function doBatchExport(): Promise<void> {
  if (batchBusy.value || !checked.value.length) return
  batchBusy.value = true
  let done = 0
  try {
    for (const a of checked.value) {
      const c = chars.list.find((x) => x.avatar === a)
      if (!c) continue
      try {
        await exportFor(c)
        done++
      } catch { /* 同上 */ }
    }
    msgOk.value = true
    msg.value = `已导出 ${done} 张卡`
  } finally {
    batchBusy.value = false
  }
}

/** 待删除的角色卡（非 null = 弹确认窗） */
const delTarget = ref<StCharacter | null>(null)

async function doDelete() {
  const c = delTarget.value
  if (!c) return
  await chars.deleteCard(c.avatar)
  delTarget.value = null
}

/* ---------- 新建 / 导入 ---------- */
const panelOpen = ref(false)
const newName = ref('')
const msg = ref('')
const msgOk = ref(false)
const fileInput = ref<HTMLInputElement | null>(null)

function togglePanel() {
  panelOpen.value = !panelOpen.value
  msg.value = ''
  newName.value = ''
}

async function doCreate() {
  const name = newName.value.trim()
  if (!name) return
  msg.value = ''
  try {
    await chars.createCard(name)
    panelOpen.value = false
    msgOk.value = true
    msg.value = `已创建「${name}」`
    layout.showDetail()
  } catch (e) {
    msgOk.value = false
    msg.value = e instanceof Error ? e.message : String(e)
  }
}

function pickFile() {
  fileInput.value?.click()
}

async function onFile(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = '' // 允许重复导入同一文件
  if (!file) return
  msg.value = ''
  try {
    const book = await chars.importCard(file)
    panelOpen.value = false
    msgOk.value = true
    msg.value = book
      ? `已导入角色卡，并自动导入内嵌世界书「${book}」`
      : '角色卡已导入'
    layout.showDetail()
  } catch (e) {
    msgOk.value = false
    msg.value = e instanceof Error ? e.message : String(e)
  }
}

onMounted(() => {
  void chars.load()
  void chat.loadSessions() // 会顺带拉取群组列表（groupsList）
})
</script>

<template>
  <MidPane title="角色卡" :subtitle="`${chars.list.length} 张`">
    <template #head>
      <input v-model="q" class="search" type="text" placeholder="搜索名称 / 描述 / 标签" />
      <button class="add" @click="togglePanel">＋ 新建角色卡</button>
      <button class="add" :class="{ on: multiMode }" @click="toggleMulti">多选</button>
      <div v-if="panelOpen" class="panel">
        <input
          v-model="newName"
          class="search"
          type="text"
          placeholder="角色名（必填）"
          @keyup.enter="doCreate"
        />
        <div class="panel-acts">
          <button class="pk primary" :disabled="chars.saving || !newName.trim()" @click="doCreate">
            {{ chars.saving ? '创建中…' : '新建空白卡' }}
          </button>
          <button class="pk" :disabled="chars.saving" @click="pickFile">
            {{ chars.saving ? '导入中…' : '导入文件…' }}
          </button>
        </div>
        <p class="panel-hint">导入支持 ST 角色卡 JSON / PNG / charx；内嵌世界书会自动导入并绑定</p>
        <input ref="fileInput" type="file" accept=".json,.png,.charx,.yaml,.yml,.byaf" hidden @change="onFile" />
      </div>
      <p v-if="msg" class="op-msg" :class="{ err: !msgOk }">{{ msg }}</p>
      <div v-if="chars.allTags.length" class="tags">
        <button class="tg" :class="{ on: !tag }" @click="tag = ''">全部</button>
        <button
          v-for="t in chars.allTags"
          :key="t"
          class="tg"
          :class="{ on: tag === t }"
          @click="tag = tag === t ? '' : t"
        >
          {{ t }}
        </button>
      </div>
    </template>

    <!-- 批量操作工具条（多选模式） -->
    <div v-if="multiMode" class="panel">
      <p class="panel-hint">已选 {{ checked.length }} 张卡</p>
      <div class="qr-row">
        <input v-model="batchTag" class="search grow" placeholder="追加标签（逗号分隔多个）" />
        <button class="pk" :disabled="batchBusy || !checked.length || !batchTag.trim()" @click="doBatchTag">打标签</button>
      </div>
      <div class="panel-acts">
        <button class="pk" :disabled="batchBusy || !checked.length" @click="doBatchExport">批量导出</button>
        <button class="pk" :disabled="batchBusy || !checked.length" @click="batchDelOpen = true">批量删除</button>
      </div>
    </div>

    <div v-if="chars.loading && !chars.list.length" class="empty-hint">加载角色卡中…</div>
    <div v-else-if="chars.error" class="empty-hint err">{{ chars.error }}</div>
    <div v-else-if="!filtered.length" class="empty-hint">没有匹配的角色卡</div>

    <button
      v-for="c in filtered"
      :key="c.avatar"
      class="list-row"
      :class="{ on: chars.currentAvatar === c.avatar }"
      @click="multiMode ? toggleCheck(c.avatar) : pick(c.avatar)"
      @contextmenu.prevent="multiMode ? undefined : onCtx(c, $event)"
    >
      <input
        v-if="multiMode"
        type="checkbox"
        class="batch-ck"
        :checked="checked.includes(c.avatar)"
        @click.stop
        @change="toggleCheck(c.avatar)"
      />
      <Avatar class="ava" :avatar="c.avatar" :name="c.name" />
      <div class="col">
        <div class="row1">
          <span class="nm">{{ c.name }}</span>
          <span v-if="c.fav" class="fav" title="收藏">
            <svg viewBox="0 0 24 24" width="11" height="11" fill="currentColor">
              <path
                d="M12 3.6l2.7 5.5 6 .9-4.3 4.2 1 6-5.4-2.9-5.4 2.9 1-6L3.3 10l6-.9z"
              />
            </svg>
          </span>
        </div>
        <div class="desc">{{ c.creatorcomment || c.description || '—' }}</div>
      </div>
    </button>

    <!-- 删除角色卡确认（连带会话记录） -->
    <ConfirmDialog
      :open="!!delTarget"
      title="删除角色卡"
      :message="delTarget ? `删除「${delTarget.name}」？\n其全部会话记录将一并删除，不可恢复。` : ''"
      confirm-text="删除"
      :busy="chars.saving"
      @confirm="doDelete"
      @cancel="delTarget = null"
    />

    <!-- ── 群组 ── -->
    <div class="grp-head">
      <span class="grp-t">群组（{{ chat.groupsList.length }}）</span>
      <span class="grow" />
      <button class="pk" :disabled="grpCreating" @click="grpOpen = !grpOpen">＋ 新建群组</button>
    </div>
    <div v-if="grpOpen" class="panel">
      <input v-model="grpName" class="search" type="text" placeholder="群名称（必填）" />
      <div class="mem-list">
        <label v-for="c in chars.list" :key="c.avatar" class="mem">
          <input
            type="checkbox"
            :checked="grpMembers.includes(c.avatar)"
            @change="($event.target as HTMLInputElement).checked
              ? grpMembers.push(c.avatar)
              : (grpMembers = grpMembers.filter((x) => x !== c.avatar))"
          />
          {{ c.name }}
        </label>
      </div>
      <div class="panel-acts">
        <button class="pk primary" :disabled="grpCreating || !grpName.trim() || grpMembers.length < 1" @click="doCreateGroup">
          {{ grpCreating ? '创建中…' : `创建（${grpMembers.length} 成员）` }}
        </button>
      </div>
      <p class="panel-hint">生成模式/激活策略创建后可在会话面板切换；成员 talkativeness 取角色卡值</p>
    </div>
    <div v-if="!chat.groupsList.length" class="empty-hint">暂无群组</div>
    <button
      v-for="g in chat.groupsList"
      :key="g.id"
      class="list-row"
      :class="{ on: chat.group?.id === g.id }"
      @click="openGroup(g)"
      @contextmenu.prevent="onGroupCtx(g, $event)"
    >
      <Avatar class="ava" :name="g.name" />
      <div class="col">
        <div class="row1"><span class="nm">{{ g.name }}</span></div>
        <div class="desc">{{ g.members.length }} 名成员 · {{ g.chats?.length ?? 0 }} 个会话</div>
      </div>
    </button>

    <!-- 删除群组确认 -->
    <ConfirmDialog
      :open="!!grpDel"
      title="删除群组"
      :message="grpDel ? `删除群组「${grpDel.name}」？\n其群聊记录将一并删除，不可恢复。` : ''"
      confirm-text="删除"
      @confirm="doDeleteGroup"
      @cancel="grpDel = null"
    />

    <!-- 批量删除确认 -->
    <ConfirmDialog
      :open="batchDelOpen"
      title="批量删除角色卡"
      :message="`删除已选的 ${checked.length} 张卡？\n其全部会话记录将一并删除，不可恢复。`"
      confirm-text="删除"
      :busy="batchBusy"
      @confirm="doBatchDelete().then(() => (batchDelOpen = false))"
      @cancel="batchDelOpen = false"
    />

    <!-- 角色卡重命名 -->
    <div v-if="charRename" class="panel" style="z-index: 30">
      <input
        v-model="charRename.name"
        class="search"
        type="text"
        maxlength="60"
        @keyup.enter="doRenameCharacter"
      />
      <div class="panel-acts">
        <button class="pk primary" :disabled="renameBusy || !charRename.name.trim()" @click="doRenameCharacter">
          {{ renameBusy ? '重命名中…' : '重命名' }}
        </button>
        <button class="pk" @click="charRename = null">取消</button>
      </div>
      <p class="panel-hint">聊天记录由 ST 服务端一并迁移到新名字</p>
    </div>

    <!-- 群组重命名 -->
    <div v-if="grpRename" class="panel" style="z-index: 30">
      <input
        v-model="grpRename.name"
        class="search"
        type="text"
        maxlength="60"
        @keyup.enter="doRenameGroup"
      />
      <div class="panel-acts">
        <button class="pk primary" :disabled="!grpRename.name.trim()" @click="doRenameGroup">重命名</button>
        <button class="pk" @click="grpRename = null">取消</button>
      </div>
    </div>
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
.panel {
  margin-top: 6px;
  padding: 8px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
}
.panel .search {
  margin-top: 0;
}
.panel-acts {
  display: flex;
  gap: 6px;
  margin-top: 6px;
}
.pk {
  flex: 1;
  padding: 6px 0;
  font-size: 11px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-panel);
  color: var(--c-text-2);
}
.pk:hover:not(:disabled) {
  border-color: var(--p-400);
  color: var(--p-600);
}
.pk.primary {
  background: var(--p-500);
  border-color: var(--p-500);
  color: var(--c-on-primary);
}
.pk:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.panel-hint {
  margin-top: 6px;
  font-size: 10px;
  line-height: 1.6;
  color: var(--c-text-3);
}
.op-msg {
  margin: 6px 4px 0;
  font-size: 11px;
  color: var(--s-success);
  word-break: break-all;
}
.op-msg.err {
  color: var(--s-error);
}
/* 群组区 */
.grp-head {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 14px 4px 4px;
}
.grp-t {
  font-size: 10px;
  color: var(--c-text-3);
}
.grow {
  flex: 1;
  min-width: 0;
}
.mem-list {
  max-height: 180px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin: 6px 0;
}
.mem {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--c-text-2);
  padding: 3px 4px;
  border-radius: var(--radius-sm);
}
.mem:hover {
  background: var(--c-panel);
}
.batch-ck {
  flex: none;
  width: 15px;
  height: 15px;
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
.tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 6px;
}
.tg {
  font-size: 10px;
  padding: 1px 7px;
  border-radius: 9px;
  background: var(--c-panel);
  color: var(--c-text-2);
}
.tg.on {
  background: var(--p-500);
  color: var(--c-on-primary);
}
.empty-hint {
  margin: 10px 4px;
  padding: 16px 10px;
  border: 1px dashed var(--c-border);
  border-radius: var(--radius-md);
  text-align: center;
  font-size: 12px;
  color: var(--c-text-3);
}
.empty-hint.err {
  border-color: var(--s-error);
  color: var(--s-error);
}

.list-row {
  display: flex;
  gap: 9px;
  width: 100%;
  padding: 9px 8px;
  text-align: left;
  border-radius: var(--radius-md);
  transition: background 0.15s;
}
.list-row:hover {
  background: var(--c-panel);
}
.list-row.on {
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
.row1 {
  display: flex;
  align-items: center;
  gap: 5px;
}
.nm {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.fav {
  flex: none;
  font-size: 10px;
  color: var(--s-warning);
}
.desc {
  font-size: 11px;
  color: var(--c-text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
