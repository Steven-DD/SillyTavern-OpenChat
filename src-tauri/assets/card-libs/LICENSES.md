# 卡片前端界面 · 帧内预置库（批1）· 版本与许可登记

> 供 `src-tauri/src/backend/cardlibs.rs` 以 `include_bytes!` 内嵌、经 `GET /card-libs/<name>` 供给 srcdoc 帧。
> 清单与理由见 `docs/卡片前端界面_预置库注入方案_v1.0.md`；上游对照见 `docs/SillyTavern_CSP与卡内HTML应用_调研_2026-10-19.md`。

## 约定

- **文件名内嵌版本**（`<lib>-<version>.<file>`）：URL 即版本，允许 `Cache-Control: immutable` 长缓存，
  不存在"换库后 WebView 吃旧缓存"这一类问题。升级 = 新增文件名 + 同步前端清单 + 更新本表。
- 版本**一次性钉死**，跟随卡生态基线（jQuery 3.x 而非 4.x，js-yaml 4.1.0 与上游/卡内引用一致）。
- 取包方式：`npm install --no-save`（未写入 package.json，产物即登记物），随后复制 dist 产物。
- 校验和用于自检防漂移（`scripts/selfcheck-cardlibs.mjs`）。

## 批次 1

| 库 | 版本 | 文件 | 字节 | 帧内全局量 | 许可 | 许可文件 | sha256 |
|---|---|---|---|---|---|---|---|
| jQuery | 3.7.1 | `jquery-3.7.1.min.js` | 87533 | `$` / `jQuery` | MIT | `jquery.LICENSE.txt` | `fc9a93dd241f6b045cbff0481cf4e1901becd0e12fb45166a8f17f95823f0b1a` |
| toastr | 2.1.4 | `toastr-2.1.4.min.js` | 5251 | `toastr` | MIT（见下注） | — | `1e0c2ad4e069276efa1d43fd1f7549912bfd64219119037e26574f27ca4d7143` |
| toastr | 2.1.4 | `toastr-2.1.4.min.css` | 6454 | （样式） | MIT | — | `47dd690f8f315bea076e92581a7e7147443bb4c847e313ab5a7d50a8c44836d0` |
| lodash | 4.17.21 | `lodash-4.17.21.min.js` | 73015 | `_` | MIT | `lodash.LICENSE.txt` | `a9705dfc47c0763380d851ab1801be6f76019f6b67e40e9b873f8b4a0603f7a9` |
| js-yaml | 4.1.0 | `js-yaml-4.1.0.min.js` | 39430 | `jsyaml`（并别名 `YAML`） | MIT | `js-yaml.LICENSE.txt` | `45dc3dd03dc07a06705a2c2989b8c7f709013f04bd5386e3279d4e447f07ebd7` |
| Vue | 3.5.13 | `vue-3.5.13.global.prod.js` | 157924 | `Vue` | MIT | `vue.LICENSE.txt` | `c459ba7cc8db65c982589fa5d64c7ff478877e8e5b0fd75683207cec6a4e89e8` |
| Vue Router | 4.5.0 | `vue-router-4.5.0.global.prod.js` | 25396 | `VueRouter` | MIT | `vue-router.LICENSE.txt` | `a2755cf2acf83df38132e204cd21ade913a216658014a631716d795e9ea49c83` |

合计 395 003 B。

**toastr 许可注**：npm 包（`toastr@2.1.4`）**未随附 LICENSE 文件**，其 `package.json` 也无 `license` 字段；
上面标 MIT 是依其上游仓库（CodeSeven/toastr）的许可声明，**未从包内取证**。若后续要严格合规，
应从上游仓库取 LICENSE 原文补 `toastr.LICENSE.txt`。

## 批次 2

