<script setup lang="ts">
/**
 * 新建会话配置弹窗：点「开始新对话」先配置、后开始。
 *
 * 配置项（参考 ST 的会话行为）：
 *  1. 会话名称 —— ST jsonl 文件名（不含扩展名），默认「角色名 - 时间戳」
 *  2. 开场白 —— 角色卡首条消息 / 备选问候（v2/v3 alternate_greetings，对应 ST 的 swipe）
 *  3. 人设 —— 跟随默认 / 锁定某人设（写 chat_metadata.persona，与 ST 一致）
 *  4. 生成参数 —— 温度 / top_p / 最大回复，可选设为本会话专属（chat_metadata.app_gen）
 *  5. 世界书 —— 只读展示角色卡绑定（改绑定会动角色卡，引导去角色卡/世界书页操作）
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import type { StCharacter } from '@/services/st/types'
import { getCharacter } from '@/services/st/data'
import { defaultChatName } from '@/services/st/chatdoc'
import { useChatStore, type NewChatOptions } from '@/stores/chat'
import { usePersonaStore } from '@/stores/persona'
import { useSettingsStore } from '@/stores/settings'
import Avatar from '@/components/Avatar.vue'

const props = defineProps<{
  open: boolean
  character: StCharacter | null
}>()

const emit = defineEmits<{ cancel: []; start: [opts: NewChatOptions] }>()

const chat = useChatStore()
const persona = usePersonaStore()
const gen = useSettingsStore()

/** 深层角色卡（含 data.alternate_greetings 与绑定世界书） */
const deep = ref<StCharacter | null>(null)
const loadingDeep = ref(false)

const name = ref('')
const greetingIdx = ref(0)
const personaId = ref('')
const useGen = ref(false)
const temperature = ref(1)
const topP = ref(1)
const maxTokens = ref(300)

/** 重名校验（同角色下文件名唯一） */
const dup = computed(
  () =>
    !!props.character &&
    chat.sessions.some((x) => x.avatar === props.character!.avatar && x.fileId === name.value.trim()),
)
const canStart = computed(() => !!props.character && name.value.trim().length > 0 && !dup.value)

/** 开场白可选项：首条消息 + 备选问候 */
const greetings = computed<{ label: string; text: string }[]>(() => {
  const c = deep.value ?? props.character
  if (!c) return []
  const alt = Array.isArray(c.data?.alternate_greetings)
    ? (c.data!.alternate_greetings as unknown[]).filter((x): x is string => typeof x === 'string' && !!x.trim())
    : []
  const out = [{ label: '首条消息', text: String(c.first_mes ?? '') }]
  alt.forEach((t, i) => out.push({ label: `备选 ${i + 1}`, text: t }))
  return out
})

/** 角色卡绑定的世界书（只读展示） */
const boundWorld = computed(() => {
  const c = deep.value ?? props.character
  const ext = c?.data?.extensions as { world?: unknown } | undefined
  const w = ext?.world
  return typeof w === 'string' && w ? w : ''
})

/** 打开时初始化 + 拉深层卡 */
watch(
  () => props.open,
  (v) => {
    if (!v || !props.character) return
    name.value = defaultChatName(props.character.name)
    greetingIdx.value = 0
    personaId.value = ''
    useGen.value = false
    temperature.value = gen.temperature
    topP.value = gen.topP
    maxTokens.value = gen.maxTokens
    deep.value = null
    loadingDeep.value = true
    getCharacter(props.character.avatar)
      .then((c) => (deep.value = c))
      .catch(() => undefined) // 拉不到就退化用列表数据（无备选问候）
      .finally(() => (loadingDeep.value = false))
  },
)

function onKey(e: KeyboardEvent) {
  if (props.open && e.key === 'Escape') emit('cancel')
}
onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => window.removeEventListener('keydown', onKey))

function start() {
  if (!canStart.value || !props.character) return
  const opts: NewChatOptions = {
    fileId: name.value.trim(),
  }
  const g = greetings.value[greetingIdx.value]
  if (g && g.text.trim()) opts.greetingText = g.text
  if (personaId.value) opts.personaId = personaId.value
  if (useGen.value) {
    opts.genOverride = {
      temperature: temperature.value,
      topP: topP.value,
      maxTokens: Math.max(16, Math.round(maxTokens.value)),
    }
  }
  emit('start', opts)
}
</script>

