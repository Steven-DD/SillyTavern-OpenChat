# CSS → UnoCSS 整改计划 v1.0

> 2026-09-24 · 基于全量扫描（16 个文件 / 2219 行 scoped CSS）
> 目标：样式统一由 UnoCSS 管理 · 配色统一变量 · 每页预留背景图/背景色

---

## 一、现状扫描结论

| 文件 | CSS 行数 | 硬编码色值 | 媒体查询 | 用到的 token |
|---|---|---|---|---|
| `views/PluginsMain.vue` | 366 | 1 | 0 | 78 |
| `views/SettingsMain.vue` | 302 | 1 | 0 | 62 |
| `views/ChatsMain.vue` | 298 | 0 | 0 | 51 |
| `views/ContactsMain.vue` | 183 | 0 | 0 | 34 |
| `views/PersonasMain.vue` | 171 | 1 | 0 | 32 |
| `views/ContactsMid.vue` | 114 | 0 | 0 | 23 |
| `components/BootScreen.vue` | 110 | **16** ⚠️ | 0 | 14 |
| `views/ChatsMid.vue` | 109 | 0 | 0 | 20 |
| `views/PersonasMid.vue` | 108 | 2 | 0 | 24 |
| `views/PluginsMid.vue` | 103 | 0 | 0 | 24 |
| `components/RailNav.vue` | 98 | 0 | 0 | 14 |
| `components/DataDirCard.vue` | 86 | 0 | 0 | 23 |
| `layouts/AppShell.vue` | 67 | 0 | 0 | 9 |
| `views/SettingsMid.vue` | 53 | 0 | 0 | 9 |
| `components/MidPane.vue` | 34 | 0 | 1 | 3 |
| `components/Avatar.vue` | 17 | 0 | 0 | 3 |
| **合计** | **2219** | **21** | **1** | — |

**结论**：token 体系其实已经落地（几乎每个文件都在用 `var(--c-*)`），真正的欠账是：
1. **21 处硬编码色值**（BootScreen 占 16 处 —— 它是最早写的，漏了 token）
2. **媒体查询只有 1 处** —— 响应式适配基本还没铺到各页面
3. 2219 行 scoped CSS 尚未与 UnoCSS 协作

---

## 二、待修改清单（按批次）

### 批次 1 · 主题层（先做，其他都依赖它）
| 文件 | 改动 |
|---|---|
| `styles/tokens.css` | ① 补 4 个语义色 token（见第四节）② 新增 **每页背景** 变量组 ③ 补 `--c-shadow` 等被硬编码替代的变量 |
| `uno.config.ts` | ① theme.colors 映射**全部** token（现在只映射了 9 个）② 加 shortcuts（卡片/按钮/输入框等复用组合） |

### 批次 2 · 硬编码清零（低风险，立即可做）
| 文件 | 改动 |
|---|---|
| `components/BootScreen.vue` | 16 处硬编码 → token（`#07c160`→`--p-500`、`#1f2329`→`--c-text`、`#f2f3f5`→`--c-bg`…） |
| `views/PersonasMain.vue` / `SettingsMain.vue` | `#137333` → `--s-success`（2 处） |
| `views/PersonasMid.vue` | `#fff` → `--c-on-primary`（2 处） |
| `views/PluginsMain.vue` | `#fff` → `--c-on-primary`（1 处） |

### 批次 3 · 布局层转原子类（响应式主战场）
| 文件 | 改动 |
|---|---|
| `layouts/AppShell.vue` | 三栏/单栏骨架 → `flex` + `lg:flex-row` 断点原子类 |
| `components/MidPane.vue` | 宽度/铺满 → `w-230px lg:w-230px w-full`，去掉手写媒体查询 |
| `components/RailNav.vue` | 侧栏/底部 Tab 两形态 → 断点原子类 + `lg:flex-col` |
| `views/*Main.vue`（5 个）| 根容器、头部、滚动区 → 原子类；**每个页面加背景变量挂载点** |

### 批次 4 · 视觉层（卡片/按钮/输入框）
| 范围 | 策略 |
|---|---|
| 卡片、按钮、输入框、气泡、徽标等**重复出现的**视觉块 | 抽成 **UnoCSS shortcuts**（如 `card` / `btn` / `btn-danger` / `input` / `msg-bubble`），模板里写 `class="card"`，定义集中在 `uno.config.ts` |
| 一次性布局微调 | 直接写原子类 |
| 复杂动画/伪元素 | **保留 scoped CSS**（原子类不适合，硬转反而更难维护） |

---

## 三、整改计划（分 4 步走）

