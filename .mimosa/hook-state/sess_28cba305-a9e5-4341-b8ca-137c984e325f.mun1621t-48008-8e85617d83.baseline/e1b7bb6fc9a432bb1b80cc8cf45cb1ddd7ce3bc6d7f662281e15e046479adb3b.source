/**
 * 插件页状态（「万物皆可插件」的统一视图）
 *
 * 数据全部来自 Rust 插件宿主的 `plugins_stack`，前端不做任何推断 ——
 * 探测、依赖求解、能力判定都在宿主侧完成，保证「UI 说的」与「实际用的」一致。
 */
import { defineStore, acceptHMRUpdate } from 'pinia'
import {
  componentInstall,
  componentInstallState,
  hasTauri,
  onDataMigrateProgress,
  onComponentInstall,
  pluginSetBinding,
  pluginSetEnabled,
  pluginSetPort,
  pluginsRescan,
  pluginsStack,
  stDataMigrate,
  stDataPlan,
  stDataRollback,
  type CapabilityReport,
  type DataStatus,
  type InstallProgress,
  type MigrationPlan,
  type MigrationProgress,
  type MigrationResult,
  type PluginReport,
  type StackReport,
} from '@/services/tauri/plugins'

/**
 * ST 绑定修好/组件装好后自动拉起 sidecar。
 *
 * 启动屏在「未找到」时已自动放行进插件页，那时没人再点「重试」——
 * 所以绑定切换（手动指定/使用版本）成功后由这里兜底拉起；
 * ST 已在跑或正在启动时不做任何事（后端 st_retry_start 本身幂等）。
 */
async function retryStIfDown(): Promise<void> {
  if (!hasTauri()) return
  try {
    const { sidecar } = await import('@/services/tauri/bridge')
    const st = sidecar.status
    if (st && (st.state === 'ready' || st.state === 'starting')) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('st_retry_start')
  } catch {
    /* 拉不起来（绑定仍无效）→ 状态条/插件页如实呈现，不打断用户 */
  }
}

