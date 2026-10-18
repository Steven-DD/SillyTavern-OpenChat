/**
 * Tauri 桌面壳命令封装（与 Rust 侧的唯一边界）
 *
 * 类型定义放这里（而不是 store），因为「谁改 Rust 结构体，谁就来改这个文件」。
 * 桌面环境下通过 Tauri invoke 调用；浏览器开发环境没有宿主，如实返回/抛错。
 */

export function hasTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<T>(cmd, args)
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

/** 导出会话原版 jsonl 到指定路径；非桌面环境抛错 */
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

/* ---------------- 数据袋（导入/导出） ---------------- */

export interface DataBagResult {
  path: string
  files: number
  bytes: number
  message: string
}

/**
 * 导出数据到指定 zip 路径。
 * includeSecrets=false（默认）排除明文密钥（secrets.json / cookie-secret.txt）；
 * true 则打包并在结果 message 里附带明文警告（调用方需负责提示/确认）。
 */
export async function stDataExport(
  dest: string,
  includeSecrets = false,
): Promise<DataBagResult | null> {
  if (!hasTauri()) return null
  return call<DataBagResult>('st_data_export', { dest, includeSecrets })
}

/** 从数据袋 zip 导入（覆盖现有数据，导入前自动备份 data.bak-<时间戳>） */
export async function stDataImport(src: string): Promise<DataBagResult | null> {
  if (!hasTauri()) return null
  return call<DataBagResult>('st_data_import', { src })
}
