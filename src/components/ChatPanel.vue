<script setup lang="ts">
/**
 * 会话配置面板（顶栏「···」唤起，推挤式二级内容区）
 *
 * 四个 Tab：
 *  1. 信息 —— 会话名（重命名）/ 角色 / 人设锁定（从顶栏迁入）/ 本会话累计用量
 *  2. 参数 —— 会话级生成参数覆盖（跟随全局 / 自定义，写 chat_metadata.app_gen）
 *  3. 记忆 —— 作者注释（chat_metadata.note_*）
 *  4. 世界书 —— 角色卡绑定世界书（只读）+ 最近一次注入条数，管理跳转世界书页
 *
 * 原设计记忆三件套中的总结/向量依赖 ST 扩展接口，暂未接入，后续版本补。
 * 推挤式约束：展开/收起只影响布局宽度，不触碰生成流程（流式不中断）。
 */
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useChatStore } from '@/stores/chat'
import { usePersonaStore } from '@/stores/persona'
import { useSettingsStore } from '@/stores/settings'
import { getCharacter } from '@/services/st/data'
import type { StCharacter } from '@/services/st/types'
import { fmtCost, fmtTok } from '@/services/usage'
import { listSprites, classifyEmotion, matchSprite, loadSpriteFallback, spriteUrl, type Sprite } from '@/services/st/expressions'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()

const chat = useChatStore()
const persona = usePersonaStore()
const gen = useSettingsStore()
const router = useRouter()

type Tab = 'info' | 'gen' | 'note' | 'wi'
const tab = ref<Tab>('info')

const defaultPersonaName = computed(
  () => persona.personas.find((x) => x.id === persona.defaultId)?.name ?? '默认',
)

/* ---- 信息 Tab：重命名 ---- */
const nameInput = ref('')
const renaming = ref(false)

watch(
  () => [props.open, chat.currentFile] as const,
  ([o]) => {
    if (o) nameInput.value = chat.currentFile
  },
  { immediate: true },
)

async function commitRename(): Promise<void> {
  renaming.value = false
  const name = nameInput.value.trim()
  if (!chat.currentAvatar || !chat.currentFile || !name || name === chat.currentFile) return
  await chat.renameSession(chat.currentAvatar, chat.currentFile, name)
  nameInput.value = chat.currentFile // store 内部失败时 currentFile 不变，回读保持一致
}

/* ---- 信息 Tab：本会话累计 ---- */

/* ---- 信息 Tab：会话词数统计（本地计算；CJK 按字计，其余按词计） ---- */
const sessionStats = computed(() => {
  const msgs = chat.messages.filter((m) => m.content.trim() && !m.pending)
  const words = (s: string): number => {
    const cjk = (s.match(/[\u4e00-\u9fff\u3040-\u30ff\uac00-\ud7af]/g) ?? []).length
    const rest = s
      .replace(/[\u4e00-\u9fff\u3040-\u30ff\uac00-\ud7af]/g, ' ')
      .split(/\s+/)
      .filter(Boolean).length
    return cjk + rest
  }
  const byName = new Map<string, { msgs: number; words: number }>()
  let totalWords = 0
  for (const m of msgs) {
    const w = words(m.content)
    totalWords += w
    const key = m.isSystem ? '旁白' : m.role === 'user' ? m.name || '我' : m.name || '角色'
    const cur = byName.get(key) ?? { msgs: 0, words: 0 }
    cur.msgs++
    cur.words += w
    byName.set(key, cur)
  }
  return {
    total: msgs.length,
    totalWords,
    byName: [...byName.entries()].sort((a, b) => b[1].words - a[1].words),
  }
})

/* ---- 参数 Tab：会话级覆盖（chat_metadata.app_gen） ---- */
const useGen = ref(false)
const temperature = ref(1)
const topP = ref(1)
const maxTokens = ref(300)

