<script setup lang="ts">
/**
 * 角色选择弹窗：会话页「新建会话」的第一步（先选角色，再进会话配置）。
 * 列表来自角色卡清单（chars.list），支持按名称/描述/标签搜索。
 */
import { computed, ref } from 'vue'
import type { StCharacter } from '@/services/st/types'
import Avatar from '@/components/Avatar.vue'

const props = defineProps<{
  open: boolean
  characters: StCharacter[]
}>()

const emit = defineEmits<{ cancel: []; pick: [c: StCharacter] }>()

const q = ref('')

const filtered = computed(() => {
  const kw = q.value.trim().toLowerCase()
  if (!kw) return props.characters
  return props.characters.filter(
    (c) =>
      c.name.toLowerCase().includes(kw) ||
      (c.description ?? '').toLowerCase().includes(kw) ||
      (c.tags ?? []).some((t) => t.toLowerCase().includes(kw)),
  )
})

function pick(c: StCharacter): void {
  q.value = ''
  emit('pick', c)
}
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="mask" @click.self="emit('cancel')">
      <div class="dlg" role="dialog" aria-modal="true">
        <header class="head">
          <span class="tt">选择角色</span>
          <button class="x" title="关闭" @click="emit('cancel')">
            <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <line x1="18" y1="6" x2="6" y2="18"></line>
              <line x1="6" y1="6" x2="18" y2="18"></line>
            </svg>
          </button>
        </header>
        <div class="body">
          <input v-model="q" class="search" type="text" placeholder="搜索角色（名称 / 描述 / 标签）" />
          <div v-if="!filtered.length" class="empty">
            {{ characters.length ? '没有匹配的角色' : '还没有角色卡，先到「角色卡」页导入或新建' }}
          </div>
          <button v-for="c in filtered" :key="c.avatar" class="row" type="button" @click="pick(c)">
            <Avatar class="ava" :avatar="c.avatar" :name="c.name" />
            <span class="col">
              <span class="nm">{{ c.name }}</span>
              <span class="ds">{{ (c.description ?? '').replace(/\s+/g, ' ').slice(0, 60) || '—' }}</span>
            </span>
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  z-index: 1100;
  display: grid;
  place-items: center;
  background: var(--c-scrim);
}
.dlg {
  display: flex;
  flex-direction: column;
  width: 420px;
  max-width: calc(100vw - 48px);
  max-height: calc(100vh - 96px);
  background: var(--c-bg);
  border: 1px solid var(--c-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--c-shadow-strong);
  overflow: hidden;
}
.head {
  flex: none;
  display: flex;
  align-items: center;
  padding: 12px 14px;
  border-bottom: 1px solid var(--c-border);
}
.tt {
  font-size: 13px;
  font-weight: 600;
}
.x {
  margin-left: auto;
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border-radius: var(--radius-sm);
  color: var(--c-text-3);
  font-size: 15px;
}
.x:hover {
  background: var(--c-panel);
  color: var(--c-text);
}
.body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 10px 12px;
  gap: 8px;
}
.search {
  flex: none;
  padding: 7px 10px;
  font-size: 12px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
  color: var(--c-text);
  outline: none;
}
.search:focus {
  border-color: var(--p-400);
}
.empty {
  padding: 24px 8px;
  text-align: center;
  font-size: 12px;
  color: var(--c-text-3);
}
.row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px;
  text-align: left;
  border-radius: var(--radius-md);
}
.row:hover {
  background: var(--c-panel);
}
.ava {
  width: 36px;
  height: 36px;
  border-radius: 50%;
  object-fit: cover;
  background: var(--c-panel);
  flex: none;
}
.col {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.nm {
  font-size: 13px;
  font-weight: 500;
  color: var(--c-text);
}
.ds {
  font-size: 11px;
  color: var(--c-text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
