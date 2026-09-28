/**
 * 插件宿主命令封装 + 与 Rust 侧对齐的类型定义
 *
 * 类型定义放这里（而不是 store），因为这里是「与 Rust 的边界」——
 * 谁改 Rust 结构体，谁就来改这个文件，一眼能找到。
 *
 * 桌面环境下通过 Tauri invoke 调用；浏览器开发环境没有宿主，
 * 返回 null 并由 UI 如实说明「桌面壳未运行」，不伪造数据。
 */

export interface DepStatus {
  id: string
  ok: boolean
  message: string
}

export interface PluginReport {
  id: string
  name: string
  /** required（必要插件）| optional（可选插件） */
  tier: string
  /** 执行模型：process | resource | frontend | st-extension */
  kind: string
  description: string
  enabled: boolean
  /** auto（自动探测）| manual（指定路径）| managed（宿主安装） */
  source: string
  path: string | null
  version: string | null
  min_version: string | null
  /** ready | missing | mismatch | disabled */
  status: string
  message: string
  /** 依赖本插件的能力名（停用前的影响预告） */
  required_by: string[]
  dependencies: DepStatus[]
  /** 清单里带下载源 */
  downloadable: boolean
  /** 宿主安装（managed）是否已支持 */
  managed_supported: boolean
  provides: string[]
}

export interface CapabilityReport {
  id: string
  name: string
  ok: boolean
  missing: string[]
  message: string
}

export interface StackReport {
  plugins: PluginReport[]
  capabilities: CapabilityReport[]
  node_path: string | null
  st_dir: string | null
  data_root: string | null
  st_port: number
  git_path: string | null
  /** 数据区现状（S2） */
  data: DataStatus | null
}

/* ---------------- 数据区（S2） ---------------- */

export interface KeyItem {
  id: string
  name: string
  /** 相对数据根的路径 */
  rel: string
  exists: boolean
  kind: string
  files: number
  bytes: number
}

export interface DataStatus {
  active_root: string
  active_config: string
  /** 是否已外移到应用数据目录 */
  external: boolean
  default_root: string
  default_config: string
  external_root: string
  external_config: string
  external_data_exists: boolean
  total_files: number
  total_bytes: number
  items: KeyItem[]
  st_running: boolean
  /** ST 目录内仍可能写入的位置（如实告知，不假装完全只读） */
  residual_writable: string[]
}

export interface MigrationPlan {
  source: string
  target: string
  config_source: string
  config_target: string
  source_exists: boolean
  target_exists: boolean
  files: number
  bytes: number
  items: KeyItem[]
  /** 阻断项（非空则不可执行） */
  blockers: string[]
  warnings: string[]
  can_proceed: boolean
}

export interface VerifyReport {
  checked: number
  bad: string[]
  source_bytes: number
  target_bytes: number
}

export interface MigrationResult {
  ok: boolean
  message: string
  files: number
  bytes: number
  target: string
  config_target: string
  verify: VerifyReport
  /** 源目录是否原样保留（恒为 true） */
  source_kept: boolean
}

export interface MigrationProgress {
  phase: string
  done_bytes: number
  total_bytes: number
  message: string
}

export function hasTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<T>(cmd, args)
}

/** 插件 + 能力快照；非桌面环境返回 null */
export async function pluginsStack(): Promise<StackReport | null> {
  if (!hasTauri()) return null
  return call<StackReport>('plugins_stack')
}

/** 强制重新探测 */
export async function pluginsRescan(): Promise<StackReport | null> {
  if (!hasTauri()) return null
  return call<StackReport>('plugins_rescan')
}

/** 切换绑定：auto / manual / managed */
export async function pluginSetBinding(
  id: string,
  source: string,
  path?: string | null,
): Promise<StackReport | null> {
  if (!hasTauri()) return null
  return call<StackReport>('plugin_set_binding', { id, source, path: path ?? null })
}

