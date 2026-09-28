<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import { usePersonaStore } from '@/stores/persona'
import { useLayoutStore } from '@/stores/layout'
import { personaAvatarUrl } from '@/services/st/persona'

interface Entry {
  path: string
  label: string
  icon: string
}

// 左栏入口 · 全局固定（会话/角色卡/插件/设置）
// 用户设置走**顶部头像**（与微信一致的入口形态），不再单列一项
const entries: Entry[] = [
  {
    path: '/chats',
    label: '会话',
    icon: 'M14 9a2 2 0 0 1-2 2H6l-4 4V4a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2zM18 9h2a2 2 0 0 1 2 2v11l-4-4h-6a2 2 0 0 1-2-2v-1',
  },
  {
    path: '/contacts',
    label: '角色卡',
    icon: 'M12.832 8.445a1 1 0 00-1.589-.098l-2.075 3.098a1 1 0 000 1.11l2 3a1 1 0 001.664 0l2-3a1 1 0 000-1.11zM7 2h10a2 2 0 012 2v16a2 2 0 01-2 2H7a2 2 0 01-2-2V4a2 2 0 012-2z',
  },
  {
    path: '/worlds',
    label: '世界书',
    // Lucide book-open
    icon: 'M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2zM22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z',
  },
  {
    path: '/plugins',
    label: '插件',
    icon: 'M10 21V8a1 1 0 0 0-1-1H4a1 1 0 0 0-1 1v12a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-5a1 1 0 0 0-1-1H3',
  },
  {
    path: '/settings',
    label: '设置',
    icon: 'M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z',
  },
]

const emit = defineEmits<{ (e: 'change'): void }>()

const route = useRoute()
const persona = usePersonaStore()
const layout = useLayoutStore()

/**
 * 用户入口：桌面走**顶部头像**；窄屏（底部 Tab）需要一个图标项，
 * 否则用户设置就进不去了（微信底部也是「我」这一项）。
 */
const ME_ENTRY: Entry = {
  path: '/personas',
  label: '我',
  icon: 'M20 21a8 8 0 0 0-16 0M12 13a5 5 0 1 0 0-10 5 5 0 0 0 0 10z',
}

const visibleEntries = computed(() => (layout.isNarrow ? [...entries, ME_ENTRY] : entries))

/** 点导航项：先通知外层复位到列表页（窄屏单栏用） */
function go(): void {
  emit('change')
}

/** 顶部头像：默认人设的头像；没有（或加载失败）就退化为用户名首字母 */
const avatarId = computed(() => persona.defaultId || persona.personas[0]?.id || '')
const avatarUrl = computed(() => (avatarId.value ? personaAvatarUrl(avatarId.value) : ''))
const initial = computed(() => {
  const s = (persona.userName || '').trim()
  if (!s) return 'U'
  return Array.from(s)[0]!.toUpperCase()
})

// 加载失败（ST 未就绪 / 图片缺失）→ 显示首字母，不出现破图
const failed = ref(false)
watch(avatarUrl, () => {
  failed.value = false
})

onMounted(() => {
  void persona.load()
})
</script>

<template>
  <!--
    形态全靠断点：窄屏底部 Tab / 宽屏侧栏（尺寸在 uno.config 的 rail-* shortcuts）。
    order：窄屏是 flex-col 布局，导航要沉到底部（微信手机形态）；宽屏 row 时回到最左。
  -->
  <nav class="rail-bottom lg:rail-side order-last lg:order-first">
    <RouterLink
      to="/personas"
      class="hidden lg:flex avatar-btn"
      :class="{ active: route.path === '/personas' }"
      :title="persona.userName ? `用户设置 · ${persona.userName}` : '用户设置'"
      aria-label="用户设置"
    >
      <img
        v-if="avatarUrl && !failed"
        :src="avatarUrl"
        :alt="persona.userName || '用户'"
        @error="failed = true"
      />
      <span v-else class="ph">{{ initial }}</span>
    </RouterLink>

    <RouterLink
      v-for="e in visibleEntries"
      :key="e.path"
      :to="e.path"
      class="rail-item flex-1 lg:flex-none lg:w-36px lg:h-36px lg:rounded-lg lg:justify-center"
      :class="{ active: route.path === e.path }"
      :title="e.label"
      @click="go()"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
        <path :d="e.icon" />
      </svg>
      <span class="text-10px leading-tight lg:hidden">{{ e.label }}</span>
    </RouterLink>

    <div class="hidden lg:block flex-1" />
  </nav>
</template>

<style scoped>
/* 布局尺寸已移到 uno.config 的 rail-bottom / rail-side / rail-item shortcuts，
   这里只保留「状态」与「头像」这类原子类不适合表达的部分 */
.rail-item.active {
  color: var(--p-600);
  background: var(--p-100);
}
.rail-item:hover {
  background: var(--c-panel);
}

.avatar-btn {
  width: 36px;
  height: 36px;
  border-radius: 50%;
  margin-bottom: 14px;
  flex: none;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  background: var(--c-panel);
  cursor: pointer;
}
.avatar-btn:hover {
  opacity: 0.88;
}
.avatar-btn.active {
  outline: 2px solid var(--p-500);
  outline-offset: 1px;
}
.avatar-btn img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.avatar-btn .ph {
  font-size: 15px;
  color: var(--p-600);
  font-weight: 600;
}
</style>
