<script setup lang="ts">
/**
 * 用户（中栏）：人设列表
 *
 * 列表项 = 头像 + 人设名（+ 默认标记）。
 */
import { computed, onMounted, ref } from 'vue'
import MidPane from '@/components/MidPane.vue'
import { usePersonaStore } from '@/stores/persona'
import { useLayoutStore } from '@/stores/layout'
import { personaAvatarUrl } from '@/services/st/persona'

const p = usePersonaStore()
const layout = useLayoutStore()
const q = ref('')

const filtered = computed(() => {
  const kw = q.value.trim().toLowerCase()
  if (!kw) return p.personas
  return p.personas.filter(
    (x) => x.name.toLowerCase().includes(kw) || x.description.toLowerCase().includes(kw),
  )
})

onMounted(() => {
  void p.load()
})
</script>

<template>
  <MidPane title="用户设置" :subtitle="`${p.personas.length} 个人设`">
    <template #head>
      <input v-model="q" class="search" type="text" placeholder="搜索人设" />
      <button class="add" @click="p.beginCreate()">＋ 新增用户</button>
    </template>

    <div v-if="p.loading && !p.personas.length" class="empty-hint">加载人设中…</div>
    <div v-else-if="p.error" class="empty-hint err">{{ p.error }}</div>
    <div v-else-if="!filtered.length" class="empty-hint">没有人设</div>

    <div
      v-for="item in filtered"
      :key="item.id"
      class="list-row"
      :class="{ on: p.selectedId === item.id, cur: p.defaultId === item.id }"
      @click="() => { p.select(item.id); layout.showDetail() }"
    >
      <img class="face" :src="personaAvatarUrl(item.id)" :alt="item.name" />
      <span class="tx">
        <span class="lb">
          {{ item.name }}
          <em v-if="p.defaultId === item.id" class="badge">当前</em>
        </span>
        <span class="ds">{{ item.description || '无描述' }}</span>
      </span>
      <button
        v-if="p.defaultId !== item.id"
        class="use"
        :disabled="p.saving"
        @click.stop="p.usePersona(item.id)"
      >
        使用
      </button>
    </div>
  </MidPane>
</template>

<style scoped>
.search {
  width: 100%;
  padding: 6px 9px;
  font-size: 12px;
  color: var(--c-text);
  background: var(--c-bg);
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  outline: none;
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
.add:hover {
  border-color: var(--p-400);
  background: var(--p-50);
}
.empty-hint {
  padding: 10px 12px;
  font-size: 12px;
  color: var(--c-text-3);
}
.use {
  flex: none;
  margin-left: auto;
  padding: 3px 9px;
  font-size: 11px;
  border: 1px solid var(--p-400);
  border-radius: var(--radius-md);
  color: var(--p-600);
  background: var(--c-canvas);
  opacity: 0;
}
.list-row:hover .use,
.list-row.on .use {
  opacity: 1;
}
.use:hover:not(:disabled) {
  background: var(--p-500);
  color: var(--c-on-primary);
}
.use:disabled {
  opacity: 0.5;
}
.empty-hint.err {
  color: var(--s-error);
}
.list-row {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  text-align: left;
  color: var(--c-text-2);
  cursor: pointer;
}
.list-row:hover {
  background: var(--c-panel);
}
.list-row.on {
  background: var(--p-100);
  color: var(--p-600);
}
.face {
  width: 34px;
  height: 34px;
  flex: none;
  object-fit: cover;
  border-radius: 50%;
  background: var(--c-panel);
}
.tx {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}
.lb {
  font-size: 13px;
  display: flex;
  align-items: center;
  gap: 5px;
}
.badge {
  padding: 0 5px;
  font-size: 10px;
  font-style: normal;
  border-radius: 4px;
  background: var(--p-500);
  color: var(--c-on-primary);
}
.ds {
  font-size: 10px;
  color: var(--c-text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