| 库 | 版本 | 文件 | 字节 | 帧内全局量 | 许可 | 许可文件 | sha256 |
|---|---|---|---|---|---|---|---|
| Font Awesome Free | 6.7.2 | `fontawesome-6.7.2/css/all.min.css`（注入） | 73890 | 图标字体 + `.fa`/`.fa-*` 类 | 字体 SIL OFL 1.1 / CSS MIT | `fontawesome.LICENSE.txt` | `74005d7c17d4a02f2f25404ec0655d9bc2fdaa53166874c87d7b7eec69d9088a` |
| Font Awesome Free | 6.7.2 | `fontawesome-6.7.2/webfonts/fa-solid-900.woff2` | 158220 | （CSS 相对引用） | 同上 | — | `aa75998623a391e61c6901794ace832e3ecdd288b56d608f21bea0411acc0b8e` |
| Font Awesome Free | 6.7.2 | `fontawesome-6.7.2/webfonts/fa-regular-400.woff2` | 25472 | （同上） | 同上 | — | `e3456d1283b9d75337a773dfd147bf908fd02c01b4bf48576d8603a69b13cbe5` |
| Font Awesome Free | 6.7.2 | `fontawesome-6.7.2/webfonts/fa-brands-400.woff2` | 118684 | （同上） | 同上 | — | `d7236a19bf23cbb2027280e8f51dc99d6c45976a2ed60de73382b034b18a2b68` |
| Font Awesome Free | 6.7.2 | `fontawesome-6.7.2/webfonts/fa-v4compatibility.woff2` | 4796 | （同上） | 同上 | — | `0ce9033c69dc714f5f45ef9bf17d55e4c46bcdfad6799a4e92b38e7781bf86bd` |
| jQuery UI | 1.13.3 | `jquery-ui-1.13.3/jquery-ui.min.js`（注入） | 254963 | `$.ui` / `$.fn.draggable|droppable|resizable|selectable|sortable|dialog|slider|tabs|datepicker|autocomplete|button|menu|tooltip|accordion` | MIT | `jquery-ui.LICENSE.txt` | `b30d2234d5e63896d085816e0bd385da43a50f929029ed72e657c19f80bd4a38` |
| jQuery UI | 1.13.3 | `jquery-ui-1.13.3/themes/base/theme.css`（注入） | 17438 | （主题样式） | MIT | — | `2a54fc5c6b0f941711299811e17dd9c18f8bba0ce6e8cb11ec418d64f44a6ca0` |
| jQuery UI | 1.13.3 | `…/themes/base/images/ui-icons_444444_256x240.png` | 7126 | （CSS 相对引用） | MIT | — | `bd0cd718fd018bf306c3327063eb10a7a929c0df9ac1b387192a574370c65625` |
| jQuery UI | 1.13.3 | `…/ui-icons_555555_256x240.png` | 7110 | （同上） | MIT | — | `7dfffbbe96d18869a4fd6266164671a7aa9e8396614b6008db01fcf71671fe02` |
| jQuery UI | 1.13.3 | `…/ui-icons_777620_256x240.png` | 4654 | （同上） | MIT | — | `8f8775b19cabddeebc3d14d34cfe796bd30d7ccfbc8b3ddda4d920efba75e7f8` |
| jQuery UI | 1.13.3 | `…/ui-icons_777777_256x240.png` | 7147 | （同上） | MIT | — | `3eb52a825d71a218153461e907580cc650a165aec2fc36e25bce5b177d5e77fb` |
| jQuery UI | 1.13.3 | `…/ui-icons_cc0000_256x240.png` | 4654 | （同上） | MIT | — | `8f98c3119565b3d2fabc79d090fe279fd73df290b18f347ff4afbab4f637216c` |
| jQuery UI | 1.13.3 | `…/ui-icons_ffffff_256x240.png` | 6523 | （同上） | MIT | — | `288abba3b3c02948d4b2317486b73ba46339ed4bd331a5ca40a4635477e7a32e` |
| jQuery UI Touch Punch | 0.2.3 | `jquery-ui-touch-punch-0.2.3.min.js`（注入） | 1291 | 无全局量（给 jQuery UI 补触摸事件） | MIT 或 GPL-2.0（双许可，见下注） | — | `000854d782781aff1b16ea5451c1da3d07efadd35ab911ccb7e4b851571a25bd` |
| Showdown | 2.1.0 | `showdown-2.1.0.min.js`（注入） | 75209 | `showdown`（`new showdown.Converter()`） | MIT | `showdown.LICENSE.txt` | `88eb6fbbe0c270ddf3384aee0c9620d070e090a26e07c67421ae36c903b5d649` |

批次 2 合计 766 523 B；连同批次 1 共 1 162 180 B。

**touch-punch 许可注**：包内无 LICENSE 文件，`package.json` 的 `license` 字段为
`"Dual licensed under the MIT or GPL Version 2 licenses."`（非 SPDX 标识）。我们按 MIT 使用；
若要严格合规应补一份双许可原文。

**版本选择依据（与「跟随最新」相反的刻意选择）**：

- **Font Awesome 钉 6.7.2，不跟 7.x**：最新已是 7.3.1，但卡生态广泛使用 `fa fa-xxx`
  老写法；6.7.2 的 `all.min.css` 保留 `.fa` 兼容类（已 grep 核实 `.fa{` 存在），
  升级到 7 是破坏性变更。
- **jQuery UI 钉 1.13.3，不用 1.14.1**：1.14 较新且主题产物结构有变化（1.14 的 dist 里
  没有 `theme.min.css`），1.13.3 是长期稳定基线。
- **只 ship woff2、不打包 ttf**：CSS 的 `src:` 列表 woff2 在前，现代内核（WebView2）
  必然选中；ttf 仅作兜底，省下 700 KB。
- CSS 里 `../webfonts/…`、`images/…` 这些**相对路径按目录结构原样保留**（名字即相对
  路径，路由整名精确匹配），因此不做任何 URL 改写。

## 批次 2 中**主动延后**：tailwindcss

上游酒馆助手注入的是 **v3 Play CDN 构建**（`cdn.tailwindcss.com` 一类脚本）。核对后
暂不内嵌，理由：

1. 该构建**不是常规 npm 产物**；npm 上能拿到的是 `@tailwindcss/browser@4.x`（v4 语义），
   与 v3 Play CDN **不等价**——v4 不支持卡里常见的 `tailwind.config = {…}` 全局配置写法。
2. 用 tailwind 的卡（含本批证据里的那张写卡助手模板）**通常自己引 `cdn.tailwindcss.com`**，
   而 CSP 放宽后这条已可直连；内嵌一份 v4 反而可能与卡自带的 v3 **样式互相打架**。
3. 正确的做法是"确需离线可用时再引入，且要与上游同一大版本"，属批次 3 的决策项。

## 与上游的差异（有意为之）


- 上游（酒馆助手）大部分库走 `https://testingcf.jsdelivr.net/...` CDN；我们**全部本地内嵌**，
  不引入运行时供应链与离线不可用面。
- 上游用宿主 `blob:` URL 注入自己的脚本；**我们不能**：帧是 sandbox 无 `allow-same-origin` 的
  不透明源，读不到宿主创建的 blob（实测脚本不执行、零 CSP 违规 ⇒ 源检查而非策略拦截）。
- `YAML` 别名（`window.YAML ||= window.jsyaml`）是**我们额外加的**兼容别名，未见上游证据。

## 批次 3（未落地，登记于此备查）

- zod（`z` 全局量）、Pixi.JS（体积大，按卡池实际需要再引）
- tailwindcss（见上「主动延后」：等与上游同大版本的离线构建方案明确后再做）
