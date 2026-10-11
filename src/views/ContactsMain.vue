<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useCharacterStore, type CharacterEditPatch } from '@/stores/character'
import { useChatStore } from '@/stores/chat'
import { useWorldsStore } from '@/stores/worlds'
import Avatar from '@/components/Avatar.vue'

const router = useRouter()
const chars = useCharacterStore()
const chat = useChatStore()
const worlds = useWorldsStore()

const char = computed(() => chars.detail ?? chars.current)

const editing = ref(false)
const form = ref<CharacterEditPatch>({})
const msg = ref('')

/* Mod 世界书选择器需要世界书列表（进编辑态时确保已加载） */
watch(editing, (v) => {
  if (v && !worlds.loaded) void worlds.load()
})

function fillFromCard() {
  const c = char.value
  form.value = {
    name: c?.name ?? '',
    description: c?.description ?? '',
    personality: c?.personality ?? '',
    scenario: c?.scenario ?? '',
    first_mes: c?.first_mes ?? '',
    mes_example: c?.mes_example ?? '',
    creator_notes: String(c?.creatorcomment ?? ''),
    tags: [...(c?.tags ?? [])],
    creator: typeof c?.creator === 'string' ? c.creator : '',
    characterVersion:
      typeof c?.character_version === 'string'
        ? c.character_version
        : typeof c?.data?.character_version === 'string'
          ? c.data.character_version
          : '',
    alternateGreetings: Array.isArray(c?.data?.alternate_greetings)
      ? (c.data.alternate_greetings as string[]).filter((x) => typeof x === 'string')
      : Array.isArray(c?.alternate_greetings)
        ? (c.alternate_greetings as string[]).filter((x) => typeof x === 'string')
        : [],
    detailedIntro: detailedIntroOf(c),
    modPrompt: modPromptOf(c),
    modWorld: modWorldOf(c),
    css: cardCssOf(c),
    bgm: cardBgmOf(c),
  }
}

/** 风月详细介绍（data.extensions.detailed_intro，HTML+JS） */
function detailedIntroOf(c: typeof chars.detail): string {
  const ext = c?.data?.extensions as { detailed_intro?: unknown } | undefined
  return typeof ext?.detailed_intro === 'string' ? ext.detailed_intro : ''
}

/** 风月 Mod 层提示词（data.extensions.mod_prompt，作者级 System 注入） */
function modPromptOf(c: typeof chars.detail): string {
  const ext = c?.data?.extensions as { mod_prompt?: unknown } | undefined
  return typeof ext?.mod_prompt === 'string' ? ext.mod_prompt : ''
}

/** 风月 Mod 层挂载的世界书（data.extensions.mod_world） */
function modWorldOf(c: typeof chars.detail): string {
  const ext = c?.data?.extensions as { mod_world?: unknown } | undefined
  return typeof ext?.mod_world === 'string' ? ext.mod_world : ''
}

/** 风月内置 CSS / BGM（data.extensions.css / bgm，作品级默认，会话级可覆盖） */
function cardCssOf(c: typeof chars.detail): string {
  const ext = c?.data?.extensions as { css?: unknown } | undefined
  return typeof ext?.css === 'string' ? ext.css : ''
}
function cardBgmOf(c: typeof chars.detail): string {
  const ext = c?.data?.extensions as { bgm?: unknown } | undefined
  return typeof ext?.bgm === 'string' ? ext.bgm : ''
}

const intro = computed(() => detailedIntroOf(char.value))
/** 沙箱预览开关（iframe sandbox=allow-scripts：可跑 JS，无同源/存储/顶层导航权限） */
const introOpen = ref(false)
watch(char, fillFromCard, { immediate: true })

const tagsText = computed({
  get: () => (form.value.tags ?? []).join(', '),
  set: (v: string) =>
    (form.value.tags = v
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)),
})

/** 备选开场白：编辑态一行一条 ↔ 数组 */
const altGreetingsText = computed({
  get: () => (form.value.alternateGreetings ?? []).join('\n'),
  set: (v: string) =>
    (form.value.alternateGreetings = v
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean)),
})

/** 展开高级字段区 */
const advOpen = ref(false)

function startEdit() {
  msg.value = ''
  fillFromCard()
  editing.value = true
}

function cancel() {
  editing.value = false
  msg.value = ''
  fillFromCard()
}

async function save() {
  msg.value = ''
  try {
    await chars.saveCard(form.value)
    editing.value = false
    msg.value = '已保存到角色卡'
  } catch (e) {
    msg.value = `保存失败：${e instanceof Error ? e.message : String(e)}`
  }
}