watch(
  () => [props.open, chat.sessionKeyOf] as const,
  ([o]) => {
    if (!o) return
    tab.value = 'info'
    const ov = chat.genOverride
    useGen.value = !!ov
    temperature.value = ov?.temperature ?? gen.temperature
    topP.value = ov?.topP ?? gen.topP
    maxTokens.value = ov?.maxTokens ?? gen.maxTokens
    void reloadSprites()
  },
  { immediate: true },
)

function applyGen(on: boolean): void {
  useGen.value = on
  if (!on) {
    void chat.setGenOverride(null)
    return
  }
  pushGen()
}

function pushGen(): void {
  if (!useGen.value) return
  void chat.setGenOverride({
    temperature: temperature.value,
    topP: topP.value,
    maxTokens: maxTokens.value,
  })
}

const genDirty = computed(() => {
  if (!chat.genOverride) return false
  const o = chat.genOverride
  return (
    o.temperature !== temperature.value || o.topP !== topP.value || o.maxTokens !== maxTokens.value
  )
})

/* ---- 表情立绘 ---- */
const sprites = ref<Sprite[]>([])
const emotion = ref('')
const emotionBusy = ref(false)
const activeSprite = computed<Sprite | null>(() =>
  matchSprite(sprites.value, emotion.value, spriteFallback.value),
)
const spriteFallback = ref('')

async function reloadSprites(): Promise<void> {
  sprites.value = []
  emotion.value = ''
  const c = chat.currentCharacter
  if (!c) return
  sprites.value = await listSprites(c.name).catch(() => [])
  spriteFallback.value = await loadSpriteFallback()
}

async function detectEmotion(): Promise<void> {
  const last = [...chat.messages].reverse().find((m) => m.role === 'assistant' && m.content.trim())
  if (!last) return
  emotionBusy.value = true
  try {
    const label = await classifyEmotion(last.content)
    if (label) emotion.value = label
  } finally {
    emotionBusy.value = false
  }
}

/* ---- 群头像上传 ---- */
async function onGroupAvatarPick(e: Event): Promise<void> {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  await chat.uploadGroupAvatar(file)
  input.value = ''
}

/* ---- 记忆 Tab：作者注释（chat_metadata.note_*） ---- */
const anOn = ref(false)
const anText = ref('')
const anInterval = ref(1)
const anDepth = ref(4)
const anPosition = ref(1)
const anRole = ref(0)

watch(
  () => [props.open, chat.sessionKeyOf] as const,
  ([o]) => {
    if (!o) return
    const an = chat.authorsNote
    anOn.value = !!an
    anText.value = an?.prompt ?? ''
    anInterval.value = an?.interval ?? 1
    anDepth.value = an?.depth ?? 4
    anPosition.value = an?.position ?? 1
    anRole.value = an?.role ?? 0
  },
  { immediate: true },
)

function applyAn(on: boolean): void {
  anOn.value = on
  if (!on) {
    void chat.setAuthorsNote(null)
    return
  }
  pushAn()
}

function pushAn(): void {
  if (!anOn.value || !anText.value.trim()) return
  void chat.setAuthorsNote({
    prompt: anText.value,
    interval: Math.max(1, Math.floor(anInterval.value) || 1),
    depth: Math.max(0, Math.floor(anDepth.value) || 0),
    position: anPosition.value,
    role: anRole.value,
  })
}

/* ---- 世界书 Tab：角色卡绑定（只读，深层卡才带 data.extensions.world） ---- */
const deep = ref<StCharacter | null>(null)

watch(
  () => [props.open, chat.currentAvatar] as const,
  ([o, avatar]) => {
    deep.value = null
    if (o && avatar) {
      getCharacter(avatar)
        .then((c) => (deep.value = c))
        .catch(() => undefined)
    }
  },
  { immediate: true },
)

const boundWorld = computed(() => {
  const ext = (deep.value ?? chat.currentCharacter)?.data?.extensions as { world?: unknown } | undefined
  return typeof ext?.world === 'string' && ext.world ? ext.world : ''
})

