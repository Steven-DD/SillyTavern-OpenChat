<script setup lang="ts">
/**
 * 世界书详情（右栏）—— 条目编辑器
 *
 * 世界书条目编辑器（全量字段）：
 * 基本（关键词/内容/常驻/选择性/位置/顺序/概率/深度）+ 高级（递归控制、inclusion group、
 * timedEffects、扫描来源扩展、匹配选项等）。
 * 保存策略：整本写回 /api/worldinfo/edit，改动防抖 800ms 自动保存。
 * 删除世界书入口在中栏列表右键菜单。
 */
import { computed, ref, watch } from 'vue'
import {
  useWorldsStore,
  WI_LOGIC,
  WI_POSITION,
  WI_ROLE,
} from '@/stores/worlds'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import type { StWorldEntry } from '@/services/st/types'

const worlds = useWorldsStore()

/** 递归扫描开关（localStorage，worldinfo.ts g.recursive） */
const recursive = ref(localStorage.getItem('app.wi.recursive') === '1')
watch(recursive, (v) => {
  localStorage.setItem('app.wi.recursive', v ? '1' : '0')
})

const book = computed(() => worlds.detail)
const entryCount = computed(() => Object.keys(book.value?.entries ?? {}).length)

const saveText = computed(() => {
  if (worlds.saving) return '保存中…'
  if (worlds.dirty) return '未保存（自动保存中）'
  return '已保存'
})

/* ---- 展开态：单条展开编辑（点击标题切换） ---- */
const expandedUid = ref<string | null>(null)

function toggleExpand(uid: string) {
  expandedUid.value = expandedUid.value === uid ? null : uid
}

/* ---- 枚举文案 ---- */
const POSITION_LABELS: Record<number, string> = {
  [WI_POSITION.before]: '角色定义前',
  [WI_POSITION.after]: '角色定义后',
  [WI_POSITION.ANTop]: '作者注释前',
  [WI_POSITION.ANBottom]: '作者注释后',
  [WI_POSITION.atDepth]: '@Depth 深度插入',
  [WI_POSITION.EMTop]: '作者注释顶（EM）',
  [WI_POSITION.EMBottom]: '作者注释底（EM）',
  [WI_POSITION.outlet]: 'Outlet 插槽',
}
const LOGIC_LABELS: Record<number, string> = {
  [WI_LOGIC.AND_ANY]: '与任一（AND_ANY）',
  [WI_LOGIC.NOT_ALL]: '非全有（NOT_ALL）',
  [WI_LOGIC.NOT_ANY]: '非任一（NOT_ANY）',
  [WI_LOGIC.AND_ALL]: '与全有（AND_ALL）',
}
const ROLE_LABELS: Record<number, string> = {
  [WI_ROLE.system]: 'system',
  [WI_ROLE.user]: 'user',
  [WI_ROLE.assistant]: 'assistant',
}

function positionText(p: number | undefined): string {
  return POSITION_LABELS[p ?? WI_POSITION.before] ?? '角色定义前'
}

function entryTitle(e: StWorldEntry): string {
  return e.comment?.trim() || e.key.join('、') || `条目 ${e.uid}`
}

/* ---- 编辑提交辅助 ---- */
type AnyEntry = Partial<StWorldEntry>

function patch(uid: string, p: AnyEntry) {
  worlds.patchEntry(uid, p)
}

/** 逗号分隔文本 ↔ 关键词数组 */
function toCsv(v: unknown): string {
  return Array.isArray(v) ? v.join(', ') : ''
}
function fromCsv(v: string): string[] {
  return v
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
}

/** 整数输入：空串 → undefined（不写），非法 → 保留默认 */
function intVal(el: HTMLInputElement): number | undefined {
  const n = Number(el.value)
  return el.value.trim() === '' || !Number.isFinite(n) ? undefined : Math.trunc(n)
}

/** 可空整数（scanDepth/sticky 等）：空串 → null */
function nullableInt(el: HTMLInputElement): number | null {
  const n = Number(el.value)
  return el.value.trim() === '' || !Number.isFinite(n) ? null : Math.trunc(n)
}

