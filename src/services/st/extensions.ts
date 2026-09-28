/**
 * 扩展（插件）集成辅助：发现 + 快捷回复存储
 * - discover：GET /api/extensions/discover（内置 + 第三方扩展清单）
 * - 快捷回复：settings.json extension_settings.stchat_quick_replies（App 自有键；
 *   ST 原生 QR 套装是独立文件 + Slash 命令体系，v2 再做文件级互通）
 */
import { stGet, stPost, stPostVoid } from './client'
import { getSettings, saveSettingsFull } from './data'

export interface DiscoveredExtension {
  type: string
  name: string
}

export async function discoverExtensions(): Promise<DiscoveredExtension[]> {
  const data = await stGet<{ type?: string; name?: string }[]>('/api/extensions/discover')
  return Array.isArray(data) ? (data as DiscoveredExtension[]) : []
}

export interface QuickReply {
  label: string
  message: string
  enabled: boolean
  /** 点击行为：send = 直接发送（默认）；insert = 追加进输入框草稿 */
  mode?: 'send' | 'insert'
}

let qrCache: QuickReply[] | null = null

export function invalidateQuickReplies(): void {
  qrCache = null
}

export async function loadQuickReplies(force = false): Promise<QuickReply[]> {
  if (qrCache && !force) return qrCache
  const settings = await getSettings()
  const ext = settings.extension_settings as Record<string, unknown> | undefined
  const raw = ext?.stchat_quick_replies
  qrCache = Array.isArray(raw)
    ? (raw as QuickReply[]).filter((q) => q && typeof q.label === 'string')
    : []
  return qrCache
}

export async function saveQuickReplies(list: QuickReply[]): Promise<void> {
  const settings = await getSettings()
  const ext = (settings.extension_settings as Record<string, unknown> | undefined) ?? {}
  if (JSON.stringify(ext.stchat_quick_replies) === JSON.stringify(list)) {
    qrCache = list
    return
  }
  settings.extension_settings = { ...ext, stchat_quick_replies: list }
  await saveSettingsFull(settings)
  qrCache = list
}

/* ---------------- ST 原生快捷回复集（QuickReplies/*.json，B5 文件互通） ---------------- */

/** ST 原生 QR 集文件（原样透传；qrList 是 ST 端结构） */
export interface StQrPreset {
  name: string
  qrList?: { label?: string; message?: string; [k: string]: unknown }[]
  [k: string]: unknown
}

/** 列出 ST 的 QR 集文件（settings 响应内嵌 quickReplyPresets，settings.js:261） */
export async function listQrPresets(): Promise<StQrPreset[]> {
  const s = await getSettings()
  const raw = (s as Record<string, unknown>).quickReplyPresets
  return Array.isArray(raw) ? (raw as StQrPreset[]) : []
}

/** 把 ST 集（qrList）导入到 App QR 列表（追加；label 去重跳过），返回导入条数 */
export async function importQrPreset(name: string): Promise<number> {
  const presets = await listQrPresets()
  const p = presets.find((x) => x.name === name)
  if (!p) throw new Error(`找不到快捷回复集「${name}」`)
  const cur = await loadQuickReplies(true)
  const labels = new Set(cur.map((q) => q.label))
  const add = (p.qrList ?? [])
    .filter((q) => q && typeof q.label === 'string' && !labels.has(q.label as string))
    .map((q) => ({ label: q.label as string, message: String(q.message ?? ''), enabled: true }))
  await saveQuickReplies([...cur, ...add])
  return add.length
}

/**
 * 把 App QR 列表导出为 ST 集（POST /api/quick-replies/save，整文件落盘 QuickReplies/<name>.json）。
 * 结构对齐 ST QuickReplySet.toJSON（QuickReplySet.js:362-374）：
 * version:2 / idIndex / 条目 id（缺失会让网页端 idIndex 计算 NaN 化）/ isHidden（网页端显隐字段，
 * App 的 enabled 不被网页端识别）。注意：网页端运行数据源是 settings.quickReplyPresets，
 * 导出文件需在网页端 QR 管理器手动导入才会生效。
 */
