/**
 * UnoCSS 配置
 *
 * ── 为什么是 UnoCSS 而不是成品组件库 ──
 * 本项目的视觉是微信式极简自绘（不是 Material / 中后台风），
 * 且已有完整的语义 token 体系（`src/styles/tokens.css`）。
 * 引入 UnoCSS 只为了两件事：**响应式断点**与**开发效率**。
 *
 * ── 铁律 ──
 * 1. 颜色一律走语义 token（下方 theme.colors 的映射），**禁止 `bg-blue-500` 这类硬编码**
 * 2. **不要引 `@unocss/reset/tailwind.css`** —— 它给 html 设 line-height:1.5，
 *    会把本项目的紧凑排版整体撑高（踩过一次）
 * 3. 重复出现的视觉块（卡片/按钮/输入框）统一用下方 shortcuts，不要在页面里各写一份
 */
import { defineConfig, presetUno, transformerDirectives } from 'unocss'

export default defineConfig({
  presets: [presetUno()],
  // 允许在 <style> 里写 @apply / --uno:（把原子类复用到老样式里）
  transformers: [transformerDirectives()],

  /**
   * 扫描范围**必须收窄**：默认会把项目里所有文本文件当源码扫，
   * 于是文档/产物里的普通英文单词（h3、mt、ring、ease…）会被当成类名生成 CSS，
   * 更危险的是撞车 —— 例如框架里写了个 `class="me"`，
   * 而 `me` 恰好是 UnoCSS 的 `margin-inline-end` 工具类，元素会被莫名加上右边距。
   */
  content: {
    pipeline: {
      // 只扫组件与脚本（不含 .css/.html）：否则 CSS/HTML 里的英文词与标签名
      // （h3、ring、mt…）会被当成类名，生成一堆无用 CSS
      include: [/\.(vue|ts|tsx|js|jsx)($|\?)/],
      exclude: [/node_modules/, /dist/, /\.git/, /\.md$/, /\.json$/, /\.(svg|png|jpg)$/],
    },
  },

  theme: {
    // 断点：与 stores/layout.ts 的 BREAKPOINT(900) 保持一致
    breakpoints: {
      sm: '640px',
      md: '768px',
      lg: '900px', // 三栏的最低宽度
      xl: '1280px',
    },
    colors: {
      /* 表面（--c-*） */
      bg: 'var(--c-bg)',
      canvas: 'var(--c-canvas)',
      panel: 'var(--c-panel)',
      line: 'var(--c-border)',
      'line-strong': 'var(--c-border-strong)',
      ink: 'var(--c-text)',
      'ink-2': 'var(--c-text-2)',
      'ink-3': 'var(--c-text-3)',
      'on-brand': 'var(--c-on-primary)',
      /* 主色阶（--p-*） */
      brand: 'var(--p-500)',
      'brand-50': 'var(--p-50)',
      'brand-100': 'var(--p-100)',
      'brand-200': 'var(--p-200)',
      'brand-300': 'var(--p-300)',
      'brand-400': 'var(--p-400)',
      'brand-600': 'var(--p-600)',
      'brand-700': 'var(--p-700)',
      /* 语义（--s-*） */
      success: 'var(--s-success)',
      'success-bg': 'var(--s-success-bg)',
      warning: 'var(--s-warning)',
      'warning-bg': 'var(--s-warning-bg)',
      error: 'var(--s-error)',
      'error-bg': 'var(--s-error-bg)',
      /* 气泡 */
      'bubble-ai': 'var(--bubble-ai)',
      'bubble-user': 'var(--bubble-user)',
    },
    borderRadius: {
      sm: 'var(--radius-sm)',
      md: 'var(--radius-md)',
      lg: 'var(--radius-lg)',
    },
    boxShadow: {
      card: 'var(--c-shadow)',
      pop: 'var(--c-shadow-strong)',
    },
  },

  shortcuts: {
    /* ---- 视觉块（原各页面重复定义的 .card / .ghost / .in 统一到这里）---- */
    card: 'bg-canvas border border-solid border-line rounded-lg p-[13px_15px]',
    'card-hd': 'text-13px font-600 mb-[9px]',
    hint: 'text-11px leading-[1.7] text-ink-3',

    // 按钮：中性（原 .ghost）
    btn: 'px-12px py-6px text-12px border border-solid border-line rounded-md text-ink-2 bg-[var(--c-bg)] whitespace-nowrap hover:border-brand-400 hover:text-brand-600 disabled:(opacity-55 cursor-not-allowed)',
    'btn-primary': 'btn bg-brand border-brand text-on-brand font-semibold hover:bg-brand-600 hover:border-brand-600 hover:text-on-brand',
    'btn-danger': 'btn bg-error border-error text-on-brand font-semibold hover:opacity-88',

    // 输入
    field: 'w-full px-10px py-7px text-12px text-ink bg-[var(--c-bg)] border border-solid border-line rounded-md outline-none focus:border-brand-400 [font-family:inherit]',
    fld: 'block mb-10px',
    'field-select': 'field appearance-none',
    'field-area': 'field resize-y leading-[1.6]',

    /* ---- 布局 ---- */
    'row-center': 'flex items-center gap-2',
    col: 'flex flex-col',
    /** 页面主体容器（配合 class="page" 使用；背景由 base.css 的 .page 规则提供） */
    'col-fill': 'flex flex-col flex-1 min-w-0',

    /* ---- 侧栏 / 底部 Tab 两形态（窄屏底部、宽屏侧边；断点 lg=900px）----
     * ⚠ border-* 教训：`border-solid` 只设 border-style，border-width 的 CSS 初始值是
     *   medium（3px）！必须把不要的边显式写 0，否则凭空长出 3px 粗框（踩过一次）。 */
    'rail-bottom':
      'w-full flex-none flex flex-row items-center justify-around py-4px bg-canvas border-t border-r-0 border-b-0 border-l-0 border-solid border-line',
    'rail-side':
      'w-60px h-full flex-none flex flex-col items-center gap-6px py-14px bg-canvas border-r border-t-0 border-b-0 border-l-0 border-solid border-line',
    /** 导航项：窄屏竖排(图标+文字)、宽屏只留图标 */
    'rail-item': 'flex flex-col items-center justify-center gap-2px px-0 py-2px color-ink-2 hover:color-brand-600',
    'rail-item-side': 'w-36px h-36px rounded-md flex items-center justify-center',

    /* ---- 中栏 / 内容栏 ----
     * 宽度/边框只在这里定义（AppShell 的包裹层负责 flex：窄屏 flex-1、宽屏 lg:flex-none）。
     * lg: 变体写在 shortcut 内部是支持的（会展开为独立媒体查询规则）。 */
    'pane-mid':
      'w-full lg:w-230px flex flex-col min-h-0 bg-canvas border-r-0 border-t-0 border-b-0 border-l-0 lg:border-r border-solid border-line',
    'pane-main': 'flex-1 flex flex-col min-w-0 min-h-0 w-full',
  },
})