/** 三态布尔 select：'' = 跟随全局（null） */
function triKey(v: boolean | null | undefined): string {
  return v === null || v === undefined ? '' : v ? '1' : '0'
}
function triVal(el: HTMLSelectElement): boolean | null {
  return el.value === '' ? null : el.value === '1'
}

/* ---- 条目操作 ---- */
function addEntry() {
  const e = worlds.addEntry()
  if (e) expandedUid.value = String(e.uid)
}

/** 待删除条目（非 null = 弹确认窗） */
const delTarget = ref<StWorldEntry | null>(null)

function confirmDeleteEntry(e: StWorldEntry) {
  delTarget.value = e
}

async function doDeleteEntry() {
  const e = delTarget.value
  if (!e) return
  worlds.removeEntry(e.uid)
  if (expandedUid.value === String(e.uid)) expandedUid.value = null
  delTarget.value = null
}
</script>

<template>
  <main class="main page" data-page="worlds">
    <!-- 未选中 -->
    <div v-if="!worlds.current" class="empty">
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
          <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2zM22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z" />
        </svg>
      </div>
      <p>未选择世界书</p>
    </div>

    <template v-else>
      <header class="head">
        <div class="who">
          <div class="nm">{{ worlds.current.name }}</div>
          <div class="sub">
            {{ entryCount }} 个条目 ·
            <span :class="{ unsaved: worlds.dirty || worlds.saving }">{{ saveText }}</span>
          </div>
        </div>
        <div class="acts">
          <button class="btn btn-primary" @click="addEntry">＋ 新增条目</button>
        </div>
      </header>
      <p v-if="worlds.error" class="save-err">{{ worlds.error }}</p>

      <div class="body">
        <!-- 递归扫描开关（ST 默认关，worldinfo.ts g.recursive） -->
        <section class="sec">
          <label class="chk">
            <input type="checkbox" v-model="recursive" />
            递归扫描（Recursive Scan，默认关闭）
          </label>
          <p class="adv-t">开启后，已激活条目的内容会参与后续轮次的关键词扫描（最多 2 轮），可能激活关联条目。</p>
        </section>

        <div v-if="worlds.detailLoading" class="empty-hint">加载条目中…</div>

        <template v-else>
          <div v-if="!worlds.entries.length" class="empty-hint">
            这本世界书还没有条目，点右上角「新增条目」开始
          </div>

          <section
            v-for="(e, i) in worlds.entries"
            :key="e.uid"
            class="ent"
            :class="{ off: e.data.disable }"
          >
            <!-- 收起态：标题 + 概览徽标 -->
            <div class="en-head" role="button" tabindex="0" @click="toggleExpand(e.uid)" @keydown.enter="toggleExpand(e.uid)">
              <span class="chev" :class="{ open: expandedUid === e.uid }">▸</span>
              <span class="en-idx">{{ i + 1 }}</span>
              <span class="en-title">{{ entryTitle(e.data) }}</span>
              <span v-if="e.data.constant" class="bd const">常驻</span>
              <span v-if="e.data.disable" class="bd off">已停用</span>
              <span v-if="e.data.useProbability && (e.data.probability ?? 100) < 100" class="bd pos"
                >{{ e.data.probability }}%</span
              >
              <span class="bd pos">{{ positionText(e.data.position) }}</span>
              <span class="bd ord">#{{ e.data.order ?? 100 }}</span>
            </div>

            <!-- 展开态：全字段编辑 -->
            <div v-if="expandedUid === e.uid" class="en-form">
              <div class="row3">
                <label class="fld">
                  <span class="lb">启用</span>
                  <input
                    type="checkbox"
                    :checked="!e.data.disable"
                    @change="patch(e.uid, { disable: !($event.target as HTMLInputElement).checked })"
                  />
                </label>
                <label class="fld">
                  <span class="lb">常驻（不依赖关键词）</span>
                  <input
                    type="checkbox"
                    :checked="!!e.data.constant"
                    @change="patch(e.uid, { constant: ($event.target as HTMLInputElement).checked })"
                  />
                </label>
                <label class="fld">
                  <span class="lb">选择性（用次级关键词）</span>
                  <input
                    type="checkbox"
                    :checked="e.data.selective !== false"
                    @change="patch(e.uid, { selective: ($event.target as HTMLInputElement).checked })"
                  />
                </label>
              </div>

              <label class="fld">
                <span class="lb">标题 / 备注</span>
                <input
                  class="field"
                  type="text"
                  :value="e.data.comment ?? ''"
                  @change="patch(e.uid, { comment: ($event.target as HTMLInputElement).value, addMemo: true })"
                />
              </label>

              <label class="fld">
                <span class="lb">主关键词（逗号分隔，支持 /正则/）</span>
                <input
                  class="field"
                  type="text"
                  :value="toCsv(e.data.key)"
                  @change="patch(e.uid, { key: fromCsv(($event.target as HTMLInputElement).value) })"
                />
              </label>

              <label class="fld">
                <span class="lb">次级关键词（逗号分隔）</span>
                <input
                  class="field"
                  type="text"
                  :disabled="!e.data.selective"
                  :value="toCsv(e.data.keysecondary)"
                  @change="patch(e.uid, { keysecondary: fromCsv(($event.target as HTMLInputElement).value) })"
                />
              </label>

              <label v-if="e.data.selective" class="fld">
                <span class="lb">次级关键词逻辑</span>
                <select
                  class="field"
                  :value="e.data.selectiveLogic ?? WI_LOGIC.AND_ANY"
                  @change="patch(e.uid, { selectiveLogic: Number(($event.target as HTMLSelectElement).value) })"
                >
                  <option v-for="(lb, k) in LOGIC_LABELS" :key="k" :value="Number(k)">{{ lb }}</option>
                </select>
              </label>

              <label class="fld">
                <span class="lb">内容（注入 prompt 的文本）</span>
                <textarea
                  class="field field-area"
                  rows="5"
                  :value="e.data.content ?? ''"
                  @change="patch(e.uid, { content: ($event.target as HTMLTextAreaElement).value })"
                ></textarea>
              </label>

              <div class="row3">
                <label class="fld">
                  <span class="lb">注入位置</span>
                  <select
                    class="field"
                    :value="e.data.position ?? WI_POSITION.before"
                    @change="patch(e.uid, { position: Number(($event.target as HTMLSelectElement).value) })"
                  >
                    <option v-for="(lb, k) in POSITION_LABELS" :key="k" :value="Number(k)">{{ lb }}</option>
                  </select>
                </label>
                <label class="fld">
                  <span class="lb">顺序（越大越靠前）</span>
                  <input
                    class="field"
                    type="number"
                    :value="e.data.order ?? 100"
                    @change="patch(e.uid, { order: intVal($event.target as HTMLInputElement) ?? 100 })"
                  />
                </label>
                <label class="fld">
                  <span class="lb">触发概率 %</span>
                  <span class="pair">
                    <input
                      class="field"
                      type="number"
                      min="0"
                      max="100"
                      :disabled="!e.data.useProbability"
                      :value="e.data.probability ?? 100"
                      @change="patch(e.uid, { probability: intVal($event.target as HTMLInputElement) ?? 100 })"
                    />
                    <input
                      type="checkbox"
                      title="启用概率"
                      :checked="e.data.useProbability !== false"
                      @change="patch(e.uid, { useProbability: ($event.target as HTMLInputElement).checked })"
                    />
                  </span>
                </label>
              </div>

              <div v-if="e.data.position === WI_POSITION.atDepth" class="row3">
                <label class="fld">
                  <span class="lb">深度（距末尾消息数）</span>
                  <input
                    class="field"
                    type="number"
                    min="0"
                    :value="e.data.depth ?? 4"
                    @change="patch(e.uid, { depth: intVal($event.target as HTMLInputElement) ?? 4 })"
                  />
                </label>
                <label class="fld">
                  <span class="lb">消息角色</span>
                  <select
                    class="field"
                    :value="e.data.role ?? WI_ROLE.system"
                    @change="patch(e.uid, { role: Number(($event.target as HTMLSelectElement).value) })"
                  >
                    <option v-for="(lb, k) in ROLE_LABELS" :key="k" :value="Number(k)">{{ lb }}</option>
                  </select>
                </label>
              </div>

              <label v-if="e.data.position === WI_POSITION.outlet" class="fld">
                <span class="lb">Outlet 名称</span>
                <input
                  class="field"
                  type="text"
                  :value="e.data.outletName ?? ''"
                  @change="patch(e.uid, { outletName: ($event.target as HTMLInputElement).value })"
                />
              </label>

              <!-- 高级字段 -->
              <details class="adv">
                <summary>高级选项</summary>

                <div class="row3">
                  <label class="fld">
                    <span class="lb">扫描深度（空 = 全局）</span>
                    <input
                      class="field"
                      type="number"
                      min="0"
                      :value="e.data.scanDepth ?? ''"
                      @change="patch(e.uid, { scanDepth: nullableInt($event.target as HTMLInputElement) })"
                    />
                  </label>
                  <label class="fld">
                    <span class="lb">区分大小写</span>
                    <select class="field" :value="triKey(e.data.caseSensitive)" @change="patch(e.uid, { caseSensitive: triVal($event.target as HTMLSelectElement) })">
                      <option value="">跟随全局</option>
                      <option value="1">是</option>
                      <option value="0">否</option>
                    </select>
                  </label>
                  <label class="fld">
                    <span class="lb">全词匹配</span>
                    <select class="field" :value="triKey(e.data.matchWholeWords)" @change="patch(e.uid, { matchWholeWords: triVal($event.target as HTMLSelectElement) })">
                      <option value="">跟随全局</option>
                      <option value="1">是</option>
                      <option value="0">否</option>
                    </select>
                  </label>
                </div>

                <div class="row3">
                  <label class="fld">
                    <span class="lb">分组（同组掷骰激活一个）</span>
                    <input
                      class="field"
                      type="text"
                      :value="e.data.group ?? ''"
                      @change="patch(e.uid, { group: ($event.target as HTMLInputElement).value })"
                    />
                  </label>
                  <label class="fld">
                    <span class="lb">组权重</span>
                    <input
                      class="field"
                      type="number"
                      min="0"
                      :disabled="!e.data.group"
                      :value="e.data.groupWeight ?? 100"
                      @change="patch(e.uid, { groupWeight: intVal($event.target as HTMLInputElement) ?? 100 })"
                    />
                  </label>
                  <label class="fld">
                    <span class="lb">组评分</span>
                    <select class="field" :value="triKey(e.data.useGroupScoring)" @change="patch(e.uid, { useGroupScoring: triVal($event.target as HTMLSelectElement) })">
                      <option value="">跟随全局</option>
                      <option value="1">是</option>
                      <option value="0">否</option>
                    </select>
                  </label>
                </div>
                <label class="fld">
                  <span class="lb">覆盖同组其它条目（groupOverride）</span>
                  <input
                    type="checkbox"
                    :checked="!!e.data.groupOverride"
                    @change="patch(e.uid, { groupOverride: ($event.target as HTMLInputElement).checked })"
                  />
                </label>

                <div class="row3">
                  <label class="fld">
                    <span class="lb">激活后保持 N 步（sticky）</span>
                    <input
                      class="field"
                      type="number"
                      min="0"
                      :value="e.data.sticky ?? ''"
                      @change="patch(e.uid, { sticky: nullableInt($event.target as HTMLInputElement) })"
                    />
                  </label>
                  <label class="fld">
                    <span class="lb">冷却 N 步（cooldown）</span>
                    <input
                      class="field"
                      type="number"
                      min="0"
                      :value="e.data.cooldown ?? ''"
                      @change="patch(e.uid, { cooldown: nullableInt($event.target as HTMLInputElement) })"
                    />
                  </label>
                  <label class="fld">
                    <span class="lb">延迟 N 步（delay）</span>
                    <input
                      class="field"
                      type="number"
                      min="0"
                      :value="e.data.delay ?? ''"
                      @change="patch(e.uid, { delay: nullableInt($event.target as HTMLInputElement) })"
                    />
                  </label>
                </div>
                <p class="adv-t">注：sticky 区间内强制激活；sticky 到期后立即进入 cooldown 跳过。</p>

                <div class="row3">
                  <label class="fld">
                    <span class="lb">不被递归激活</span>
                    <input
                      type="checkbox"
                      :checked="!!e.data.excludeRecursion"
                      @change="patch(e.uid, { excludeRecursion: ($event.target as HTMLInputElement).checked })"
                    />
                  </label>
                  <label class="fld">
                    <span class="lb">内容不触发递归</span>
                    <input
                      type="checkbox"
                      :checked="!!e.data.preventRecursion"
                      @change="patch(e.uid, { preventRecursion: ($event.target as HTMLInputElement).checked })"
                    />
                  </label>
                  <label class="fld">
                    <span class="lb">递归延迟步数</span>
                    <input
                      class="field"
                      type="number"
                      min="0"
                      :value="e.data.delayUntilRecursion ?? 0"
                      @change="patch(e.uid, { delayUntilRecursion: intVal($event.target as HTMLInputElement) ?? 0 })"
                    />
                  </label>
                </div>

                <div class="row3">
                  <label class="fld">
                    <span class="lb">忽略 token 预算</span>
                    <input
                      type="checkbox"
                      :checked="!!e.data.ignoreBudget"
                      @change="patch(e.uid, { ignoreBudget: ($event.target as HTMLInputElement).checked })"
                    />
                  </label>
                  <label class="fld">
                    <span class="lb">已向量化（供向量检索）</span>
                    <input
                      type="checkbox"
                      :checked="!!e.data.vectorized"
                      @change="patch(e.uid, { vectorized: ($event.target as HTMLInputElement).checked })"
                    />
                  </label>
                  <label class="fld">
                    <span class="lb">外部自动化 ID</span>
                    <input
                      class="field"
                      type="text"
                      :value="e.data.automationId ?? ''"
                      @change="patch(e.uid, { automationId: ($event.target as HTMLInputElement).value })"
                    />
                  </label>
                </div>

                <p class="adv-t">扫描范围扩展（把下列文本也纳入关键词扫描）</p>
                <div class="row3">
                  <label class="fld">
                    <span class="lb">用户人设</span>
                    <input type="checkbox" :checked="!!e.data.matchPersonaDescription" @change="patch(e.uid, { matchPersonaDescription: ($event.target as HTMLInputElement).checked })" />
                  </label>
                  <label class="fld">
                    <span class="lb">角色描述</span>
                    <input type="checkbox" :checked="!!e.data.matchCharacterDescription" @change="patch(e.uid, { matchCharacterDescription: ($event.target as HTMLInputElement).checked })" />
                  </label>
                  <label class="fld">
                    <span class="lb">角色性格</span>
                    <input type="checkbox" :checked="!!e.data.matchCharacterPersonality" @change="patch(e.uid, { matchCharacterPersonality: ($event.target as HTMLInputElement).checked })" />
                  </label>
                </div>
                <div class="row3">
                  <label class="fld">
                    <span class="lb">角色深度提示</span>
                    <input type="checkbox" :checked="!!e.data.matchCharacterDepthPrompt" @change="patch(e.uid, { matchCharacterDepthPrompt: ($event.target as HTMLInputElement).checked })" />
                  </label>
                  <label class="fld">
                    <span class="lb">场景</span>
                    <input type="checkbox" :checked="!!e.data.matchScenario" @change="patch(e.uid, { matchScenario: ($event.target as HTMLInputElement).checked })" />
                  </label>
                  <label class="fld">
                    <span class="lb">作者的话</span>
                    <input type="checkbox" :checked="!!e.data.matchCreatorNotes" @change="patch(e.uid, { matchCreatorNotes: ($event.target as HTMLInputElement).checked })" />
                  </label>
                </div>
              </details>

              <div class="en-ops">
                <button class="btn btn-sm" :disabled="i === 0" @click="worlds.moveEntry(e.uid, -1)">上移</button>
                <button class="btn btn-sm" :disabled="i === worlds.entries.length - 1" @click="worlds.moveEntry(e.uid, 1)">下移</button>
                <span class="spacer" />
                <button class="btn btn-sm danger-text" @click="confirmDeleteEntry(e.data)">删除条目</button>
              </div>
            </div>
          </section>
        </template>
      </div>
    </template>

    <!-- 删除条目确认 -->
    <ConfirmDialog
      :open="!!delTarget"
      title="删除条目"
      :message="delTarget ? `删除「${entryTitle(delTarget)}」？\n删除后不可恢复（保存后写入磁盘）。` : ''"
      confirm-text="删除"
      :busy="worlds.saving"
      @confirm="doDeleteEntry"
      @cancel="delTarget = null"
    />
  </main>
