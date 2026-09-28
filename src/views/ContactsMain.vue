<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useCharacterStore, type CharacterEditPatch } from '@/stores/character'
import { useChatStore } from '@/stores/chat'
import Avatar from '@/components/Avatar.vue'

const router = useRouter()
const chars = useCharacterStore()
const chat = useChatStore()

const char = computed(() => chars.detail ?? chars.current)

const editing = ref(false)
const form = ref<CharacterEditPatch>({})
const msg = ref('')

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
  }
}
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
</style>
