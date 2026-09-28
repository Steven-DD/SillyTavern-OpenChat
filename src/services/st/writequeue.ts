/**
 * 会话文件写队列：所有「整文件读-改-写 / 全量保存」必须串行化。
 *
 * 背景（架构审计 P1）：单聊会话文件有多条独立写入通路 —— saveCurrent 全量保存、
 * 宏变量/摘要/作者注释/参数覆盖的读-改-写、timedEffects 写回、群聊 saveGroupLines ——
 * 彼此互不感知。防抖定时器等异步回调可以在生成链的任意 await 边界插入：
 * A 任务 getChat 拿到旧内容后，B 任务整文件写入新消息，随后 A 把**旧内容**写回，
 * 刚落盘的发言就从文件里消失了（「后写者胜」回滚）。
 * 统一经本队列入队后，任意时刻最多一个会话文件写任务在飞行。
 */
let chain: Promise<unknown> = Promise.resolve()

/**
 * 把一个会话文件写任务追加到队列尾部串行执行。
 * 返回任务自身的 Promise（成功/失败都透传给调用方）；任务失败不会阻断后续任务。
 */
export function enqueueChatWrite<T>(task: () => Promise<T>): Promise<T> {
  const run = chain.then(task, task)
  chain = run.then(
    () => undefined,
    () => undefined,
  )
  return run
}