</template>

<style scoped>
.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--c-text-3);
  font-size: 13px;
}
.head {
  height: 56px;
  flex: none;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 16px;
  border-bottom: 1px solid var(--c-border);
}
.who {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.nm {
  font-size: 14px;
  font-weight: 600;
  color: var(--c-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sub {
  font-size: 11px;
  color: var(--c-text-3);
}
.sub .unsaved {
  color: var(--s-warning);
}
.save-err {
  margin: 6px 16px 0;
  font-size: 11px;
  color: var(--s-error);
  word-break: break-all;
}
.body {
  flex: 1;
  overflow-y: auto;
  padding: 12px 16px 24px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.empty-hint {
  flex: none;
  margin: 10px 4px;
  padding: 16px 10px;
  border: 1px dashed var(--c-border);
  border-radius: var(--radius-md);
  text-align: center;
  font-size: 12px;
  line-height: 1.7;
  color: var(--c-text-3);
}

/* 条目卡片。
   ⚠ 必须禁止 flex 收缩：.body 是 overflow-y:auto 的纵向 flex 容器，
   默认 flex-shrink:1 会把展开的长条目压到容器高度，配合 overflow:hidden
   内容被裁掉且无法滚动（踩过） */
.ent {
  flex: none;
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  background: var(--c-bg);
  overflow: hidden;
}
.ent.off {
  opacity: 0.62;
}
.en-head {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 9px 10px;
  cursor: pointer;
  user-select: none;
}
.en-head:hover {
  background: var(--c-panel);
}
.chev {
  flex: none;
  font-size: 10px;
  color: var(--c-text-3);
  transition: transform 0.15s;
}
.chev.open {
  transform: rotate(90deg);
}
.en-idx {
  flex: none;
  min-width: 18px;
  font-size: 10px;
  color: var(--c-text-3);
  text-align: right;
}
.en-title {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--c-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bd {
  flex: none;
  padding: 0 6px;
  font-size: 9px;
  line-height: 16px;
  border-radius: 8px;
  background: var(--c-panel);
  color: var(--c-text-2);
}
.bd.const {
  color: var(--p-600);
  border: 1px solid var(--p-400);
}
.bd.off {
  color: var(--s-error);
}

/* 编辑表单 */
.en-form {
  padding: 10px 12px 12px;
  border-top: 1px solid var(--c-border);
  display: flex;
  flex-direction: column;
  gap: 9px;
}
.row3 {
  display: flex;
  gap: 8px;
  align-items: flex-end;
}
.row3 > .fld {
  flex: 1;
  min-width: 0;
}
.fld {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.fld > .lb {
  font-size: 10px;
  color: var(--c-text-3);
}
.fld > .pair {
  display: flex;
  align-items: center;
  gap: 6px;
}
.fld > .pair .field {
  flex: 1;
  min-width: 0;
}
.field {
  padding: 6px 8px;
  font-size: 12px;
  color: var(--c-text);
  background: var(--c-canvas);
  border: 1px solid var(--c-border);
  border-radius: var(--radius-md);
  outline: none;
  min-width: 0;
}
.field:focus {
  border-color: var(--p-400);
}
.field:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.field-area {
  resize: vertical;
  font-family: inherit;
  line-height: 1.6;
}
.adv {
  border: 1px dashed var(--c-border);
  border-radius: var(--radius-md);
  padding: 6px 10px;
}
.adv summary {
  cursor: pointer;
  font-size: 11px;
  color: var(--c-text-3);
  user-select: none;
}
.adv > .fld,
.adv .row3 {
  margin-top: 8px;
}
.adv-t {
  margin-top: 10px;
  font-size: 10px;
  color: var(--c-text-3);
}
.en-ops {
  display: flex;
  align-items: center;
  gap: 6px;
  padding-top: 4px;
}
.en-ops .spacer {
  flex: 1;
}
</style>