| 步骤 | 内容 | 验证方式 |
|---|---|---|
| **S1** | 批次 1 + 2：主题层扩充 + 硬编码清零 | `grep` 确认 0 硬编码；切三个主题目视无变化 |
| **S2** | 批次 3：布局层转原子类 + 各页背景挂载点 | 拖动窗口跨 900px：宽屏三栏 / 窄屏单栏两形态均正常 |
| **S3** | 批次 4：shortcuts 抽取 + 各视图视觉块替换 | 逐页对比整理前截图（重点：卡片间距、按钮态、气泡） |
| **S4** | 收尾：删冗余 scoped CSS、统一命名 | `vue-tsc` + 全页面走查 |

**关键约束（血的教训）**：
- ⚠️ **不要引 `@unocss/reset/tailwind.css`** —— 它的 `html{line-height:1.5}` 会把紧凑排版整体撑高（本次已踩）
- ⚠️ 每步只动一类，**不混着改** —— 出问题能立刻定位是哪类改动导致
- ⚠️ 每步完成后必须**跨断点看一次**，别只测宽屏

---

## 四、配色统一变量管理

### 现有（已够用，保留）
`--c-*` 表面色 · `--p-*` 主色阶(50~900) · `--s-*` 语义色(成功/警告/错误) · 圆角/字体

### 需补 4 个
| 新 token | 用途 | 替换掉 |
|---|---|---|
| `--c-on-primary` | 主色上的文字色 | `#fff`（3 处） |
| `--c-shadow` / `--c-shadow-strong` | 浮层阴影 | `rgb(0 0 0 / 8%)` |
| `--c-overlay` | 遮罩（已有 `--overlay`，统一命名） | — |
| `--s-success` 补强 | 已有，但被 `#137333` 绕过 | `#137333`（2 处） |

### 规范（写进项目约定）
1. 组件里**只允许** `var(--token)` 或 UnoCSS 映射类（`bg-canvas` / `text-ink-2`）
2. 禁止 `#hex` / `rgb()` 直接出现在 .vue 里 —— 用 eslint 规则或提交前 `grep` 卡住
3. 新增颜色必须先进 `tokens.css`，再在 `uno.config.ts` 里映射一次

---

## 五、每页背景图/背景色预留

### 设计（皮肤包只覆盖变量即可换背景）
`tokens.css` 增加：

```css
/* 每页背景：皮肤包覆盖这组变量即可（浅色/深色/暮光各自一套） */
--page-bg: var(--c-bg);              /* 页面底色（默认走全局底色） */
--page-bg-image: none;               /* 背景图：url(...) 或渐变 */
--page-bg-size: cover;
--page-bg-repeat: no-repeat;
--page-bg-blend: normal;             /* 图与底色混合模式 */
```

各页面用 `data-page` 属性挂载（**结构统一，方便皮肤按页覆盖**）：

```html
<main class="page" data-page="chats">
```
```css
.page {
  background-color: var(--page-bg);
  background-image: var(--page-bg-image);
  background-size: var(--page-bg-size);
  background-repeat: var(--page-bg-repeat);
  background-blend-mode: var(--page-bg-blend);
}
/* 按页覆盖（示例：聊天页放一张氛围背景） */
.page[data-page='chats'] {
  --page-bg: var(--page-chats-bg);
  --page-bg-image: var(--page-chats-image);
}
```

页面标识：`chats` / `contacts` / `plugins` / `settings` / `personas`（5 个）

**为什么这么做**：聊天页的真实需求就是「一张背景图 + 气泡浮在上面」（ST 原生做法），提前把变量留好，将来做「聊天背景设置」功能时零改造。

---

## 六、风险与回滚

| 风险 | 应对 |
|---|---|
| 全量转原子类引入视觉回归 | 分批做，每批单独验证；出问题只回滚该批 |
| 模板被原子类淹没、可读性下降 | 重复视觉块走 **shortcuts**（`class="card"`），不让模板长满类名 |
| 主题/皮肤被硬编码破坏 | 硬编码清零 + 规范卡口（第四节） |
| 响应式在窄屏露出新问题 | 每批完成后跨断点走查（900px 上下各看一轮） |

**不做的事**（明确排除）：
- ❌ 不引入成品组件库（Element Plus / Naive UI 等 —— 无移动端，且毁微信式调性）
- ❌ 不换前端框架（SvelteKit 只能省约 100KB，成本是全量重写）

---

## 七、需要你确认的 3 件事

1. **迁移策略**：上面是「分层迁移」（布局转原子类 + 视觉块走 shortcuts + 复杂样式保留 scoped）。
   如果你要的是**字面意义上「2219 行全部改成原子类」**，我也能做，但回归面 100%、模板会变长 —— 我建议按本方案走。**选哪个？**
2. **shortcuts 命名**：`card` / `btn` / `input` 这类短名，还是带前缀（`ui-card`）避免与原生标签语义混淆？
3. **首批范围**：建议先做 **批次 1 + 2**（主题层 + 硬编码清零，风险最低、收益立刻可见），你验收后再推进 3、4。