async function startChat() {
  const c = char.value
  if (!c) return
  await chat.openWithCharacter(c)
  await router.push('/chats')
}

/* ---------- 中栏右键菜单的「编辑」入口：选中角色后请求进入编辑态 ---------- */
watch(
  () => chars.editRequested,
  (v) => {
    if (!v) return
    chars.editRequested = false
    startEdit()
  },
)

/** 只读信息（不可编辑，来自卡片元数据） */
const meta = computed(() => {
  const c = char.value
  if (!c) return []
  return [
    { k: '头像文件', v: c.avatar },
    { k: '卡规范', v: c.spec ? `${c.spec} ${c.spec_version ?? ''}`.trim() : '—' },
    { k: '创建时间', v: c.create_date || '—' },
    { k: '最近会话', v: c.chat || '—' },
    {
      k: '作者',
      v:
        typeof c.creator === 'string'
          ? c.creator
          : typeof c.data?.creator === 'string'
            ? c.data.creator
            : '—',
    },
    {
      k: '卡片版本',
      v:
        typeof c.character_version === 'string'
          ? c.character_version
          : typeof c.data?.character_version === 'string'
            ? c.data.character_version
            : '—',
    },
    {
      k: '备选开场白',
      v: `${Array.isArray(c.data?.alternate_greetings) ? c.data.alternate_greetings.length : Array.isArray(c.alternate_greetings) ? c.alternate_greetings.length : 0} 条`,
    },
    { k: '健谈度', v: String(c.talkativeness ?? '—') },
  ]
})
</script>