/** 启用 / 停用插件 */
export async function pluginSetEnabled(
  id: string,
  enabled: boolean,
): Promise<StackReport | null> {
  if (!hasTauri()) return null
  return call<StackReport>('plugin_set_enabled', { id, enabled })
}

/** 覆盖 ST 端口；传 null 回到清单默认值 */
export async function pluginSetPort(port: number | null): Promise<StackReport | null> {
  if (!hasTauri()) return null
  return call<StackReport>('plugin_set_port', { port })
}

/* ---------------- 数据区命令（S2） ---------------- */

/** 数据区现状（位置 / 体积 / 关键数据清单） */
export async function stDataStatus(): Promise<DataStatus | null> {
  if (!hasTauri()) return null
  return call<DataStatus>('st_data_status')
}

/** 迁移计划（纯只读预览） */
export async function stDataPlan(): Promise<MigrationPlan | null> {
  if (!hasTauri()) return null
  return call<MigrationPlan>('st_data_plan')
}

/**
 * 执行迁移（复制 → 校验 → 切换绑定；源目录保留）。
 * 失败时 reject，`err` 是 Rust 侧给出的可读原因。
 */
export async function stDataMigrate(): Promise<MigrationResult | null> {
  if (!hasTauri()) return null
  return call<MigrationResult>('st_data_migrate')
}

/** 回滚到 ST 目录内的默认位置（仅改绑定，不动数据） */
export async function stDataRollback(): Promise<DataStatus | null> {
  if (!hasTauri()) return null
  return call<DataStatus>('st_data_rollback')
}

/** 删除外移数据副本（仅当当前数据不在外移目录时允许；返回删除后的状态） */
export async function stDataRemoveExternal(): Promise<DataStatus | null> {
  if (!hasTauri()) return null
  return call<DataStatus>('st_data_remove_external')
}

/**
 * 为数据迁移暂停 SillyTavern（仅当由本应用托管）。
 * 外部实例会 reject —— 不越权杀用户自己起的进程。
 */
export async function stPauseForMigration(): Promise<string | null> {
  if (!hasTauri()) return null
  return call<string>('st_pause_for_migration')
}

/** 订阅迁移进度；非桌面环境返回 null */
export async function onDataMigrateProgress(
  cb: (p: MigrationProgress) => void,
): Promise<(() => void) | null> {
  if (!hasTauri()) return null
  const { listen } = await import('@tauri-apps/api/event')
  const un = await listen<MigrationProgress>('st-data-migrate', (e) => cb(e.payload))
  return un
}

/* ---------------- 数据袋（导入/导出） ---------------- */

export interface DataBagResult {
  path: string
  files: number
  bytes: number
  message: string
}

/** 导出数据（data 目录 + config.yaml）到指定 zip 路径；ST 运行中会被拒绝 */
export async function stDataExport(dest: string): Promise<DataBagResult | null> {
  if (!hasTauri()) return null
  return call<DataBagResult>('st_data_export', { dest })
}

/** 从数据袋 zip 导入（覆盖现有数据，导入前自动备份 data.bak-<时间戳>） */
export async function stDataImport(src: string): Promise<DataBagResult | null> {
  if (!hasTauri()) return null
  return call<DataBagResult>('st_data_import', { src })
}

/* ---------------- 会话 jsonl 行级增量操作 ---------------- */

/** 会话单行操作（index 为 jsonl 行号，0 = 元数据头行） */
export type ChatLineOp =
  | { op: 'replace'; index: number; json: string }
  | { op: 'delete'; index: number }
  | { op: 'insert'; index: number; json: string }

/**
 * 行级增量改写会话文件：除目标行外原样保留（swipes/extra 等天然保真）。
 * 仅桌面环境可用；web 环境抛错（调用方应回落全量保存）。
 */
export async function chatMutateLines(
  avatar: string,
  file: string,
  ops: ChatLineOp[],
): Promise<void> {
  await call<void>('chat_mutate_lines', { avatar, file, ops })
}