const wiCount = computed(() => chat.promptInfo?.wiActivated ?? 0)

function goWorlds(): void {
  emit('close')
  void router.push('/worlds')
}
</script>

<template>
  <aside class="panel" :class="{ open }">
    <div class="inner">
      <div class="tabs">
        <button class="t" :class="{ on: tab === 'info' }" @click="tab = 'info'">信息</button>
        <button class="t" :class="{ on: tab === 'gen' }" @click="tab = 'gen'">参数</button>
        <button class="t" :class="{ on: tab === 'note' }" @click="tab = 'note'">记忆</button>
        <button class="t" :class="{ on: tab === 'wi' }" @click="tab = 'wi'">世界书</button>
        <button class="x" title="收起（或再点顶栏 ···）" @click="emit('close')">×</button>
      </div>

      <!-- 信息 -->
      <div v-if="tab === 'info'" class="body">
        <!-- 群聊：成员与生成模式 -->
        <template v-if="chat.group">
          <label class="fld">
            <span class="lbl">群组</span>
            <span class="val">{{ chat.group.name }}（{{ chat.group.members.length }} 名成员）</span>
          </label>
          <label class="fld">
            <span class="lbl">群头像</span>
            <input
              class="in"
              type="file"
              accept="image/*"
              :disabled="chat.streaming"
              @change="onGroupAvatarPick"
            />
          </label>
          <label class="fld">
            <span class="lbl">激活策略（决定发言人数与顺序）</span>
            <select
              class="in"
              :value="chat.group.activation_strategy"
              :disabled="chat.streaming"
              @change="chat.setGroupActivationStrategy(Number(($event.target as HTMLSelectElement).value))"
            >
              <option :value="0">自然顺序（@提及优先 + talkativeness 掷骰）</option>
              <option :value="1">列表顺序（启用成员全部发言）</option>
              <option :value="2">手动（非用户输入时随机 1 人）</option>
              <option :value="3">池化（未发言者随机 1 人）</option>
            </select>
          </label>
          <label class="fld">
            <span class="lbl">生成模式（成员卡片合并方式）</span>
            <select
              class="in"
              :value="chat.group.generation_mode"
              :disabled="chat.streaming"
              @change="chat.setGroupGenerationMode(Number(($event.target as HTMLSelectElement).value))"
            >
              <option :value="0">轮换（每次换一个成员）</option>
              <option :value="1">依次生成（全体成员）</option>
              <option :value="2">依次生成（含被禁用成员）</option>
            </select>
          </label>
          <div class="fld">
            <span class="lbl">成员（talkativeness 参与自动模式掷骰）</span>
            <span class="val mems">{{ chat.groupMemberMetaList.map((m) => `${m.name}(${m.talkativeness ?? 0.5})`).join('、') }}</span>
          </div>
          <label class="fld chk">
            <input
              type="checkbox"
              :checked="chat.groupAutoMode"
              :disabled="chat.streaming"
              @change="chat.toggleGroupAutoMode(($event.target as HTMLInputElement).checked)"
            />
            <span>自动发言（每 {{ chat.group.auto_mode_delay ?? 5 }} 秒无输入触发一轮）</span>
          </label>
        </template>
        <template v-else>
        <label class="fld">
          <span class="lbl">会话名称</span>
          <input
            v-model="nameInput"
            class="in"
            :disabled="!chat.currentFile"
            @focus="renaming = true"
            @blur="renaming && commitRename()"
            @keydown.enter="($event.target as HTMLInputElement).blur()"
          />
        </label>
        <p class="hint">{{ renaming ? 'Enter 确认重命名（会话列表同步更新）' : '回车确认修改' }}</p>

        <div class="row2">
          <div class="fld">
            <span class="lbl">角色</span>
            <span class="val">{{ chat.currentCharacter?.name ?? '—' }}</span>
          </div>
          <div class="fld">
            <span class="lbl">消息数</span>
            <span class="val mono">{{ chat.messages.length }}</span>
          </div>
        </div>

        <label v-if="persona.personas.length" class="fld">
          <span class="lbl">人设（本会话）</span>
          <select
            class="in"
            :value="chat.lockedPersonaId ?? ''"
            :disabled="chat.streaming"
            @change="chat.lockPersona(($event.target as HTMLSelectElement).value || null)"
          >
            <option value="">跟随默认（{{ defaultPersonaName }}）</option>
            <option v-for="x in persona.personas" :key="x.id" :value="x.id">锁定：{{ x.name }}</option>
          </select>
        </label>

        <div class="stat">
          <p class="stat-t">本会话累计</p>
          <div class="stat-r"><span>输入 / 输出</span><span class="mono"
            >↑ {{ fmtTok(chat.sessionUsage.input) }} · ↓ {{ fmtTok(chat.sessionUsage.output) }}</span
          ></div>
          <div class="stat-r"><span>费用</span><span class="mono"
            >{{ chat.sessionUsage.cost ? `≈ ${fmtCost(chat.sessionUsage.cost)}` : '—' }}</span
          ></div>
          <div class="stat-r"><span>生成轮数</span><span class="mono">{{ chat.sessionUsage.turns }}</span></div>
          <p class="stat-note">壳侧本地统计；费用按内置牌价估算，仅供参考</p>
        </div>

        <div class="stat">
          <p class="stat-t">会话词数统计</p>
          <div class="stat-r"><span>消息 / 词数</span><span class="mono"
            >{{ sessionStats.total }} 条 · 约 {{ sessionStats.totalWords }} 词</span
          ></div>
          <div v-for="[nm, st] in sessionStats.byName" :key="nm" class="stat-r">
            <span>{{ nm }}</span><span class="mono">{{ st.msgs }} 条 / {{ st.words }} 词</span>
          </div>
          <p class="stat-note">按当前会话消息本地计算（CJK 按字计，其余按词计）</p>
        </div>

        <!-- 表情立绘（sprites 立绘 + classify 自动检测，不可用时手动） -->
        <template v-if="sprites.length">
          <div class="fld">
            <span class="lbl">表情立绘（{{ sprites.length }}）</span>
            <div class="sprite-row">
              <img
                v-for="sp in sprites"
                :key="sp.path"
                class="sprite"
                :class="{ on: activeSprite?.path === sp.path }"
                :src="spriteUrl(sp.path)"
                :alt="sp.label"
                :title="sp.label"
                @click="emotion = sp.label"
              />
            </div>
          </div>
          <div class="btns">
            <button class="btn" :disabled="emotionBusy" @click="detectEmotion">
              {{ emotionBusy ? '识别中…' : '按最新回复识别表情' }}
            </button>
          </div>
          <p class="hint">
            立绘来自 ST sprites/&lt;角色&gt;/ 目录（ST 的 Character Expressions 扩展数据）。
            自动识别需要 ST 端启用 classify 模块，否则手动点选。
          </p>
        </template>
        </template>
      </div>

      <!-- 参数 -->
      <div v-else-if="tab === 'gen'" class="body">
        <label class="switch-row">
          <input type="checkbox" :checked="useGen" :disabled="chat.streaming" @change="applyGen(($event.target as HTMLInputElement).checked)" />
          <span>本会话专属参数<span class="sub">{{ useGen ? '（chat_metadata.app_gen）' : '（跟随全局设置）' }}</span></span>
        </label>

        <template v-if="useGen">
          <label class="fld">
            <span class="lbl">温度 <span class="mono">{{ temperature.toFixed(2) }}</span></span>
            <input v-model.number="temperature" type="range" min="0" max="2" step="0.05" :disabled="chat.streaming" @change="pushGen" />
          </label>
          <label class="fld">
            <span class="lbl">Top P <span class="mono">{{ topP.toFixed(2) }}</span></span>
            <input v-model.number="topP" type="range" min="0.05" max="1" step="0.05" :disabled="chat.streaming" @change="pushGen" />
          </label>
          <label class="fld">
            <span class="lbl">最大回复 tokens</span>
            <input v-model.number="maxTokens" class="in" type="number" min="16" max="8192" step="16" :disabled="chat.streaming" @change="pushGen" />
          </label>
          <p class="hint">{{ genDirty ? '修改即写盘并即时生效' : '与当前会话已存参数一致' }}</p>
        </template>
        <p v-else class="hint">
          全局：温度 {{ gen.temperature }} · Top P {{ gen.topP }} · 回复 {{ gen.maxTokens }} tok（在「设置 → 模型」修改）
        </p>
      </div>

      <!-- 记忆：作者注释 -->
      <div v-else-if="tab === 'note'" class="body">
        <label class="switch-row">
          <input type="checkbox" :checked="anOn" :disabled="chat.streaming" @change="applyAn(($event.target as HTMLInputElement).checked)" />
          <span>作者注释<span class="sub">{{ anOn ? '（已启用）' : '（未启用）' }}</span></span>
        </label>

        <template v-if="anOn">
          <label class="fld">
            <span class="lbl">注释内容（支持 {{ '\{\{char\}\}' }} / {{ '\{\{user\}\}' }} 宏）</span>
            <textarea
              v-model="anText"
              class="in"
              rows="4"
              placeholder="例：仅角色扮演，不要跳出故事"
              :disabled="chat.streaming"
              @change="pushAn"
            ></textarea>
          </label>
          <div class="row2">
            <label class="fld">
              <span class="lbl">注入位置</span>
              <select v-model.number="anPosition" class="in" :disabled="chat.streaming" @change="pushAn">
                <option :value="1">聊天内 @Depth</option>
                <option :value="0">角色定义后</option>
                <option :value="2">角色定义前</option>
              </select>
            </label>
            <label class="fld">
              <span class="lbl">深度</span>
              <input v-model.number="anDepth" class="in" type="number" min="0" max="32" :disabled="chat.streaming || anPosition !== 1" @change="pushAn" />
            </label>
          </div>
          <div class="row2">
            <label class="fld">
              <span class="lbl">频率（每 N 条消息）</span>
              <input v-model.number="anInterval" class="in" type="number" min="1" max="99" :disabled="chat.streaming" @change="pushAn" />
            </label>
            <label class="fld">
              <span class="lbl">消息角色</span>
              <select v-model.number="anRole" class="in" :disabled="chat.streaming || anPosition !== 1" @change="pushAn">
                <option :value="0">system</option>
                <option :value="1">user</option>
                <option :value="2">assistant</option>
              </select>
            </label>
          </div>
          <p class="hint">修改即写盘并即时生效；深度/角色仅「聊天内 @Depth」位置生效</p>
        </template>
        <p v-else class="hint">
          按固定频率向对话注入的提示（ST 经典用法：防出戏提醒、文风约束等）
        </p>

        <!-- 自动记忆（Summarize） -->
        <div class="sep-line" />
        <div class="fld">
          <span class="lbl">自动记忆（Summarize）</span>
          <span class="val summary">{{ chat.summary || '暂无摘要' }}</span>
        </div>
        <div class="btns">
          <button class="btn-go" :disabled="chat.streaming || !chat.currentFile" @click="chat.updateSummary()">
            {{ chat.streaming ? '生成中…' : '更新摘要' }}
          </button>
        </div>
        <p class="hint">摘要写入会话文件（chat_metadata.summary）；频率/注入深度在「设置 → 通用 → 插件功能」配置</p>
      </div>

      <!-- 世界书 -->
      <div v-else-if="tab === 'wi'" class="body">
        <div class="fld">
          <span class="lbl">角色卡绑定</span>
          <span class="val">{{ boundWorld || '未绑定世界书' }}</span>
        </div>
        <div class="fld">
          <span class="lbl">最近一次注入</span>
          <span class="val mono">{{ wiCount ? `${wiCount} 条` : '—' }}</span>
        </div>
        <p class="hint">条目编辑请到世界书页（本面板只读展示，避免误改共享数据）</p>
        <button class="btn-go" @click="goWorlds">去世界书页管理 →</button>
      </div>
    </div>
  </aside>