<template>
  <main class="main page" data-page="contacts">
    <!-- 未选中 -->
    <div v-if="!char" class="empty">
      <div class="big">
        <svg
          viewBox="0 0 24 24"
          width="30"
          height="30"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path
            d="M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zm-7 9c0-3.9 3.1-7 7-7s7 3.1 7 7M17 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm4.5 11c0-2.5-2-4.5-4.5-4.5-.8 0-1.5.2-2.1.5"
          />
        </svg>
      </div>
      <p>未选择角色卡</p>
    </div>

    <template v-else>
      <header class="head">
        <Avatar class="ava" :avatar="char.avatar" :name="char.name" />
        <div class="who">
          <div class="nm">
            {{ char.name }}
            <span v-if="char.fav" class="fav" title="收藏">
              <svg viewBox="0 0 24 24" width="11" height="11" fill="currentColor">
                <path
                  d="M12 3.6l2.7 5.5 6 .9-4.3 4.2 1 6-5.4-2.9-5.4 2.9 1-6L3.3 10l6-.9z"
                />
              </svg>
            </span>
          </div>
          <div class="tags">
            <span v-for="t in char.tags ?? []" :key="t" class="tg">{{ t }}</span>
            <span v-if="!(char.tags ?? []).length" class="tg none">无标签</span>
          </div>
        </div>
        <div class="acts">
          <template v-if="!editing">
            <button v-if="intro" class="btn" @click="introOpen = true">详细介绍</button>
            <button class="btn btn-primary" @click="startChat">发消息</button>
          </template>
          <template v-else>
            <button class="btn" :disabled="chars.saving" @click="cancel">取消</button>
            <button class="btn btn-primary" :disabled="chars.saving" @click="save">
              {{ chars.saving ? '保存中…' : '保存' }}
            </button>
          </template>
        </div>
      </header>

      <div class="body">
        <p v-if="msg" class="notice">{{ msg }}</p>

        <!-- 编辑态 -->
        <template v-if="editing">
          <label class="fld">
            <span class="lb">名称</span>
            <input v-model="form.name" class="field" type="text" />
          </label>
          <label class="fld">
            <span class="lb">标签（逗号分隔）</span>
            <input v-model="tagsText" class="field" type="text" placeholder="fantasy, female" />
          </label>
          <label class="fld">
            <span class="lb">描述 Description</span>
            <textarea v-model="form.description" class="field field-area" rows="8" />
          </label>
          <label class="fld">
            <span class="lb">性格 Personality</span>
            <textarea v-model="form.personality" class="field field-area" rows="4" />
          </label>
          <label class="fld">
            <span class="lb">场景 Scenario</span>
            <textarea v-model="form.scenario" class="field field-area" rows="4" />
          </label>
          <label class="fld">
            <span class="lb">开场白 First message</span>
            <textarea v-model="form.first_mes" class="field field-area" rows="6" />
          </label>
          <label class="fld">
            <span class="lb">对话示例 Example dialogue</span>
            <textarea v-model="form.mes_example" class="field field-area" rows="6" />
          </label>
          <label class="fld">
            <span class="lb">创作者备注</span>
            <textarea v-model="form.creator_notes" class="field field-area" rows="3" />
          </label>

          <!-- 高级字段（v2/v3 spec） -->
          <button class="adv-toggle" type="button" @click="advOpen = !advOpen">
            高级字段 {{ advOpen ? '▾' : '▸' }}
          </button>
          <template v-if="advOpen">
            <div class="grid2">
              <label class="fld">
                <span class="lb">作者 Creator</span>
                <input v-model="form.creator" class="field" type="text" placeholder="卡作者署名" />
              </label>
              <label class="fld">
                <span class="lb">卡片版本 Version</span>
                <input v-model="form.characterVersion" class="field" type="text" placeholder="如 1.0" />
              </label>
            </div>
            <label class="fld">
              <span class="lb">备选开场白（一行一条，Swipe 备选用）</span>
              <textarea v-model="altGreetingsText" class="field field-area" rows="4" />
            </label>
            <label class="fld">
              <span class="lb">详细介绍（风月创作：HTML+JS，保存后在沙箱中渲染预览）</span>
              <textarea v-model="form.detailedIntro" class="field field-area" rows="8" spellcheck="false" />
            </label>
            <label class="fld">
              <span class="lb">Mod 提示词（风月作者层：注入 System，紧跟作品提示词之后）</span>
              <textarea v-model="form.modPrompt" class="field field-area" rows="5" spellcheck="false" />
            </label>
            <label class="fld">
              <span class="lb">Mod 世界书（风月作者层：条目参与世界书扫描）</span>
              <select v-model="form.modWorld" class="field">
                <option v-if="!worlds.loaded && form.modWorld" :value="form.modWorld" disabled>世界书列表加载中…</option>
                <option value="">不挂载</option>
                <option v-for="w in worlds.list" :key="w.file_id" :value="w.file_id">{{ w.name || w.file_id }}</option>
              </select>
            </label>
            <label class="fld">
              <span class="lb">内置 CSS（作品级聊天页美化：气泡/状态栏/输入区，用 #customized-* 钩子定位；会话级可覆盖）</span>
              <textarea v-model="form.css" class="field field-area" rows="6" spellcheck="false" />
            </label>
            <label class="fld">
              <span class="lb">BGM（音乐 URL，进入会话后顶栏出现播放开关）</span>
              <input v-model="form.bgm" class="field" type="text" placeholder="https://…/bgm.mp3" spellcheck="false" />
            </label>
          </template>

          <p class="warn">保存将重写角色卡文件，不影响已有会话。</p>
        </template>

        <!-- 只读态 -->
        <template v-else>
          <section class="sec">
            <h4>描述 Description</h4>
            <pre class="txt">{{ char.description || '—' }}</pre>
          </section>
          <section v-if="char.personality" class="sec">
            <h4>性格 Personality</h4>
            <pre class="txt">{{ char.personality }}</pre>
          </section>
          <section v-if="char.scenario" class="sec">
            <h4>场景 Scenario</h4>
            <pre class="txt">{{ char.scenario }}</pre>
          </section>
          <section v-if="char.first_mes" class="sec">
            <h4>开场白</h4>
            <pre class="txt">{{ char.first_mes }}</pre>
          </section>
          <section v-if="char.mes_example" class="sec">
            <h4>对话示例</h4>
            <pre class="txt">{{ char.mes_example }}</pre>
          </section>
          <section v-if="char.creatorcomment" class="sec">
            <h4>创作者备注</h4>
            <pre class="txt">{{ char.creatorcomment }}</pre>
          </section>

          <section class="sec">
            <h4>卡片信息</h4>
            <table class="kv">
              <tbody>
                <tr v-for="r in meta" :key="r.k">
                  <td class="k">{{ r.k }}</td>
                  <td class="v">{{ r.v }}</td>
                </tr>
              </tbody>
            </table>
          </section>
        </template>
      </div>

      <!-- 详细介绍：iframe 沙箱渲染（风月语义：可跑 JS/完整 CSS，无同源与顶层导航权限） -->
      <Teleport to="body">
        <div v-if="introOpen" class="intro-mask" @click.self="introOpen = false">
          <div class="intro-dlg" role="dialog" aria-modal="true">
            <header class="intro-head">
              <span class="intro-tt">{{ char.name }} · 详细介绍</span>
              <button class="intro-x" title="关闭" @click="introOpen = false">×</button>
            </header>
            <iframe
              class="intro-frame"
              sandbox="allow-scripts"
              :srcdoc="intro"
            />
          </div>
        </div>
      </Teleport>
    </template>
  </main>