export async function exportQrPreset(name: string, list: QuickReply[]): Promise<void> {
  await stPostVoid('/api/quick-replies/save', {
    version: 2,
    id: `stchat-${name}`,
    name,
    disableSend: false,
    placeBeforeInput: false,
    injectInput: false,
    color: 'transparent',
    onlyBorderColor: false,
    idIndex: list.length + 1,
    qrList: list.map((q, i) => ({
      id: i + 1,
      showLabel: true,
      label: q.label,
      title: '',
      message: q.message,
      contextList: [],
      preventAutoExecute: false,
      isHidden: !q.enabled,
      executeOnStartup: false,
      executeOnUser: false,
      executeOnAi: false,
      executeOnChatChange: false,
      executeOnGroupMemberDraft: false,
      executeOnNewChat: false,
    })),
  })
}

/* ------------------------------------------------------------------ *
 * 扩展管理（安装 / 更新 / 版本 / 删除 / 启停 / 分支 / 移动）—— 对齐 ST 扩展管理器
 * 注意：ST 这些端点的错误响应是纯文本（非 JSON），统一用 stPost + 手工解析
 * ------------------------------------------------------------------ */

/** 从 Git URL 安装扩展；返回清单信息（ST install 响应体） */
export interface ExtInstallResult {
  version?: string
  author?: string
  display_name?: string
  folderName?: string
}

async function postExt<T>(path: string, body: unknown): Promise<T> {
  const res = await stPost(path, body)
  const text = await res.text()
  if (!res.ok) throw new Error(text || `请求失败（HTTP ${res.status}）`)
  try {
    return JSON.parse(text) as T
  } catch {
    return text as unknown as T
  }
}

export async function installExtension(
  url: string,
  global = false,
  branch?: string,
): Promise<ExtInstallResult> {
  return postExt<ExtInstallResult>('/api/extensions/install', {
    url,
    global,
    ...(branch ? { branch } : {}),
  })
}

/** 更新响应：commitHash / isUpToDate / remoteUrl */
export interface ExtUpdateResult {
  commitHash?: string
  isUpToDate?: boolean
  remoteUrl?: string
}

/** 检查并更新扩展（git pull） */
export async function updateExtension(name: string, global = false): Promise<ExtUpdateResult> {
  return postExt<ExtUpdateResult>('/api/extensions/update', { extensionName: name, global })
}

/** /version 响应：分支名 + HEAD + 是否落后 + 远端地址 */
export interface ExtVersionInfo {
  currentBranchName?: string
  currentCommitHash?: string
  isUpToDate?: boolean
  remoteUrl?: string
}

/** 查询扩展版本状态（git 信息） */
export async function getExtensionVersion(name: string, global = false): Promise<ExtVersionInfo> {
  return postExt<ExtVersionInfo>('/api/extensions/version', { extensionName: name, global })
}

/** 删除扩展（不可恢复） */
export async function deleteExtension(name: string, global = false): Promise<void> {
  await stPostVoid('/api/extensions/delete', { extensionName: name, global })
}

export interface ExtBranch {
  current: boolean
  commit: string
  name: string
  label: string
}

/** 分支列表（远程仓库全部分支 + 本地） */
export async function getExtensionBranches(name: string, global = false): Promise<ExtBranch[]> {
  return postExt<ExtBranch[]>('/api/extensions/branches', { extensionName: name, global })
}

export async function switchExtensionBranch(
  name: string,
  global: boolean,
  branch: string,
): Promise<void> {
  await postExt('/api/extensions/switch', { extensionName: name, global, branch })
}

/** 在用户目录 ↔ 全局目录间移动（global = 目标侧） */
export async function moveExtension(name: string, global: boolean): Promise<void> {
  await postExt('/api/extensions/move', { extensionName: name, global })
}

/** 拉取扩展 manifest.json（静态路径；name 已含 third-party/ 前缀） */
export interface ExtManifest {
  display_name?: string
  description?: string
  author?: string
  version?: string
  loading_order?: number
  requires?: string[]
  dependencies?: string[]
  [k: string]: unknown
}

export async function getExtensionManifest(name: string): Promise<ExtManifest | null> {
  try {
    return await stGet<ExtManifest>(`/scripts/extensions/${name}/manifest.json`)
  } catch {
    return null
  }
}