/** 导出会话原版 jsonl 到指定路径（与 ST 文件互通）；非桌面环境抛错 */
export async function chatExportFile(
  avatar: string,
  file: string,
  dest: string,
): Promise<void> {
  await call<void>('chat_export_file', { avatar, file, dest })
}

/** 导出角色卡原文件（PNG/JSON/CHARX 字节级复制）到指定路径；非桌面环境抛错 */
export async function characterExportCard(avatar: string, dest: string): Promise<void> {
  await call<void>('character_export_png', { avatar, dest })
}

/* ---------------- 应用设置（代理 / 安装目录） ---------------- */

export interface AppSettings {
  /** HTTP 代理，空 = 直连 */
  proxy: string
  /** npm install 是否也走代理 */
  proxy_for_npm: boolean
  /** 组件安装根目录，空 = %LOCALAPPDATA%\<app-id> */
  install_root: string
  _readme?: string
}

export async function getAppSettings(): Promise<AppSettings | null> {
  if (!hasTauri()) return null
  return call<AppSettings>('get_app_settings')
}

export async function setAppSettings(s: AppSettings): Promise<AppSettings | null> {
  if (!hasTauri()) return null
  return call<AppSettings>('set_app_settings', { settings: s })
}

/* ---------------- 组件安装（S4c：宿主安装 managed） ---------------- */

export interface ManagedVersion {
  version: string
  /** required 文件齐全，可作为绑定候选 */
  complete: boolean
  files: number
  bytes: number
  /** 是否是当前绑定 */
  bound: boolean
}

export interface ComponentVersions {
  component: string
  /** 清单推荐版本（安装输入框默认值） */
  default_version: string | null
  install_root: string
  /** 清单启用了预打包源（优先下载+解压，无 npm） */
  prebuilt_enabled: boolean
  default_installed: boolean
  versions: ManagedVersion[]
}

export interface InstallProgress {
  component: string
  version: string
  /** download | extract | npm | verify | done */
  phase: string
  percent: number
  message: string
  download: { done: number; total: number; message: string } | null
}

/** 已安装版本列表（宿主安装区数据源） */
export async function componentVersions(id: string): Promise<ComponentVersions | null> {
  if (!hasTauri()) return null
  return call<ComponentVersions>('component_versions', { id })
}

/**
 * 安装组件。进度通过 `component-install` 事件推送（onComponentInstall 订阅）。
 * 装完**不自动切绑定** —— 由用户在插件页点「使用此版本」。
 */
export async function componentInstall(id: string, version: string): Promise<unknown | null> {
  if (!hasTauri()) return null
  return call<unknown>('component_install', { id, version })
}

/** 查询全局安装状态（视图重挂载时拉一次，恢复进度条显示） */
export interface InstallState {
  in_flight: boolean
  progress: InstallProgress | null
}

export async function componentInstallState(): Promise<InstallState | null> {
  if (!hasTauri()) return null
  return call<InstallState>('component_install_state')
}

/** 删除一个已安装版本（绑定中的版本会被 Rust 侧拒绝） */
export async function componentRemove(
  id: string,
  version: string,
): Promise<ComponentVersions | null> {
  if (!hasTauri()) return null
  return call<ComponentVersions>('component_remove', { id, version })
}

/** 绑定到某个已安装版本（managed）。路径由 Rust 侧裁决，前端只传版本号。 */
export async function componentUse(id: string, version: string): Promise<StackReport | null> {
  if (!hasTauri()) return null
  return call<StackReport>('component_use', { id, version })
}

/** 订阅安装进度；非桌面环境返回 null */
export async function onComponentInstall(
  cb: (p: InstallProgress) => void,
): Promise<(() => void) | null> {
  if (!hasTauri()) return null
  const { listen } = await import('@tauri-apps/api/event')
  const un = await listen<InstallProgress>('component-install', (e) => cb(e.payload))
  return un
}