</template>

<style scoped>
.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  /* 背景交给全局 .page[data-page]（见 base.css），此处不再重复声明 */
}
.empty {
  margin: auto;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  color: var(--c-text-3);
  font-size: 12px;
}
.empty .big {
  font-size: 30px;
}
.head {
  flex: none;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 14px 18px;
  border-bottom: 1px solid var(--c-border);
}
.head .ava {
  width: 48px;
  height: 48px;
  border-radius: 50%;
  object-fit: cover;
  background: var(--c-panel);
}
.who {
  min-width: 0;
}
.nm {
  font-size: 15px;
  font-weight: 600;
}
.fav {
  font-size: 11px;
  color: var(--s-warning);
}
.tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 3px;
}
.tg {
  font-size: 10px;
  padding: 1px 7px;
  border-radius: 9px;
  background: var(--c-panel);
  color: var(--c-text-2);
}
.tg.none {
  color: var(--c-text-3);
}
.acts {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 7px;
}
.primary:hover:not(:disabled) {
  background: var(--p-600);
}
.primary:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.ghost:hover:not(:disabled) {
  border-color: var(--p-400);
  color: var(--p-600);
}

.body {
  flex: 1;
  overflow-y: auto;
  padding: 14px 18px 24px;
}
.notice {
  margin-bottom: 10px;
  padding: 7px 11px;
  font-size: 12px;
  border-radius: var(--radius-md);
  background: var(--s-success-bg);
  color: var(--s-success);
}
.warn {
  margin-top: 4px;
  font-size: 11px;
  color: var(--s-warning);
}
/* 高级字段折叠区 */
.adv-toggle {
  margin: 2px 0 10px;
  padding: 4px 8px;
  font-size: 11px;
  font-weight: 600;
  color: var(--c-text-2);
  background: var(--c-panel);
  border-radius: var(--radius-md);
  width: fit-content;
  cursor: pointer;
}
.adv-toggle:hover {
  color: var(--p-600);
}
.grid2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0 12px;
}
.sec {
  margin-bottom: 16px;
}
.sec h4 {
  font-size: 12px;
  font-weight: 600;
  color: var(--c-text-2);
  margin-bottom: 5px;
}
.txt {
  margin: 0;
  padding: 9px 11px;
  font-family: inherit;
  font-size: 12px;
  line-height: 1.7;
  white-space: pre-wrap;
  word-break: break-word;
  background: var(--c-canvas);
  border-radius: var(--radius-md);
  color: var(--c-text);
  max-height: 320px;
  overflow-y: auto;
}
.lb {
  display: block;
  font-size: 11px;
  font-weight: 600;
  color: var(--c-text-2);
  margin-bottom: 4px;
}
.in,
.in:focus,
.ta:focus {
  border-color: var(--p-400);
}
.kv {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.kv td {
  padding: 5px 8px;
  border-bottom: 1px solid var(--c-border);
}
.k {
  width: 110px;
  color: var(--c-text-3);
}
.v {
  color: var(--c-text);
  word-break: break-all;
}
/* 详细介绍沙箱遮罩 */
.intro-mask {
  position: fixed;
  inset: 0;
  z-index: 90;
  display: grid;
  place-items: center;
  padding: 24px;
  background: rgb(0 0 0 / 55%);
}
.intro-dlg {
  display: flex;
  flex-direction: column;
  width: min(860px, 100%);
  height: min(86vh, 900px);
  border-radius: var(--radius-lg);
  background: var(--c-bg);
  border: 1px solid var(--c-border);
  overflow: hidden;
}
.intro-head {
  flex: none;
  display: flex;
  align-items: center;
  padding: 10px 14px;
  border-bottom: 1px solid var(--c-border);
}
.intro-tt {
  font-size: 13px;
  font-weight: 600;
}
.intro-x {
  margin-left: auto;
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border-radius: var(--radius-sm);
  color: var(--c-text-3);
  font-size: 15px;
}
.intro-x:hover {
  background: var(--c-panel);
  color: var(--c-text);
}
.intro-frame {
  flex: 1;
  width: 100%;
  border: 0;
  background: #fff;
}
</style>
