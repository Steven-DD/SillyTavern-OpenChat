<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import MidPane from '@/components/MidPane.vue'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import { useWorldsStore } from '@/stores/worlds'
import { useLayoutStore } from '@/stores/layout'
import { useCtxMenuStore } from '@/stores/ctxmenu'
import type { StWorldListItem } from '@/services/st/types'

const worlds = useWorldsStore()
const layout = useLayoutStore()
const ctx = useCtxMenuStore()
const q = ref('')

const filtered = computed(() => {
  const kw = q.value.trim().toLowerCase()
  if (!kw) return worlds.list
  return worlds.list.filter(
    (w) => w.name.toLowerCase().includes(kw) || w.file_id.toLowerCase().includes(kw),
  )
})

function pick(fileId: string) {
  worlds.select(fileId)
  layout.showDetail() // 窄屏单栏：进入内容页
}

/* ---------- 右键菜单：删除 ---------- */
function onCtx(w: StWorldListItem, e: MouseEvent) {
  ctx.show(
    e,
    [{ id: 'delete', label: '删除', danger: true }],
    (id) => {
      if (id === 'delete') delTarget.value = w
    },
  )
}

/** 待删除的世界书（非 null = 弹确认窗） */
const delTarget = ref<StWorldListItem | null>(null)

async function doDelete() {
  const w = delTarget.value
  if (!w) return
  try {
    await worlds.remove(w.file_id)
    delTarget.value = null
  } catch {
    /* 删除失败保留弹窗，store.error 会提示 */
  }
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
    await worlds.create(name)
    panelOpen.value = false
    msgOk.value = true
    msg.value = `已创建世界书「${name}」`
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
    const name = await worlds.importFile(file)
    panelOpen.value = false
    msgOk.value = true
    msg.value = `已导入世界书「${name}」`
    layout.showDetail()
  } catch (e) {
    msgOk.value = false
    msg.value = e instanceof Error ? e.message : String(e)
  }
}

onMounted(() => {
  void worlds.load()
})
</script>

<template>
  <MidPane title="世界书" :subtitle="`${worlds.list.length} 本`">
    <template #head>
      <input v-model="q" class="search" type="text" placeholder="搜索世界书" />
      <button class="add" @click="togglePanel">＋ 新建世界书</button>
      <div v-if="panelOpen" class="panel">
        <input
          v-model="newName"
          class="search"
          type="text"
          placeholder="世界书名称（必填）"
          @keyup.enter="doCreate"
        />
        <div class="panel-acts">
          <button class="pk primary" :disabled="worlds.mutating || !newName.trim()" @click="doCreate">
            {{ worlds.mutating ? '创建中…' : '新建空白书' }}
          </button>
          <button class="pk" :disabled="worlds.mutating" @click="pickFile">
            {{ worlds.mutating ? '导入中…' : '导入文件…' }}
          </button>
        </div>
        <p class="panel-hint">导入支持 ST 世界书 JSON（顶层含 entries）</p>
        <input ref="fileInput" type="file" accept=".json" hidden @change="onFile" />
      </div>
      <p v-if="msg" class="op-msg" :class="{ err: !msgOk }">{{ msg }}</p>
    </template>

    <div v-if="worlds.loading && !worlds.list.length" class="empty-hint">加载世界书中…</div>
    <div v-else-if="worlds.error" class="empty-hint err">{{ worlds.error }}</div>
    <div v-else-if="!filtered.length" class="empty-hint">
      {{ worlds.list.length ? '没有匹配的世界书' : '还没有世界书，点上方新建或导入' }}
    </div>

    <button
      v-for="w in filtered"
      :key="w.file_id"
      class="list-row"
      :class="{ on: worlds.currentId === w.file_id }"
      @click="pick(w.file_id)"
      @contextmenu.prevent="onCtx(w, $event)"
    >
      <svg
        class="ic"
        viewBox="0 0 24 24"
        width="16"
        height="16"
        fill="none"
        stroke="currentColor"
        stroke-width="1.8"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2zM22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z" />
      </svg>
      <span class="col">
        <span class="nm">{{ w.name }}</span>
        <span class="ds">{{ w.file_id }}</span>
      </span>
    </button>

    <!-- 删除世界书确认 -->
    <ConfirmDialog
      :open="!!delTarget"
      title="删除世界书"
      :message="delTarget ? `删除「${delTarget.name}」？\n删除后不可恢复。` : ''"
      confirm-text="删除"
      :busy="worlds.mutating"
      @confirm="doDelete"
      @cancel="delTarget = null"
    />
  </MidPane>
</template>

<style scoped>
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
  align-items: center;
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
.ic {
  flex: none;
  color: var(--c-text-3);
}
.list-row.on .ic {
  color: var(--p-600);
}
.col {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.nm {
  font-size: 13px;
  font-weight: 600;
  color: var(--c-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ds {
  font-size: 10px;
  color: var(--c-text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
