/**
 * RisuAI Lua 脚本宿主桥 (risu 模块)
 *
 * 向沙箱内 Lua 环境导出标准 `risu` API：
 * - risu.getVar(key, defaultVal)
 * - risu.setVar(key, val)
 * - risu.rollDice(d)
 * - risu.getLastMessage()
 * - risu.getChatLength()
 * - risu.getMessage(idx)
 * - risu.getCharacterName()
 * - risu.getUserName()
 * - risu.print(...) / risu.log(...)
 */
import { lua, to_luastring, to_jsstring } from 'fengari'
import { rollDice } from './macros'
import { log } from '@/services/log'

export interface RisuBridgeContext {
  characterName: string
  userName: string
  variables: Map<string, unknown>
  messages: Array<{ role: string; content: string }>
  onVarChange?: () => void
}

/** 辅助函数：将 JS 值推入 Lua 栈 */
export function pushToLua(L: any, val: unknown): void {
  if (val === null || val === undefined) {
    lua.lua_pushnil(L)
  } else if (typeof val === 'number') {
    if (Number.isInteger(val)) {
      lua.lua_pushinteger(L, val)
    } else {
      lua.lua_pushnumber(L, val)
    }
  } else if (typeof val === 'boolean') {
    lua.lua_pushboolean(L, val ? 1 : 0)
  } else if (typeof val === 'string') {
    lua.lua_pushstring(L, to_luastring(val))
  } else if (Array.isArray(val)) {
    lua.lua_newtable(L)
    val.forEach((item, idx) => {
      pushToLua(L, item)
      lua.lua_rawseti(L, -2, idx + 1)
    })
  } else if (typeof val === 'object') {
    lua.lua_newtable(L)
    for (const [k, v] of Object.entries(val)) {
      lua.lua_pushstring(L, to_luastring(k))
      pushToLua(L, v)
      lua.lua_rawset(L, -3)
    }
  } else {
    lua.lua_pushstring(L, to_luastring(String(val)))
  }
}

/** 辅助函数：从 Lua 栈读取 JS 值 */
export function pullFromLua(L: any, idx: number): unknown {
  const t = lua.lua_type(L, idx)
  switch (t) {
    case lua.LUA_TNIL:
      return null
    case lua.LUA_TBOOLEAN:
      return lua.lua_toboolean(L, idx) !== 0
    case lua.LUA_TNUMBER:
      return lua.lua_tonumber(L, idx)
    case lua.LUA_TSTRING:
      return to_jsstring(lua.lua_tolstring(L, idx))
    case lua.LUA_TTABLE: {
      const obj: Record<string, unknown> = {}
      lua.lua_pushnil(L)
      const absIdx = lua.lua_absindex(L, idx)
      while (lua.lua_next(L, absIdx) !== 0) {
        const k = String(pullFromLua(L, -2))
        const v = pullFromLua(L, -1)
        obj[k] = v
        lua.lua_pop(L, 1)
      }
      return obj
    }
    default:
      return null
  }
}

/** 将 risu 全局表注册绑定到 Lua 虚拟机中 */
export function registerRisuBridge(L: any, ctx: RisuBridgeContext): void {
  lua.lua_newtable(L)

  // 1. risu.getVar(key, defaultVal)
  lua.lua_pushjsfunction(L, (state: any) => {
    const key = to_jsstring(lua.lua_tolstring(state, 1))
    if (!key) {
      lua.lua_pushnil(state)
      return 1
    }
    const val = ctx.variables.get(key)
    if (val !== undefined && val !== null) {
      pushToLua(state, val)
    } else {
      // 检查是否有第二个默认值参数
      if (lua.lua_gettop(state) >= 2) {
        lua.lua_pushvalue(state, 2)
      } else {
        lua.lua_pushnil(state)
      }
    }
    return 1
  })
  lua.lua_setfield(L, -2, to_luastring('getVar'))

  // 2. risu.setVar(key, val)
  lua.lua_pushjsfunction(L, (state: any) => {
    const key = to_jsstring(lua.lua_tolstring(state, 1))
    if (!key) return 0
    const val = pullFromLua(state, 2)
    ctx.variables.set(key, val)
    ctx.onVarChange?.()
    return 0
  })
  lua.lua_setfield(L, -2, to_luastring('setVar'))

  // 3. risu.rollDice(d)
  lua.lua_pushjsfunction(L, (state: any) => {
    const top = lua.lua_gettop(state)
    if (top < 1) {
      pushToLua(state, rollDice('1d20') ?? 0)
      return 1
    }
    const type = lua.lua_type(state, 1)
    let expr = '1d20'
    if (type === lua.LUA_TNUMBER) {
      const sides = Math.floor(lua.lua_tonumber(state, 1))
      expr = `1d${Math.max(1, sides)}`
    } else if (type === lua.LUA_TSTRING) {
      expr = to_jsstring(lua.lua_tolstring(state, 1))
    }
    const res = rollDice(expr) ?? 0
    pushToLua(state, res)
    return 1
  })
  lua.lua_setfield(L, -2, to_luastring('rollDice'))

  // 4. risu.getLastMessage()
  lua.lua_pushjsfunction(L, (state: any) => {
    const last = ctx.messages[ctx.messages.length - 1]
    pushToLua(state, last?.content ?? '')
    return 1
  })
  lua.lua_setfield(L, -2, to_luastring('getLastMessage'))

  // 5. risu.getChatLength()
  lua.lua_pushjsfunction(L, (state: any) => {
    pushToLua(state, ctx.messages.length)
    return 1
  })
  lua.lua_setfield(L, -2, to_luastring('getChatLength'))

  // 6. risu.getMessage(idx) —— Lua 从 1 起始
  lua.lua_pushjsfunction(L, (state: any) => {
    const idx = Math.floor(lua.lua_tonumber(state, 1))
    const jsIdx = idx >= 1 ? idx - 1 : ctx.messages.length + idx
    const msg = ctx.messages[jsIdx]
    pushToLua(state, msg?.content ?? '')
    return 1
  })
  lua.lua_setfield(L, -2, to_luastring('getMessage'))

  // 7. risu.getCharacterName()
  lua.lua_pushjsfunction(L, (state: any) => {
    pushToLua(state, ctx.characterName)
    return 1
  })
  lua.lua_setfield(L, -2, to_luastring('getCharacterName'))

  // 8. risu.getUserName()
  lua.lua_pushjsfunction(L, (state: any) => {
    pushToLua(state, ctx.userName)
    return 1
  })
  lua.lua_setfield(L, -2, to_luastring('getUserName'))

  // 9. risu.print / risu.log
  const logFn = (state: any) => {
    const n = lua.lua_gettop(state)
    const parts: string[] = []
    for (let i = 1; i <= n; i++) {
      parts.push(String(pullFromLua(state, i)))
    }
    const msg = parts.join(' ')
    log.info('LuaScript', msg)
    return 0
  }
  lua.lua_pushjsfunction(L, logFn)
  lua.lua_setfield(L, -2, to_luastring('print'))
  lua.lua_pushjsfunction(L, logFn)
  lua.lua_setfield(L, -2, to_luastring('log'))

  // 将构造好的表赋给全局 `risu`
  lua.lua_setglobal(L, to_luastring('risu'))
}
