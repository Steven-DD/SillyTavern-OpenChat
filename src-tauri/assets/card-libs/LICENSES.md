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

## 与上游的差异（有意为之）

- 上游（酒馆助手）大部分库走 `https://testingcf.jsdelivr.net/...` CDN；我们**全部本地内嵌**，
  不引入运行时供应链与离线不可用面。
- 上游用宿主 `blob:` URL 注入自己的脚本；**我们不能**：帧是 sandbox 无 `allow-same-origin` 的
  不透明源，读不到宿主创建的 blob（实测脚本不执行、零 CSP 违规 ⇒ 源检查而非策略拦截）。
- `YAML` 别名（`window.YAML ||= window.jsyaml`）是**我们额外加的**兼容别名，未见上游证据。

## 批次 2/3（未落地，登记于此备查）

- 批2：Font Awesome（CSS + `webfonts/` 相对路径整包）、jQuery UI、jquery-ui-touch-punch、showdown、tailwindcss
- 批3（按需）：zod、Pixi.JS
