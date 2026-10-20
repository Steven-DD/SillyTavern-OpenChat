<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import MidPane from '@/components/MidPane.vue'
import { useLayoutStore } from '@/stores/layout'

const { t } = useI18n()

/** 设置分区（中栏二级列表，右栏渲染对应表单）—— 图标统一 Lucide */
const sections = computed(() => [
  {
    id: 'api',
    // Lucide gem
    icon: 'M6 3h12l4 6-10 13L2 9ZM11 3 8 9l4 13 4-13-3-6M2 9h20',
    label: t('settings.model'),
  },
  {
    id: 'instruct',
    // Lucide text-quote / instruct
    icon: 'M3 21c3 0 7-1 7-8V5c0-1.25-.756-2.017-2-2H4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2 1 0 1 0 1 1v1c0 1-1 2-2 2s-1 .008-1 1.031V20c0 1 0 1 1 1zM15 21c3 0 7-1 7-8V5c0-1.25-.757-2.017-2-2h-4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2 1 0 1 0 1 1v1c0 1-1 2-2 2s-1 .008-1 1.031V20c0 1 0 1 1 1z',
    label: t('settings.instruct'),
  },
  {
    id: 'theme',
    // Lucide settings
    icon: 'M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z',
    label: t('settings.theme'),
  },
  {
    id: 'data',
    // Lucide database（ellipse 转弧线合并）
    icon: 'M3 5a9 3 0 1 0 18 0 9 3 0 1 0-18 0zM3 5V19A9 3 0 0 0 21 19V5M3 12A9 3 0 0 0 21 12',
    label: t('settings.data'),
  },
])

const route = useRoute()
const router = useRouter()
const layout = useLayoutStore()

const current = computed(() => String(route.query.sec ?? 'api'))

function go(id: string) {
  void router.push({ path: '/settings', query: { sec: id } })
  layout.showDetail() // 窄屏单栏：进入右栏表单
}
</script>

<template>
  <MidPane :title="t('nav.settings')">
    <button
      v-for="s in sections"
      :key="s.id"
      class="cat"
      :class="{ active: current === s.id }"
      @click="go(s.id)"
    >
      <span class="ic">
        <svg
          viewBox="0 0 24 24"
          width="16"
          height="16"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path :d="s.icon" />
        </svg>
      </span>
      <span class="tx">
        <span class="lb">{{ s.label }}</span>
      </span>
    </button>
  </MidPane>
</template>

<style scoped>
.cat {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 10px 11px;
  border-radius: var(--radius-md);
  text-align: left;
  color: var(--c-text-2);
}
.cat:hover {
  background: var(--c-panel);
}
.cat.active {
  background: var(--p-100);
  color: var(--p-600);
}
.cat.active .lb {
  font-weight: 600;
}
.ic {
  /* 图标列：固定高度并与标题行（13px ≈ 18px 行高）垂直居中，
     修复图标浮在标题上方的错位 */
  display: flex;
  align-items: center;
  height: 18px;
  flex: none;
}
.ic svg {
  display: block;
}
.tx {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}
.lb {
  font-size: 13px;
}
</style>
