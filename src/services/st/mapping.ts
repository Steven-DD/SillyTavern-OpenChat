/**
 * 配置映射 · IO 层：本 App 的配置 → SillyTavern 的 settings.json
 *
 * ── 为什么需要它 ──
 * 目标形态是「App 是唯一入口，底层用稳定版 ST」。用户只应该面对本 App 的配置项，
 * 不该知道 ST 把某个值存在哪个键里。纯逻辑在 `mapping-core.ts`，这里只负责读写与编排。
 *
 * ── ST 实现要求的约束 ──
 * 1. **写必须是「读全量 → 合并 → 写全量」**。`/api/settings/save` 是整体覆盖
 *    （`JSON.stringify(request.body)`），只传部分字段会把其余键**直接抹掉**。
 * 2. **body 必须是 settings 本身**，不能包成 `{ settings: ... }`（会写坏文件）。
 *
 * ── 映射范围的取舍（诚实说明）──
 * App 负责组装 prompt，并在每次请求中显式传递 model、temperature 和 max_tokens
 * （见 `prompt.ts` / `api.ts`），所以 ST 的全局设置**对生成结果的影响比直觉小**。
 * 映射的真正价值是「不让两边的数字撕裂」，以及为将来可能放开 ST 界面留一致性。
 * 细节见 `mapping-core.ts` 的映射表注释。
 */
import { getSettings, saveSettings, invalidateSettingsCache } from './data'
import { mutateSettings } from './settings-writequeue'
import {
  diffMapping,
  mergeInto,
  type AppConfigForMapping,
  type Dict,
  type MappingDiff,
} from './mapping-core'
import type { StSettingsLite } from './types'

export type {
  AppConfigForMapping,
  MappingDiff,
  MappingEntry,
  MappingRole,
} from './mapping-core'
export { buildEntries, diffMapping, getPath, mergeInto, roleLabel, setPath } from './mapping-core'

export interface MappingReport {
  ok: boolean
  entries: MappingDiff[]
  /** 一致（同步成功）的条数 */
  applied: number
  total: number
  message: string
}

/** 只读地拿一份对比结果（不动 ST） */
export async function inspectMapping(cfg: AppConfigForMapping): Promise<MappingDiff[]> {
  const settings = await getSettings()
  return diffMapping(settings, cfg)
}

/**
 * 把本 App 的配置写进 ST 的 settings.json。
 *
 * 经 settings 写队列（T5）：整个「读全量 → 内存合并 → 写全量 → 读回校验」
 * 在一个队列事务内完成 —— 合并基于锁内新鲜读，不会抹掉排队中其它模块刚写的键。
 */
export async function applyMapping(cfg: AppConfigForMapping): Promise<MappingReport> {
  return mutateSettings(async (before) => {
    const merged: Dict = mergeInto(before, cfg)

    await saveSettings(merged as StSettingsLite)

    // 读回校验：不信任「写成功」，只信任「读到的值对」
    invalidateSettingsCache()
    const after = await getSettings()
    const entries = diffMapping(after, cfg)
    const applied = entries.filter((e) => e.inSync).length
    const total = entries.length

    return {
      next: merged as StSettingsLite,
      skip: true, // 落盘已由上方 saveSettings 完成，这里只透传报告
      result: {
        ok: applied === total,
        entries,
        applied,
        total,
        message:
          applied === total
            ? `已同步 ${applied} 项到 settings`
            : `同步完成，但 ${total - applied} 项读回不一致`,
      } satisfies MappingReport,
    }
  })
}
