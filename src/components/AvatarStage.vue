<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  stageLayout,
  currentEmotion,
  currentLipSync,
  saveStageSettings,
  type StageLayout,
} from '@/services/st/modelStage'

defineProps<{
  avatar: string
  characterName: string
}>()

const emit = defineEmits<{
  close: []
}>()

/* ---- 视口交互：拖拽与悬浮 ---- */
const isDragging = ref(false)
const dragOffset = ref({ x: 0, y: 0 })
const stagePos = ref({ x: 40, y: 80 })

function startDrag(e: MouseEvent) {
  if (stageLayout.value !== 'floating') return
  isDragging.value = true
  dragOffset.value = {
    x: e.clientX - stagePos.value.x,
    y: e.clientY - stagePos.value.y,
  }
  window.addEventListener('mousemove', onDragging)
  window.addEventListener('mouseup', stopDrag)
}

function onDragging(e: MouseEvent) {
  if (!isDragging.value) return
  stagePos.value = {
    x: Math.max(10, Math.min(window.innerWidth - 320, e.clientX - dragOffset.value.x)),
    y: Math.max(10, Math.min(window.innerHeight - 420, e.clientY - dragOffset.value.y)),
  }
}

function stopDrag() {
  isDragging.value = false
  window.removeEventListener('mousemove', onDragging)
  window.removeEventListener('mouseup', stopDrag)
}

/* ---- 视口模式切换 ---- */
function switchLayout(layout: StageLayout) {
  stageLayout.value = layout
  saveStageSettings({ layout })
  if (layout === 'hidden') emit('close')
}

/* ---- 视线跟踪与呼吸动态 ---- */
const gaze = ref({ x: 0, y: 0 })
function onMouseMove(e: MouseEvent) {
  const rect = (e.currentTarget as HTMLElement)?.getBoundingClientRect()
  if (!rect) return
  const cx = rect.left + rect.width / 2
  const cy = rect.top + rect.height / 2
  gaze.value = {
    x: Math.max(-15, Math.min(15, (e.clientX - cx) / 15)),
    y: Math.max(-10, Math.min(10, (e.clientY - cy) / 20)),
  }
}

function resetGaze() {
  gaze.value = { x: 0, y: 0 }
}

/* ---- 情绪标签与表情渲染计算 ---- */
const emotionLabelText = computed(() => {
  const emo = currentEmotion.value.toLowerCase()
  const labels: Record<string, string> = {
    joy: '喜悦',
    happy: '开心',
    sadness: '悲伤',
    sad: '难过',
    anger: '生气',
    angry: '愤怒',
    surprise: '惊讶',
    blush: '害羞',
    fear: '恐惧',
    neutral: '待机',
  }
  return labels[emo] || emo
})

// 口型缩放高度（由 speech.ts 实时音频 RMS 驱动）
const mouthScaleY = computed(() => 1 + currentLipSync.value * 2.2)

// 触碰互动
const isTapped = ref(false)
function onAvatarTap() {
  isTapped.value = true
  setTimeout(() => {
    isTapped.value = false
  }, 600)
}
</script>

<template>
  <div
    v-if="stageLayout !== 'hidden'"
    class="avatar-stage-container"
    :class="[
      `layout-${stageLayout}`,
      { dragging: isDragging, tapped: isTapped },
    ]"
    :style="
      stageLayout === 'floating'
        ? { left: `${stagePos.x}px`, top: `${stagePos.y}px` }
        : {}
    "
    @mousemove="onMouseMove"
    @mouseleave="resetGaze"
  >
    <!-- 头部工具栏 -->
    <div class="stage-header" @mousedown="startDrag">
      <div class="character-info">
        <span class="character-name">{{ characterName || '角色立绘' }}</span>
        <span class="emotion-badge" :class="currentEmotion">
          {{ emotionLabelText }}
        </span>
      </div>
      <div class="stage-actions">
        <button
          class="mode-btn"
          :class="{ active: stageLayout === 'docked' }"
          title="停靠右侧"
          @click="switchLayout('docked')"
        >
          ⊞
        </button>
        <button
          class="mode-btn"
          :class="{ active: stageLayout === 'floating' }"
          title="浮动窗口"
          @click="switchLayout('floating')"
        >
          ❐
        </button>
        <button
          class="mode-btn"
          :class="{ active: stageLayout === 'background' }"
          title="背景模式"
          @click="switchLayout('background')"
        >
          ▤
        </button>
        <button class="mode-btn close-btn" title="关闭视口" @click="switchLayout('hidden')">
          ✕
        </button>
      </div>
    </div>

    <!-- 舞台主体 Canvas / 立绘视口 -->
    <div class="stage-viewport" @click="onAvatarTap">
      <!-- 角色形象层 -->
      <div
        class="avatar-wrapper"
        :style="{
          transform: `translate(${gaze.x}px, ${gaze.y}px)`,
        }"
      >
        <img
          v-if="avatar"
          :src="avatar"
          class="character-image"
          alt="Avatar"
          draggable="false"
        />
        <div v-else class="avatar-placeholder">
          <span>暂无立绘资产</span>
        </div>

        <!-- 动态表情与口型特效层 -->
        <div
          v-if="currentLipSync > 0.05"
          class="lipsync-indicator"
          :style="{
            transform: `scaleY(${mouthScaleY})`,
          }"
        />
      </div>

      <!-- 状态微件：TTS 口型指示器 -->
      <div v-if="currentLipSync > 0.05" class="speaking-wave-badge">
        <span class="wave-bar" :style="{ height: `${currentLipSync * 16 + 4}px` }" />
        <span class="wave-bar" :style="{ height: `${currentLipSync * 22 + 6}px` }" />
        <span class="wave-bar" :style="{ height: `${currentLipSync * 14 + 4}px` }" />
        <span class="text">正在说话</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.avatar-stage-container {
  display: flex;
  flex-direction: column;
  background: var(--bg-card, rgba(30, 34, 42, 0.9));
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.1));
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.36);
  backdrop-filter: blur(16px);
  user-select: none;
  z-index: 90;
  transition: box-shadow 0.2s, border-color 0.2s;
}