<template>
  <Teleport to="body">
    <Transition name="ncd">
      <div v-if="open && character" class="mask" @click.self="emit('cancel')" @contextmenu.prevent>
        <div class="dlg" role="dialog" aria-modal="true">
          <header>
            <Avatar class="ava" :avatar="character.avatar" :name="character.name" />
            <div class="tt">
              <h5>新建会话</h5>
              <span class="sub">{{ character.name }}</span>
            </div>
          </header>

          <div class="body">
            <!-- 1. 会话名称 -->
            <section>
              <label class="lb">会话名称</label>
              <input v-model="name" class="inp" placeholder="角色名 - 时间戳" spellcheck="false" />
              <p v-if="dup" class="err">已存在同名会话，请换一个名字</p>
            </section>

            <!-- 2. 开场白 -->
            <section v-if="greetings.length">
              <label class="lb">
                开场白
                <span v-if="loadingDeep" class="hint">读取中…</span>
              </label>
              <div class="gl">
                <button
                  v-for="(g, i) in greetings"
                  :key="i"
                  class="g"
                  :class="{ on: greetingIdx === i }"
                  type="button"
                  @click="greetingIdx = i"
                >
                  <span class="gl-lb">{{ g.label }}</span>
                  <span class="gl-tx">{{ g.text.trim() || '（空）' }}</span>
                </button>
              </div>
            </section>

            <!-- 3. 人设 -->
            <section v-if="persona.personas.length">
              <label class="lb">人设</label>
              <select v-model="personaId" class="inp field-select">
                <option value="">跟随默认人设</option>
                <option v-for="x in persona.personas" :key="x.id" :value="x.id">
                  锁定：{{ x.name }}
                </option>
              </select>
            </section>

            <!-- 4. 生成参数（可选本会话专属） -->
            <section>
              <label class="chk">
                <input v-model="useGen" type="checkbox" />
                <span>使用本会话专属生成参数</span>
                <span v-if="!useGen" class="hint">跟随全局设置</span>
              </label>
              <div v-if="useGen" class="params">
                <div class="p">
                  <span class="p-lb">温度 {{ temperature.toFixed(2) }}</span>
                  <input v-model.number="temperature" type="range" min="0" max="2" step="0.05" />
                </div>
                <div class="p">
                  <span class="p-lb">Top P {{ topP.toFixed(2) }}</span>
                  <input v-model.number="topP" type="range" min="0" max="1" step="0.01" />
                </div>
                <div class="p">
                  <span class="p-lb">最大回复</span>
                  <input v-model.number="maxTokens" class="inp num" type="number" min="16" max="8192" step="16" />
                </div>
              </div>
            </section>

            <!-- 5. 世界书（只读展示，改绑定会动角色卡） -->
            <section>
              <label class="lb">世界书</label>
              <p class="world">
                <template v-if="boundWorld">
                  已绑定「{{ boundWorld }}」
                </template>
                <template v-else>未绑定</template>
                <span class="hint">· 更换绑定请到角色卡或世界书页</span>
              </p>
            </section>
          </div>

          <footer class="btns">
            <button class="btn" @click="emit('cancel')">取消</button>
            <button class="btn btn-primary" :disabled="!canStart" @click="start">开始会话</button>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  z-index: 1100;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--c-scrim);
}
.dlg {
  display: flex;
  flex-direction: column;
  width: 380px;
  max-width: calc(100vw - 48px);
  max-height: calc(100vh - 96px);
  background: var(--c-bg);
  border: 1px solid var(--c-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--c-shadow-strong);
}
header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 14px 16px 10px;
  border-bottom: 1px solid var(--c-border);
}
.ava {
  width: 34px;
  height: 34px;
  border-radius: 50%;
  object-fit: cover;
  background: var(--c-panel);
  flex: none;
}
.tt h5 {
  font-size: 14px;
  font-weight: 600;
  color: var(--c-text);
}
.sub {
  font-size: 11px;
  color: var(--c-text-3);
}
.body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 12px 16px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.lb {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--c-text-2);
}
.hint {
  font-size: 10px;
  font-weight: 400;
  color: var(--c-text-3);
}
.inp {
  width: 100%;
  padding: 7px 10px;
  font-size: 12px;
  color: var(--c-text);
  background: var(--c-panel);
  border: 1px solid var(--c-border);
  border-radius: var(--radius-sm);
  outline: none;
}
.inp:focus {
  border-color: var(--p-500, #4a8fd4);
}
.err {
  margin-top: 4px;
  font-size: 11px;
  color: var(--s-error);
}

/* 开场白选项 */
.gl {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 180px;
  overflow-y: auto;
}
.g {
  text-align: left;
  padding: 7px 10px;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-sm);
  background: var(--c-panel);
  transition: border-color 0.12s, background 0.12s;
}
.g:hover {
  border-color: var(--c-text-3);
}
.g.on {
  border-color: var(--p-500, #4a8fd4);
  background: var(--p-50);
}
.gl-lb {
  display: block;
  font-size: 11px;
  font-weight: 600;
  color: var(--c-text-2);
}
.gl-tx {
  display: -webkit-box;
  margin-top: 2px;
  font-size: 11px;
  line-height: 1.5;
  color: var(--c-text-3);
  overflow: hidden;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  word-break: break-word;
}

/* 参数 */
.chk {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--c-text);
  cursor: pointer;
}
.params {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 8px;
  padding: 10px;
  background: var(--c-panel);
  border-radius: var(--radius-sm);
}
.p {
  display: flex;
  align-items: center;
  gap: 8px;
}
.p-lb {
  flex: none;
  width: 96px;
  font-size: 11px;
  color: var(--c-text-2);
}
.p input[type='range'] {
  flex: 1;
  accent-color: var(--p-500, #4a8fd4);
}
.num {
  width: 90px;
  flex: none;
}

/* 世界书 */
.world {
  font-size: 11px;
  color: var(--c-text-2);
}

.btns {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 16px 14px;
  border-top: 1px solid var(--c-border);
}

/* 过渡 */
.ncd-enter-active,
.ncd-leave-active {
  transition: opacity 0.14s ease;
}
.ncd-enter-active .dlg,
.ncd-leave-active .dlg {
  transition: transform 0.14s ease;
}
.ncd-enter-from,
.ncd-leave-to {
  opacity: 0;
}
.ncd-enter-from .dlg,
.ncd-leave-to .dlg {
  transform: scale(0.96);
}
</style>
