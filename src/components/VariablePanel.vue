<script setup lang="ts">
/**
 * 风月变量区块：嵌在会话面板「参数」Tab 底部（无悬浮球）。
 *
 * 数据源 = chat store 的 chatVariables（chat_metadata.variables，宏/STscript/正则
 * 三条通路共用的同一份存储）。原始值可内联编辑，变更走 chatVarStore()
 * （自带 400ms 防抖落盘）。id 钩子对齐风月 #customized-variable-panel-* 速查表，
 * 供卡内 CSS 定制。
 */
import { computed, ref } from 'vue'
import { useChatStore } from '@/stores/chat'

const chat = useChatStore()

/** 刷新：强制重渲染（变量可能被生成/脚本异步改写） */
const tick = ref(0)

/* ---- 变量树：嵌套对象按点号路径拍平 ---- */
interface VarRow {
  path: string
  value: unknown
  primitive: boolean
}
const rows = computed<VarRow[]>(() => {
  void tick.value
  const out: VarRow[] = []
  const walk = (obj: Record<string, unknown>, prefix: string): void => {
    for (const [k, v] of Object.entries(obj)) {
      const path = prefix ? `${prefix}.${k}` : k
      if (v !== null && typeof v === 'object') {
        if (Object.keys(v as Record<string, unknown>).length) walk(v as Record<string, unknown>, path)
        else out.push({ path, value: v, primitive: false })
      } else {
        out.push({ path, value: v, primitive: typeof v !== 'object' })
      }
    }
  }
  walk(chat.chatVariables, '')
  return out
})

function fmt(v: unknown): string {
  return typeof v === 'string' ? v : JSON.stringify(v) ?? 'null'
}

/* ---- 内联编辑（仅原始类型） ---- */
const editingPath = ref('')
const editValue = ref('')

function startEdit(row: VarRow): void {
  editingPath.value = row.path
  editValue.value = fmt(row.value)
}
function commitEdit(path: string): void {
  const raw = editValue.value
  if (editingPath.value !== path) return
  editingPath.value = ''
  // 数值/布尔按类型解析，其余按字符串；空输入 = 删除变量
  const vars = chat.chatVarStore()
  if (raw === '') {
    vars.delete(path)
    return
  }
  let value: unknown = raw
  if (raw === 'true') value = true
  else if (raw === 'false') value = false
  else if (raw !== '' && !Number.isNaN(Number(raw)) && /^-?\d+(\.\d+)?$/.test(raw.trim())) value = Number(raw)
  vars.set(path, value)
}

function removeVar(path: string): void {
  chat.chatVarStore().delete(path)
}

function refresh(): void {
  tick.value++
}
</script>

<template>
  <div id="customized-variable-panel" class="vpanel">
    <div id="customized-variable-panel-floating-panel-header" class="head">
      <span id="customized-variable-panel-floating-panel-header-title" class="tt">变量</span>
      <span id="customized-variable-panel-floating-panel-header-actions" class="acts">
        <button
          id="customized-variable-panel-floating-panel-header-actions-refresh"
          class="a"
          type="button"
          title="刷新"
          @click="refresh"
        >
          ⟳
        </button>
      </span>
    </div>
    <div id="customized-variable-panel-floating-panel-content" class="body">
      <div v-if="!rows.length" class="empty">暂无变量（STscript /setvar 或卡内逻辑写入后显示）</div>
      <template v-else>
        <div
          v-for="r in rows"
          :key="`${tick}:${r.path}`"
          id="customized-variable-panel-floating-panel-content-template"
          class="row"
        >
          <span class="k" :title="r.path">{{ r.path }}</span>
          <template v-if="r.primitive">
            <input
              v-if="editingPath === r.path"
              v-model="editValue"
              class="v edit"
              spellcheck="false"
              @keydown.enter="($event.target as HTMLInputElement).blur()"
              @blur="commitEdit(r.path)"
              @keydown.esc="editingPath = ''"
            />
            <button v-else class="v val" type="button" title="点击编辑" @click="startEdit(r)">
              {{ fmt(r.value) || '""' }}
            </button>
          </template>
          <span v-else class="v obj" :title="fmt(r.value)">{{ fmt(r.value) }}</span>
          <button class="del" type="button" title="删除变量" @click="removeVar(r.path)">✕</button>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.vpanel {
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
  overflow: hidden;
}
.head {
  display: flex;
  align-items: center;
  padding: 6px 10px;
  border-bottom: 1px solid var(--c-border);
  background: var(--c-panel);
}
.tt {
  font-size: 11px;
  font-weight: 600;
  color: var(--c-text-2);
}
.acts {
  margin-left: auto;
  display: flex;
  gap: 4px;
}
.a {
  width: 20px;
  height: 20px;
  display: grid;
  place-items: center;
  border-radius: var(--radius-sm);
  color: var(--c-text-3);
  font-size: 12px;
}
.a:hover {
  background: var(--c-panel-2, var(--c-bg));
  color: var(--c-text);
}
.body {
  max-height: 240px;
  overflow-y: auto;
  padding: 5px 8px;
}
.empty {
  padding: 12px 6px;
  font-size: 11px;
  color: var(--c-text-3);
  text-align: center;
}
.row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 2px;
  font-size: 11px;
  border-radius: var(--radius-sm);
}
.row:hover {
  background: var(--c-panel);
}
.k {
  flex: none;
  max-width: 45%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--c-text-2);
  font-family: var(--font-mono, ui-monospace, monospace);
}
.v {
  flex: 1;
  min-width: 0;
  text-align: left;
  font-family: var(--font-mono, ui-monospace, monospace);
}
.v.val {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--c-text);
  padding: 1px 4px;
  border-radius: var(--radius-sm);
}
.v.val:hover {
  background: color-mix(in srgb, var(--p-500, var(--c-text)) 12%, transparent);
}
.v.edit {
  padding: 1px 4px;
  border: 1px solid var(--p-400, var(--c-border));
  border-radius: var(--radius-sm);
  background: var(--c-bg);
  color: var(--c-text);
}
.v.obj {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--c-text-3);
  cursor: default;
}
.del {
  flex: none;
  width: 18px;
  height: 18px;
  display: none;
  place-items: center;
  border-radius: var(--radius-sm);
  color: var(--c-text-3);
  font-size: 10px;
}
.row:hover .del {
  display: grid;
}
.del:hover {
  background: color-mix(in srgb, var(--s-error, #e5484d) 14%, transparent);
  color: var(--s-error, #e5484d);
}
</style>