/* 停靠右侧模式 */
.layout-docked {
  width: 320px;
  height: 100%;
  border-left: 1px solid var(--border-color, rgba(255, 255, 255, 0.12));
  border-radius: 0;
}

/* 浮动 PIP 窗口模式 */
.layout-floating {
  position: fixed;
  width: 280px;
  height: 420px;
  border-radius: 12px;
  overflow: hidden;
}

/* 背景融入模式 */
.layout-background {
  position: absolute;
  right: 0;
  top: 0;
  width: 45%;
  height: 100%;
  opacity: 0.25;
  pointer-events: none;
  border: none;
  background: transparent;
  box-shadow: none;
}

.stage-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  background: rgba(0, 0, 0, 0.25);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  cursor: grab;
}
.layout-floating .stage-header:active {
  cursor: grabbing;
}

.character-info {
  display: flex;
  align-items: center;
  gap: 8px;
}
.character-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-color, #e0e6ed);
}

.emotion-badge {
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.1);
  color: #a0aec0;
}
.emotion-badge.joy,
.emotion-badge.happy {
  background: rgba(72, 187, 120, 0.2);
  color: #48bb78;
}
.emotion-badge.anger,
.emotion-badge.angry {
  background: rgba(245, 101, 101, 0.2);
  color: #f56565;
}
.emotion-badge.sadness,
.emotion-badge.sad {
  background: rgba(66, 153, 225, 0.2);
  color: #4299e1;
}
.emotion-badge.surprise {
  background: rgba(237, 137, 54, 0.2);
  color: #ed8936;
}

.stage-actions {
  display: flex;
  gap: 4px;
}
.mode-btn {
  background: transparent;
  border: none;
  color: #a0aec0;
  font-size: 12px;
  padding: 4px 6px;
  border-radius: 4px;
  cursor: pointer;
}
.mode-btn:hover {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
}
.mode-btn.active {
  background: rgba(99, 102, 241, 0.3);
  color: #818cf8;
}
.close-btn:hover {
  background: rgba(245, 101, 101, 0.3);
  color: #f56565;
}

.stage-viewport {
  flex: 1;
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  cursor: pointer;
}

.avatar-wrapper {
  position: relative;
  width: 90%;
  height: 90%;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.15s ease-out;
  animation: breathing 4s ease-in-out infinite;
}

@keyframes breathing {
  0%,
  100% {
    transform: translateY(0);
  }
  50% {
    transform: translateY(-6px);
  }
}

.tapped .avatar-wrapper {
  animation: tap-bounce 0.5s ease;
}

@keyframes tap-bounce {
  0% {
    transform: scale(1);
  }
  30% {
    transform: scale(0.95) translateY(4px);
  }
  60% {
    transform: scale(1.05) translateY(-8px);
  }
  100% {
    transform: scale(1);
  }
}

.character-image {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
  filter: drop-shadow(0 8px 16px rgba(0, 0, 0, 0.4));
}

.avatar-placeholder {
  color: #718096;
  font-size: 12px;
}

.speaking-wave-badge {
  position: absolute;
  bottom: 12px;
  display: flex;
  align-items: flex-end;
  gap: 3px;
  padding: 4px 8px;
  border-radius: 12px;
  background: rgba(0, 0, 0, 0.6);
  backdrop-filter: blur(8px);
}
.wave-bar {
  width: 3px;
  background: #6366f1;
  border-radius: 2px;
  transition: height 0.08s ease;
}
.speaking-wave-badge .text {
  font-size: 10px;
  color: #e0e7ff;
  margin-left: 4px;
}
</style>
