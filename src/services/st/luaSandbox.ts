/**
 * Lua 沙箱执行环境（基于 Fengari 纯 JS Lua 5.3 运行时）
 *
 * 安全原则：
 * 1. 禁用一切涉及宿主 OS、IO、外部包加载与调试的危险标准库（os/io/package/debug/dofile/loadfile）
 * 2. 指令计数硬上限防护（防止 while true 等死循环冻结 UI 线程）
 * 3. 严格隔离全局状态，每次执行均在纯净沙箱中运行
 */
import { lua, lauxlib, lualib, to_luastring, to_jsstring } from 'fengari'
import { registerRisuBridge, type RisuBridgeContext, pushToLua, pullFromLua } from './risuBridge'
import { log } from '@/services/log'

const MAX_INSTRUCTIONS = 250_000

export interface LuaExecutionResult {
  success: boolean
  result?: unknown
  error?: string
}

/** 创建纯净且安全的 Lua 运行状态 */
export function createSandboxedLuaState(ctx: RisuBridgeContext): any {
  const L = lauxlib.luaL_newstate()
  // 加载标准库
  lualib.luaL_openlibs(L)

  // 1. 剔除危险模块与函数（纵深防御，杜绝沙箱逃逸）
  const dangerousGlobals = [
    'os',
    'io',
    'package',
    'debug',
    'dofile',
    'loadfile',
    'load',
    'require',
    'module',
  ]
  for (const name of dangerousGlobals) {
    lua.lua_pushnil(L)
    lua.lua_setglobal(L, to_luastring(name))
  }

  // 2. 指令执行配额 hook（防止死循环卡死前端渲染）
  let instructionCount = 0
  lua.lua_sethook(
    L,
    (state: any) => {
      instructionCount += 1000
      if (instructionCount > MAX_INSTRUCTIONS) {
        lauxlib.luaL_error(
          state,
          to_luastring(`Lua 脚本执行超时：指令数已突破配额上限 (${MAX_INSTRUCTIONS})`),
        )
      }
    },
    lua.LUA_MASKCOUNT,
    1000,
  )

  // 3. 注册 risu 宿主 API 桥
  registerRisuBridge(L, ctx)

  return L
}

/** 在沙箱中执行任意 Lua 代码片段 */
export function runLuaScript(code: string, ctx: RisuBridgeContext): LuaExecutionResult {
  if (!code || !code.trim()) return { success: true }

  let L: any = null
  try {
    L = createSandboxedLuaState(ctx)
    const status = lauxlib.luaL_dostring(L, to_luastring(code))
    if (status !== lua.LUA_OK) {
      const errStr = to_jsstring(lua.lua_tolstring(L, -1))
      log.warn('LuaSandbox', `Lua 执行出错: ${errStr}`)
      return { success: false, error: errStr }
    }
    const top = lua.lua_gettop(L)
    const result = top > 0 ? pullFromLua(L, -1) : undefined
    return { success: true, result }
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    log.error('LuaSandbox', `沙箱捕获未处理异常: ${msg}`)
    return { success: false, error: msg }
  } finally {
    if (L) {
      try {
        lua.lua_close(L)
      } catch {
        /* 忽略虚拟机释放竞态 */
      }
    }
  }
}

/**
 * 执行 onPrompt 生命周期钩子：
 * 在构建大模型提示词之前触发，脚本可通过 return 字符串注入追加系统提示词
 */
export function runLuaOnPrompt(code: string, ctx: RisuBridgeContext): string {
  if (!code || !code.trim()) return ''
  let L: any = null
  try {
    L = createSandboxedLuaState(ctx)
    const status = lauxlib.luaL_dostring(L, to_luastring(code))
    if (status !== lua.LUA_OK) {
      const err = to_jsstring(lua.lua_tolstring(L, -1))
      log.warn('LuaSandbox', `onPrompt 脚本编译失败: ${err}`)
      return ''
    }

    // 检查是否存在全局 onPrompt 函数
    lua.lua_getglobal(L, to_luastring('onPrompt'))
    const t = lua.lua_type(L, -1)
    if (t === lua.LUA_TFUNCTION) {
      // 传入上下文概况
      pushToLua(L, {
        character: ctx.characterName,
        user: ctx.userName,
        chatLength: ctx.messages.length,
      })
      const callStatus = lua.lua_pcall(L, 1, 1, 0)
      if (callStatus !== lua.LUA_OK) {
        const err = to_jsstring(lua.lua_tolstring(L, -1))
        log.warn('LuaSandbox', `onPrompt 调用失败: ${err}`)
        return ''
      }
      if (lua.lua_isstring(L, -1)) {
        return to_jsstring(lua.lua_tolstring(L, -1)) || ''
      }
      log.warn('LuaSandbox', `onPrompt 返回类型非字符串: ${lua.lua_type(L, -1)}`)
    } else {
      log.warn('LuaSandbox', `onPrompt 全局变量类型非函数: ${t}`)
    }
    return ''
  } catch (e) {
    log.warn('LuaSandbox', `onPrompt 钩子执行失败: ${e}`)
    return ''
  } finally {
    if (L) {
      try {
        lua.lua_close(L)
      } catch {
        /* 忽略 */
      }
    }
  }
}

/**
 * 执行 onMessage 生命周期钩子：
 * 在接收到助手回复或用户发信后触发，更新好感度/装备/状态等变量
 */
export function runLuaOnMessage(
  code: string,
  ctx: RisuBridgeContext,
  messageText: string,
  role: string,
): void {
  if (!code || !code.trim()) return
  let L: any = null
  try {
    L = createSandboxedLuaState(ctx)
    const status = lauxlib.luaL_dostring(L, to_luastring(code))
    if (status !== lua.LUA_OK) return

    // 检查是否存在全局 onMessage 函数
    lua.lua_getglobal(L, to_luastring('onMessage'))
    if (lua.lua_isfunction(L, -1)) {
      pushToLua(L, messageText)
      pushToLua(L, role)
      const callStatus = lua.lua_pcall(L, 2, 0, 0)
      if (callStatus !== lua.LUA_OK) {
        const err = to_jsstring(lua.lua_tolstring(L, -1))
        log.warn('LuaSandbox', `onMessage 运行异常: ${err}`)
      }
    }
  } catch (e) {
    log.warn('LuaSandbox', `onMessage 触发异常: ${e}`)
  } finally {
    if (L) {
      try {
        lua.lua_close(L)
      } catch {
        /* 忽略 */
      }
    }
  }
}

/** 从角色卡对象提取 Lua 脚本内容 */
export function extractCardLuaScripts(charData: unknown): string[] {
  if (!charData || typeof charData !== 'object') return []
  const d = charData as Record<string, any>
  const scripts: string[] = []
  const ext = d.data?.extensions || d.extensions
  if (ext && typeof ext === 'object') {
    if (typeof ext.risu_script === 'string' && ext.risu_script.trim()) {
      scripts.push(ext.risu_script)
    }
    if (typeof ext.risuai?.script === 'string' && ext.risuai.script.trim()) {
      scripts.push(ext.risuai.script)
    }
    if (typeof ext.lua === 'string' && ext.lua.trim()) {
      scripts.push(ext.lua)
    }
    if (Array.isArray(ext.lua_scripts)) {
      for (const s of ext.lua_scripts) {
        if (typeof s === 'string' && s.trim()) scripts.push(s)
        else if (s && typeof s.content === 'string' && s.content.trim()) scripts.push(s.content)
      }
    }
  }
  return scripts
}