/** 停用名单（extension_settings.disabledExtensions，与 ST 前端同键） */
export async function getDisabledExtensions(): Promise<string[]> {
  const settings = await getSettings()
  const ext = settings.extension_settings as Record<string, unknown> | undefined
  const raw = ext?.disabledExtensions
  return Array.isArray(raw) ? raw.filter((x): x is string => typeof x === 'string') : []
}

export async function setExtensionDisabled(name: string, disabled: boolean): Promise<string[]> {
  const settings = await getSettings()
  const ext = (settings.extension_settings as Record<string, unknown> | undefined) ?? {}
  const current = Array.isArray(ext.disabledExtensions)
    ? (ext.disabledExtensions as unknown[]).filter((x): x is string => typeof x === 'string')
    : []
  const next = disabled
    ? current.includes(name)
      ? current
      : [...current, name]
    : current.filter((x) => x !== name)
  if (JSON.stringify(next) === JSON.stringify(current)) return next
  settings.extension_settings = { ...ext, disabledExtensions: next }
  await saveSettingsFull(settings)
  return next
}

/** 内置扩展说明：ST 原生用途 + 本 App 的集成状态 */
export const BUILTIN_EXT_INFO: Record<string, { desc: string; app: string }> = {
  assets: { desc: '资产管理：下载/更新 UI 资源、背景、音效', app: 'ST 网页端使用；App 暂未接入' },
  attachments: { desc: '附件管理：把文件附加到消息', app: 'ST 网页端使用；App 暂未接入' },
  caption: { desc: '图像反推：为图片生成文字说明发给模型', app: 'ST 网页端使用；App 暂未接入' },
  'connection-manager': { desc: '连接管理：一键切换 API 连接预设', app: 'App 的「设置 → 模型」承担同等职责' },
  expressions: { desc: '表情立绘：按消息情感切换角色立绘', app: 'ST 网页端使用；App 暂未接入' },
  gallery: { desc: '图库：角色聊天中生成的图片管理', app: 'ST 网页端使用；App 暂未接入' },
  memory: { desc: '总结记忆：定期摘要并注入提示词', app: '未集成；作者注释已内置（会话配置 → 记忆）' },
  'quick-reply': { desc: '快捷回复：可配置按钮发送预设消息/脚本', app: '已集成：聊天输入区上方按钮条（管理在 设置 → 通用）' },
  regex: { desc: '正则脚本：对输入/输出/提示词做查找替换', app: '已集成：编辑管理在 设置 → 通用（数据与 ST 互通）' },
  'stable-diffusion': { desc: '文生图：本地/云端 SD 生成图片', app: 'ST 网页端使用；App 暂未接入' },
  speech: { desc: '语音合成/识别：本地 TTS 与语音输入', app: '已集成 TTS 朗读：消息工具条 🔊' },
  'token-counter': { desc: 'Token 计数器：独立文本计数面板', app: 'App 状态条已显示每轮 token 用量' },
  translate: { desc: '翻译：消息/输入翻译（多家供应商）', app: '已集成：消息工具条「译」' },
  vectors: { desc: '向量存储：数据源向量化 + 检索注入', app: '未集成（依赖会话级注入编排，后续排期）' },
  shared: { desc: '共享：跨扩展的共享数据', app: 'ST 网页端使用' },
  slideshow: { desc: '幻灯片：背景幻灯片播放', app: 'ST 网页端使用' },
}

/** 扩展在 App 侧的使用说明（内置查表，第三方给通用说明） */
export function extUsageInfo(name: string, type: string): { desc: string; app: string } {
  const bare = name.startsWith('third-party/') ? name.slice('third-party/'.length) : name
  if (type === 'system' || BUILTIN_EXT_INFO[bare]) {
    return BUILTIN_EXT_INFO[bare] ?? { desc: 'ST 内置插件', app: 'ST 网页端使用；App 暂未接入' }
  }
  return {
    desc: '第三方插件（前端脚本）',
    app: '供 ST 网页端加载使用；本 App 内不运行动态脚本，此处仅提供下载/更新/删除/启停管理',
  }
}
