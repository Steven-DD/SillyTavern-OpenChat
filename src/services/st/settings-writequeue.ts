/**
 * settings.json 写队列（评审整改 T5）：所有 settings 读-改-写必须串行化。
 *
 * 背景（评审 A3，已复现）：/api/settings/save 是整体覆盖语义（传什么写什么，
 * 没传的键被直接抹掉），而 settings 的写通路散落 10+ 处 —— mapping 同步、
 * QR/正则/翻译/记忆/向量/TTS/表情/PM/人设各自「读全量 → 合并 → 写全量」，
 * 彼此互不感知。每个调用方 getSettings()（5s TTL 缓存）拿到旧快照后，
 * 另一个模块完成写回，随后前者把**基于旧快照的合并结果**写回 ——
 * 后写者的键丢了（lost update）。会话 JSONL 有专用写队列（writequeue.ts），
 * settings 此前没有任何等价机制。
 *
 * ── 与 writequeue 的关键差异 ──
 * 聊天队列锁「写」动作即可（读发生在任务外）；settings 的丢失窗口在
 * 「任务内的 getSettings 之后、写回之前」，所以本队列的任务函数拿到的是
 * **锁内的新鲜全量**，调用方在锁内完成合并 —— 整个读-改-写是一个原子事务。
 *
 * ── 写合并（single-flight）──
 * 多个 patch 在队列中排队时，任务执行时刻逐个基于「当时最新落盘内容」合并，
 * 先到先写、后到者基于前者的结果再合并 —— 键不丢。
 */

import { getSettings, saveSettingsFull, invalidateSettingsCache } from './data'
import type { StSettingsLite } from './types'

let chain: Promise<unknown> = Promise.resolve()

/**
 * 把一个 settings 写任务追加到队列尾部串行执行。
 * 返回任务自身的 Promise（成功/失败都透传）；任务失败不阻断后续任务。
 */
function enqueue<T>(task: () => Promise<T>): Promise<T> {
  const run = chain.then(task, task)
  chain = run.then(
    () => undefined,
    () => undefined,
  )
  return run
}

export type SettingsMutationResult<T> = {
  /** 合并后的完整 settings（整体覆盖语义的 body 本体，禁包装对象） */
  next: StSettingsLite
  /** 返回值透传给调用方（如合并后的局部配置） */
  result?: T
  /** 返回 false = 无变化不写（调用方不必自行比对；但自行比对可少一次读） */
  skip?: boolean
}

export type SettingsMutation<T> = (
  settings: StSettingsLite,
) => SettingsMutationResult<T> | Promise<SettingsMutationResult<T>>

/**
 * 原子地「读最新全量 → 调用方合并 → 写全量」。
 *
 * - settings 是任务执行时刻从服务端新读的（锁内强制绕过 5s TTL 缓存），
 *   排在前面的任务刚写的内容对后面任务可见 —— 这是防丢键的关键。
 * - 返回 mutation 的 result（无 result 则返回 void）。
 */
export function mutateSettings<T = void>(mutate: SettingsMutation<T>): Promise<T> {
  return enqueue(async () => {
    // 锁内强制新鲜读：绕过 TTL 缓存，避免基于 5s 前的旧快照合并
    invalidateSettingsCache()
    const current = await getSettings()
    const { next, result, skip } = await mutate(current)
    if (!skip) {
      await saveSettingsFull(next)
    }
    return result as T
  })
}

/**
 * 纯写任务（已持完整 body 的少数场景；仍走队列串行）。
 * 仅限「写的是刚读出的完整对象」的调用方使用 —— 禁止把部分字段当 body。
 */
export function enqueueSettingsWrite<T>(task: () => Promise<T>): Promise<T> {
  return enqueue(task)
}

/**
 * 等待队列排空（测试与关停路径用）。
 */
export async function settingsQueueIdle(): Promise<void> {
  await chain.catch(() => undefined)
}