</template>

<style scoped>
/* 推挤式：宽度过渡挤压聊天列；内容固定宽防压缩变形 */
.panel {
  width: 0;
  flex: none;
  overflow: hidden;
  transition: width 0.22s ease;
  background: var(--c-bg);
}
.panel.open {
  width: 300px;
  border-left: 1px solid var(--c-border);
}
.inner {
  width: 300px;
  height: 100%;
  display: flex;
  flex-direction: column;
}
.tabs {
  flex: none;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 10px 10px 0;
}
.t {
  padding: 5px 12px;
  font-size: 12px;
  color: var(--c-text-2);
  border-radius: 99px;
  transition: background 0.15s;
}
.t:hover {
  background: var(--c-panel);
}
.t.on {
  color: var(--p-600);
  background: var(--p-50);
  font-weight: 600;
}
.x {
  margin-left: auto;
  width: 26px;
  height: 26px;
  font-size: 15px;
  color: var(--c-text-3);
  border-radius: var(--radius-md);
}
.x:hover {
  background: var(--c-panel);
  color: var(--c-text);
}
.body {
  flex: 1;
  overflow-y: auto;
  padding: 14px 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.fld {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}
.lbl {
  font-size: 11px;
  color: var(--c-text-3);
}
.in {
  padding: 6px 8px;
  font-size: 12px;
  color: var(--c-text);
  background: var(--c-canvas);
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  width: 100%;
}
.in:focus {
  outline: none;
  border-color: var(--p-400);
}
.row2 {
  display: flex;
  gap: 12px;
}
.row2 .fld {
  flex: 1;
}
.val {
  font-size: 12px;
  color: var(--c-text);
  padding: 2px 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mono {
  font-family: var(--font-mono);
}
.hint {
  font-size: 11px;
  color: var(--c-text-3);
  margin: -6px 0 0;
}
/* 表情立绘 */
.sprite-row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 6px;
}
.sprite {
  width: 56px;
  height: 56px;
  object-fit: cover;
  border-radius: var(--radius-md);
  border: 2px solid transparent;
  cursor: pointer;
  background: var(--c-panel);
}
.sprite.on {
  border-color: var(--p-500);
}
.switch-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--c-text);
  cursor: pointer;
}
.switch-row .sub {
  color: var(--c-text-3);
  font-size: 11px;
}
.stat {
  background: var(--c-canvas);
  border-radius: var(--radius-lg);
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.stat-t {
  font-size: 11px;
  color: var(--c-text-3);
  margin: 0 0 2px;
}
.stat-r {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  color: var(--c-text-2);
}
.stat-r .mono {
  color: var(--c-text);
}
.stat-note {
  font-size: 10px;
  color: var(--c-text-3);
  margin: 4px 0 0;
}
.btn-go {
  align-self: flex-start;
  padding: 6px 12px;
  font-size: 12px;
  color: var(--p-600);
  border: 1px solid var(--p-200);
  border-radius: var(--radius-md);
  transition: background 0.15s;
}
.btn-go:hover {
  background: var(--p-50);
}
.sep-line {
  height: 1px;
  background: var(--c-border);
  margin: 4px 0;
}
.summary {
  font-size: 11px;
  line-height: 1.7;
  color: var(--c-text-2);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 140px;
  overflow-y: auto;
}
input[type='range'] {
  accent-color: var(--p-500);
}
</style>
