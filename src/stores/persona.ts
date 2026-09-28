/**
 * 用户人设 store
 *
 * 单一事实来源是 ST 的 settings.json（power_user 段），本 store 只做「读 → 缓存 → 写回」，
 * 不在前端另存一份。
 */
import { defineStore, acceptHMRUpdate } from 'pinia'
import { useChatStore } from '@/stores/chat'
import {
  listPersonas,
  savePersonas,
  uploadAvatar,
  deleteAvatar,
  type Persona,
  type PersonaSnapshot,
} from '@/services/st/persona'

export const usePersonaStore = defineStore('persona', {
  state: () => ({
    personas: [] as Persona[],
    /** 当前编辑的人设 id */
    selectedId: '' as string,
    defaultId: null as string | null,
    userName: '',
    depth: 2,
    role: 0,
    loading: false,
    saving: false,
    error: '',
    /** 最近一次操作的结果提示 */
    message: '',
    /** 中栏「新增用户」触发 → 右栏展开新建表单（跨组件通信走 store） */
    createMode: false,
  }),

  getters: {
    selected(state): Persona | null {
      return state.personas.find((p) => p.id === state.selectedId) ?? null
    },
    hasPersisted(state): boolean {
      return state.personas.length > 0 || state.userName !== ''
    },
  },

  actions: {
    async load(): Promise<void> {
      this.loading = true
      this.error = ''
      try {
        const snap: PersonaSnapshot = await listPersonas()
        this.personas = snap.personas
        this.defaultId = snap.defaultId
        this.userName = snap.userName
        this.depth = snap.depth
        this.role = snap.role
        if (!this.selectedId && snap.personas.length) {
          this.selectedId =
            (snap.defaultId && snap.personas.find((p) => p.id === snap.defaultId)?.id) ||
            snap.personas[0].id
        }
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.loading = false
      }
    },

    select(id: string): void {
      this.selectedId = id
      this.message = ''
    },

    /** 请求进入新建模式（由中栏按钮触发，右栏消费） */
    beginCreate(): void {
      this.createMode = true
      this.message = ''
    },

    endCreate(): void {
      this.createMode = false
    },

    /** 改字段（不落盘，由 saveSelected / saveAll 触发写入） */
    patchSelected(patch: Partial<Persona>): void {
      const p = this.personas.find((x) => x.id === this.selectedId)
      if (p) Object.assign(p, patch)
    },

    /**
     * 使用某人设：用户名连带切成人设名，左栏头像随之更新。
     * ST 侧没有「当前人设」这个独立字段 —— 它就是靠 username 反查 personas 得到的，
     * 所以这里同步改 username 才是真正的等价实现。
     */
    async usePersona(id: string): Promise<void> {
      const target = this.personas.find((x) => x.id === id)
      if (!target) return
      this.selectedId = id
      this.defaultId = id
      this.userName = target.name
      await this.saveAll()
      this.message = `已切换到「${target.name}」`
    },

    /** 保存全部人设数据（映射 + 默认 + 用户名 + 深度/角色） */
    async saveAll(): Promise<void> {
      this.saving = true
      this.error = ''
      try {
        await savePersonas({
          personas: this.personas,
          defaultId: this.defaultId,
          userName: this.userName,
          depth: this.depth,
          role: this.role,
        })
        // 用户名改了要立刻影响聊天（{{user}} 注入用），不必等刷新
        const chat = useChatStore()
        if (this.userName.trim()) chat.userName = this.userName.trim()
        this.message = '已保存'
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.saving = false
      }
    },

    /** 新建人设：先落头像文件，再建映射 */
    async create(file: File, name: string): Promise<void> {
      this.saving = true
      this.error = ''
      try {
        const id = await uploadAvatar(file)
        const persona: Persona = {
          id,
          name: name.trim() || id.replace(/\.[^.]+$/, ''),
          description: '',
          position: 0,
        }
        this.personas.push(persona)
        this.selectedId = id
        if (!this.defaultId) this.defaultId = id
        await this.saveAll()
        this.message = `已创建人设「${persona.name}」`
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.saving = false
      }
    },

    /** 删除人设：删映射 + 删头像文件（文件删不掉也不阻断） */
    async remove(id: string): Promise<void> {
      this.saving = true
      this.error = ''
      try {
        this.personas = this.personas.filter((p) => p.id !== id)
        if (this.defaultId === id) this.defaultId = this.personas[0]?.id ?? null
        if (this.selectedId === id) this.selectedId = this.personas[0]?.id ?? ''
        await this.saveAll()
        try {
          await deleteAvatar(id)
        } catch {
          /* 头像文件删不掉不影响映射已删，忽略 */
        }
        this.message = '已删除'
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.saving = false
      }
    },

    /** 更换当前人设头像（保留映射与描述） */
    async changeAvatar(file: File): Promise<void> {
      const cur = this.selected
      if (!cur) return
      this.saving = true
      this.error = ''
      try {
        await deleteAvatar(cur.id).catch(() => undefined)
        const newId = await uploadAvatar(file)
        const idx = this.personas.findIndex((p) => p.id === cur.id)
        if (idx >= 0) this.personas[idx] = { ...cur, id: newId }
        if (this.selectedId === cur.id) this.selectedId = newId
        if (this.defaultId === cur.id) this.defaultId = newId
        await this.saveAll()
        this.message = '头像已更换'
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.saving = false
      }
    },
  },
})

/* 开发期热更新接管：改本 store 后无需手动刷新页面（Pinia 官方推荐写法） */
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(usePersonaStore, import.meta.hot))
}
