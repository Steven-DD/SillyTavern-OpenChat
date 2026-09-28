import { createRouter, createWebHashHistory } from 'vue-router'

/**
 * 全局一致性规则（《界面设计文档 v1.0》）：
 * AppShell 是唯一布局壳 —— 左栏四入口全局固定，
 * 中栏(mid) 与右栏(main) 由路由命名视图按入口切换。
 * 采用 hash 模式：Tauri 生产环境 file:// 协议下无需服务端回退。
 */
const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', redirect: '/chats' },
    {
      path: '/chats',
      components: {
        mid: () => import('@/views/ChatsMid.vue'),
        main: () => import('@/views/ChatsMain.vue'),
      },
    },
    {
      path: '/contacts',
      components: {
        mid: () => import('@/views/ContactsMid.vue'),
        main: () => import('@/views/ContactsMain.vue'),
      },
    },
    {
      path: '/worlds',
      components: {
        mid: () => import('@/views/WorldsMid.vue'),
        main: () => import('@/views/WorldsMain.vue'),
      },
    },
    {
      path: '/personas',
      components: {
        mid: () => import('@/views/PersonasMid.vue'),
        main: () => import('@/views/PersonasMain.vue'),
      },
    },
    {
      path: '/plugins',
      components: {
        mid: () => import('@/views/PluginsMid.vue'),
        main: () => import('@/views/PluginsMain.vue'),
      },
    },
    {
      path: '/settings',
      components: {
        mid: () => import('@/views/SettingsMid.vue'),
        main: () => import('@/views/SettingsMain.vue'),
      },
    },
  ],
})

export default router