export const usePluginsStore = defineStore('plugins', {
  state: () => ({
    stack: null as StackReport | null,
    loading: false,
    error: '',
    /** 浏览器开发环境没有宿主 */
    unavailable: false,
    /** 当前选中的插件（中栏点选 → 右栏详情） */
    selectedId: '' as string,
    /** 操作进行中（避免重复点击） */
    mutating: false,
    /** 迁移计划（打开向导时拉取，纯只读） */
    dataPlan: null as MigrationPlan | null,
    /** 迁移进行中 */
    migrating: false,
    progress: null as MigrationProgress | null,
    /**
     * 组件安装的**全局**状态（S4c）：
     * 放 store 而不是视图组件里 —— 切换菜单后视图会卸载，
     * 进度和「安装中」标记必须活过视图切换，否则会出现「后台在装、前台能再点」的裂缝。
     */
    installInFlight: false,
    installProgress: null as InstallProgress | null,
    /** 安装监听是否已初始化（只订阅一次） */
    installWatcherReady: false,
  }),

  getters: {
    required(state): PluginReport[] {
      return state.stack?.plugins.filter((p) => p.tier === 'required') ?? []
    },
    optional(state): PluginReport[] {
      return state.stack?.plugins.filter((p) => p.tier !== 'required') ?? []
    },
    selected(state): PluginReport | null {
      return state.stack?.plugins.find((p) => p.id === state.selectedId) ?? null
    },
    capabilities(state): CapabilityReport[] {
      return state.stack?.capabilities ?? []
    },
    /** 聊天能力可用（聊天页状态条也用这个） */
    chatReady(state): boolean {
      return state.stack?.capabilities.find((c) => c.id === 'chat')?.ok ?? false
    },
    /** 未就绪的必要插件数（插件页红点用） */
    problems(state): number {
      return (
        state.stack?.plugins.filter((p) => p.tier === 'required' && p.status !== 'ready').length ??
        0
      )
    },
    dataStatus(state): DataStatus | null {
      return state.stack?.data ?? null
    },
    /** ST 数据是否已外移 */
    dataExternal(state): boolean {
      return state.stack?.data?.external ?? false
    },
  },

  actions: {
    /**
     * 初始化安装状态监听（幂等，整个应用生命周期只订阅一次）。
     * 先拉一次全局状态（恢复可能正在进行的安装的进度），再挂事件订阅。
     */
    async initInstallWatcher() {
      if (!hasTauri() || this.installWatcherReady) return
      this.installWatcherReady = true
      try {
        const s = await componentInstallState()
        if (s) {
          this.installInFlight = s.in_flight
          this.installProgress = s.progress
        }
      } catch {
        /* 状态拿不到不影响其它功能 */
      }
      await onComponentInstall((p) => {
        this.installProgress = p
      })
    },

    /**
     * 安装组件（全局入口）。进度经事件持续更新到 store；
     * 结束后刷新插件快照（但不切绑定 —— 由用户在版本列表点「使用此版本」）。
     */
    async installComponent(id: string, version: string): Promise<boolean> {
      if (this.installInFlight) return false
      this.installInFlight = true
      this.installProgress = null
      this.error = ''
      const startedAt = Date.now()
      let ok = true
      try {
        await componentInstall(id, version)
      } catch (e) {
        ok = false
        this.error = e instanceof Error ? e.message : String(e)
        throw e
      } finally {
        this.installInFlight = false
        // 进度条不能立刻清空：早退/短任务会在用户看清之前就结束，看起来像"没有进度条"。
        // 不足最短展示时长时延迟清理，留住终态；到点后只清本次自己那一份状态。
        const KEEP_MS = 1600
        const snapshot = this.installProgress
        const rest = KEEP_MS - (Date.now() - startedAt)
        if (snapshot && rest > 0) {
          setTimeout(() => {
            if (this.installProgress === snapshot) this.installProgress = null
          }, rest)
        } else {
          this.installProgress = null
        }
        await this.load().catch(() => {})
      }
      return ok
    },

    async load() {
      if (!hasTauri()) {
        this.unavailable = true
        return
      }
      this.loading = true
      this.error = ''
      try {
        const s = await pluginsStack()
        if (s) {
          this.stack = s
          if (!this.selectedId && s.plugins.length) this.selectedId = s.plugins[0].id
        }
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.loading = false
      }
    },

    async rescan() {
      if (!hasTauri()) return
      this.loading = true
      this.error = ''
      try {
        const s = await pluginsRescan()
        if (s) this.stack = s
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      } finally {
        this.loading = false
      }
    },

    select(id: string) {
      this.selectedId = id
    },

    /** 切换绑定（auto / manual / managed）；失败时把宿主的报错原样抛出给 UI */
    async setBinding(id: string, source: string, path?: string | null) {
      this.mutating = true
      this.error = ''
      try {
        const s = await pluginSetBinding(id, source, path)
        if (s) this.stack = s
        // ST 绑定修好（手动指定/切回 auto 命中）→ 自动拉起，不需要用户再去别处点「重试」
        if (id === 'sillytavern') void retryStIfDown()
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        throw e
      } finally {
        this.mutating = false
      }
    },

    async setEnabled(id: string, enabled: boolean) {
      this.mutating = true
      this.error = ''
      try {
        const s = await pluginSetEnabled(id, enabled)
        if (s) this.stack = s
        if (id === 'sillytavern' && enabled) void retryStIfDown()
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        throw e
      } finally {
        this.mutating = false
      }
    },

    async setPort(port: number | null) {
      this.mutating = true
      this.error = ''
      try {
        const s = await pluginSetPort(port)
        if (s) this.stack = s
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        throw e
      } finally {
        this.mutating = false
      }
    },

    /* ---------------- 数据区（S2） ---------------- */

    /** 拉取迁移计划（只读预览，不动任何文件） */
    async loadDataPlan(): Promise<MigrationPlan | null> {
      this.error = ''
      try {
        this.dataPlan = await stDataPlan()
        return this.dataPlan
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        return null
      }
    },

    /**
     * 执行迁移。宿主会先复制再逐文件校验，通过后才切换绑定；源目录永远保留。
     * 失败时把宿主给的原因原样抛出（含阻断项说明）。
     */
    async migrateData(): Promise<MigrationResult | null> {
      this.migrating = true
      this.progress = null
      this.error = ''
      const off = await onDataMigrateProgress((p) => {
        this.progress = p
      })
      try {
        const r = await stDataMigrate()
        await this.load() // 绑定已切换，刷新以反映新数据位置
        this.dataPlan = null
        return r
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        throw e
      } finally {
        this.migrating = false
        this.progress = null
        off?.()
      }
    },

    /** 回滚到 ST 目录内的默认位置（仅改绑定，不动数据） */
    async rollbackData(): Promise<void> {
      this.mutating = true
      this.error = ''
      try {
        await stDataRollback()
        await this.load()
        this.dataPlan = null
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
        throw e
      } finally {
        this.mutating = false
      }
    },
  },
})

/* 开发期热更新接管：改本 store 后无需手动刷新页面（Pinia 官方推荐写法） */
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(usePluginsStore, import.meta.hot))
}
