<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import AppShell from '@/layouts/AppShell.vue'
import CtxMenu from '@/components/CtxMenu.vue'
import { useBackgroundStore } from '@/stores/background'

const bg = useBackgroundStore()

function onGlobalCtx(e: MouseEvent): void {
  const t = e.target as Element | null
  if (t?.closest?.('input, textarea, [contenteditable="true"]')) return
  e.preventDefault()
}
onMounted(() => {
  window.addEventListener('contextmenu', onGlobalCtx)
  bg.apply()
})
onUnmounted(() => window.removeEventListener('contextmenu', onGlobalCtx))
</script>

<template>
  <AppShell />
  <CtxMenu />
</template>