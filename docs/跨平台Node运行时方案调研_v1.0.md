# 跨平台 Node 运行时方案调研 v1.0

> **调研目的**：ST Chat（Tauri 2 + Vue 3）当前 v1 依赖外部 SillyTavern 目录与系统 Node，无法跨平台分发。本文调研业界做法、官方约束与真实先例，给出跨平台策略建议。
>
> **调研时间**：2026-09-22
> **数据来源标注**：`[实测]` 本机真实测量 / `[文档]` 官方文档 / `[先例]` 真实开源项目 / `[待验证]` 尚未实证

---

## 一、结论速览（TL;DR）

| # | 结论 |
|---|---|
| 1 | **不要试图把 ST 编译成单文件**：ST 运行时从磁盘加载 `public/`（592 个文件）、16 个扩展目录、`default/` 骨架，还用了动态 `import()`。SEA / Bun `--compile` 的「单文件」前提与 ST 架构正面冲突。[实测] |
| 2 | **官方推荐的正确姿势**就是我们的形态：`stock Node 二进制作为 sidecar` + `JS 作为 resources`。Tauri 官方文档原话：「sidecar 只能捆绑自包含二进制，Node 应用很难打包 —— 可以把 stock Node 二进制作为 sidecar，把 JavaScript 文件作为 Resource」。[文档] |
| 3 | **安装包做小、运行时后取**是 ST 生态的主流做法。真实先例 `al01cn/sillyTavern-launcher`（**同样是 Tauri v2 + Vue 3 + Rust**）就是：安装包只装应用，Node 与 ST 首次运行时下载到可写数据目录。[先例] |
| 4 | **跨平台的核心不是「怎么塞进安装包」，而是「数据目录必须外移」**。macOS `.app` 包体只读、更新会整包替换；ST 又要写 `data/`。解法：`--dataRoot` 把可写数据指到系统用户数据目录（ST 官方支持该参数）。[实测+文档] |
| 5 | 各平台 Portable Node 官方分发包实测 29–49 MB（未压缩→解压后约 90–130 MB），6 个目标平台全部可下载。[实测] |

---

## 二、我们的硬约束（全部本机实测）

### 2.1 SillyTavern 的可打包性

| 项 | 实测值 | 对方案的影响 |
|---|---|---|
| 原生模块 `.node` | **0 个** | 纯 JS → 单文件编译方案**技术上不被原生依赖阻断** |
| 其它原生二进制（`.dll/.so/.dylib/.exe`） | **0 个** | 同上 |
| 模块体系 | ESM（`"type": "module"`） | SEA 需 `mainFormat:"module"`，**仅 Node 25.5+/26 支持** |
| 生产依赖数 | 96 个 | esbuild 打包可行但风险高（见下） |
| `engines` | `node >= 20` | 运行时最低版本要求 |

### 2.2 ⚠ 决定性的架构冲突：ST 不是「单文件应用」

| ST 运行时依赖磁盘的内容 | 规模 | 单文件方案能否处理 |
|---|---|---|
| `public/` 静态资源（它要 serve 给浏览器） | 592 个文件 | ❌ 编译后没有源文件树 |
| `public/scripts/extensions/` 运行时扩展 | 16 个目录 | ❌ 启动时扫描/加载 |
| `default/` 预设骨架（启动时拷贝） | 200 个文件 | ❌ 需真实文件 |
| 源码里的动态 `import()` | 3 处文件 | ⚠ esbuild 可处理但需小心 |
| 可写数据 `data/` | 需读写（会话/角色卡/密钥） | ❌ 只读包体写不了 |

> 真实先例的教训可以直接引用：那位用 Bun 编译 Node 服务的作者踩到 ——
> 「Static files have no source tree any more ... `__dirname` 是虚拟路径，可执行文件旁边没有 `public/` 目录」，
> 最后不得不把静态文件改成 Tauri resource 再把解析后的路径通过环境变量传进去。[先例]

**结论：ST 必须作为「目录树」存在，不能塞进一个可执行文件。**

### 2.3 可写数据必须外移（跨平台关键）

`[实测]` ST 支持 `--dataRoot <path>` 命令行参数（`src/command-line.js:212`，且 CLI 优先于 `config.yaml` 的 `dataRoot: ./data`）。

这一条打通了整个跨平台设计：

```
应用只读区（安装包内）        →  ST 源码 + node_modules + public/ + default/
用户可写区（系统用户数据目录）→  data/（会话、角色卡、密钥、世界书…）
```

于是 ST 版本升级 = 换只读区，**用户数据零风险**；macOS 只读 `.app` 包体也不再是问题。

### 2.4 体积账

| 项 | 实测体积 |
|---|---|
| ST 全量（含 node_modules） | **501 MB** / 25,620 个文件 |
| ├─ `node_modules` | 374 MB |
| ├─ `data/`（用户数据，**不应打包**） | 30 MB |
| └─ `src` + `public` + `default` 等 | 约 97 MB |
| `node.exe`（单平台运行时） | 83 MB |
| 当前应用本体（exe / NSIS 安装包） | 7.9 MB / **2.2 MB** |

**各平台官方 Portable Node 分发包**（`[实测]` nodejs.org HTTP 200 + Content-Length）：

| 目标平台 | 分发文件 | 压缩包 |
|---|---|---|
| Windows x64 | `node-v22.22.2-win-x64.zip` | **34.0 MB** |
| Windows arm64 | `node-v22.22.2-win-arm64.zip` | 29.9 MB |
| macOS arm64 | `node-v22.22.2-darwin-arm64.tar.gz` | 47.7 MB |
| macOS x64 | `node-v22.22.2-darwin-x64.tar.gz` | 48.8 MB |
| Linux x64 | `node-v22.22.2-linux-x64.tar.xz` | 29.6 MB |
| Linux arm64 | `node-v22.22.2-linux-arm64.tar.xz` | 28.8 MB |

> 官方分发包是**免安装绿色包**（解压即用），因此既可作为 sidecar 直接随包分发，也可作为首次运行的下载源。

---

## 三、四条路线的对比与裁决

### 路线 A：安装包自包含（node 作 sidecar + ST 作 resources）

| 维度 | 评估 |
|---|---|
| 做法 | `bundle.externalBin: ["binaries/node"]`（每平台一份 `node-<target-triple>[.exe]`）+ `bundle.resources` 打入 ST 目录树 |
| 优点 | 装完即用，零网络依赖，最接近「传统桌面软件」体验 |
| 缺点 | **安装包暴涨**：单平台 node(~34MB) + ST 源码(~97MB，不含用户数据) → NSIS 约 100–130 MB；多平台需各出一份 |
| 额外代价 | ST 更新要重发安装包；macOS 需对 node 可执行文件与整个包体签名/公证 |
| 适用 | 面向小白用户的正式分发版 |

### 路线 B：安装包轻量 + 首次运行获取 ★ 推荐

| 维度 | 评估 |
|---|---|
| 做法 | 安装包只含应用（**2.2 MB**）。首次运行引导：① 复用系统已有 Node，或 ② 下载官方 Portable Node 到 `<appDataDir>/runtime/node/`；ST 目录 ③ 用户指定已有目录，或 ④ 从 GitHub Releases 下载解压到 `<appDataDir>/sillytavern/<version>/` |
| 真实先例 | `al01cn/sillyTavern-launcher`（Tauri v2 + Vue 3 + Rust）：`data/node/`（可选内置 Node）、`data/sillytavern/<version>/`、`data/st_data/`；带 `download-progress` / `install-progress` / `process-log` / `process-exit` / `repair-missing-deps` 事件；发布 `.msi/.exe`、`.dmg`、`.AppImage/.deb` |
| 优点 | 安装包极小；Node 与 ST 可独立升级；天然规避 macOS 包体只读问题；对开发者友好（复用本机已有环境） |
| 缺点 | 首次运行需网络（可提供离线包导入兜底）；要写下载/解压/校验/进度 UI |
| 备注 | 该先例还内置 **MinGit/PortableGit**（`data/mingit/`）—— ST 的扩展安装依赖 git，跨平台时这是被容易忽略的第三个运行时依赖 |

### 路线 C：编译成单文件（Node SEA / Bun `--compile`）

| 维度 | Node SEA | Bun `--compile` |
|---|---|---|
| 现状 | `[文档]` Node 25.5.0（2026-01）起一条命令 `node --build-sea sea-config.json`（注入逻辑内建，不再依赖 postject）；2026-02 的 PR #61813 才加上 ESM 入口（`mainFormat:"module"`）。**Node 24 LTS 尚未回移该命令** | `[文档]` 一条命令，支持交叉编译；1.3.9 起字节码优化使启动比 SEA+cache 快约 25% |
| 前提 | 必须先把整个应用 esbuild 打包成**单个文件**；原生模块须保持 external | 仅纯 JS 依赖可行 |
| 对 ST 的裁决 | ❌ **不适用**。即便 ST 无原生模块（实测 0），它仍需 `public/` 592 文件、16 个扩展目录、`default/` 骨架的**真实文件树**；且 96 个依赖 + 3 处动态 import 的打包正确性无法保证 | ❌ 同上 |
| 跨平台注意 | SEA 跨平台构建时 `useCodeCache`/`useSnapshot` 必须为 false（code cache 与平台绑定） | 支持交叉编译，但仍受「单文件」限制 |

> 路线 C 适合「自己的小服务」，不适合「把别人的大型服务塞进去」。**本文档建议明确排除。**

### 路线 D：换掉 Node（Rust 重写后端 / 内嵌 JS 引擎）

`[先例]` Android 版 `sillytavern-launcher-mobile` 走的是**把 libnode 作为原生库嵌入**（APK 500MB+），而非重写。
`[评估]` Rust 重写 ST 后端（Express + 96 依赖 + 完整角色扮演管线）工作量不可接受；嵌入式 JS 引擎（rquickjs / deno_core）要补齐完整 Node API 与 `node_modules` 解析，同样不现实。
**裁决：不采纳，仅作背景。**

---

## 四、跨平台关键技术事实（官方文档）

### 4.1 sidecar 的命名与打包规则

- `externalBin` 里写**不带后缀的路径**（stem），Tauri 构建时自动追加 `-<target-triple>`；Windows 连 `.exe` 都是自动补的（声明时**不要**写 `.exe`）。[文档]
- 获取本机三元组：`rustc --print host-tuple`（Rust 1.84+）或 `rustc -Vv | grep host`。
- 交叉编译时注意：Tauri 官方示例的「自动重命名脚本」**在交叉编译场景不可靠**，必须按目标平台准备对应文件。

### 4.2 各平台目标三元组与安装包格式

| 平台 | 架构 | 目标三元组 | 安装包 |
|---|---|---|---|
| Windows | x64 | `x86_64-pc-windows-msvc` | NSIS `.exe` / MSI |
| Windows | arm64 | `aarch64-pc-windows-msvc` | NSIS `.exe` |
| macOS | Intel | `x86_64-apple-darwin` | `.dmg` |
| macOS | Apple Silicon | `aarch64-apple-darwin` | `.dmg` |
| Linux | x64 | `x86_64-unknown-linux-gnu` | `.AppImage` / `.deb` |
| Linux | arm64 | `aarch64-unknown-linux-gnu` | `.AppImage` / `.deb` |

### 4.3 macOS 的两个硬要求

1. **sidecar 必须一并签名**：官方文档指出「编译出的 sidecar 是独立可执行文件，必须与主应用一同签名」，Tauri 的 `bundle.macOS.signingIdentity` / `entitlements` 会覆盖 `externalBin` 中的二进制；entitlements 还需包含 sidecar 使用 stdio 等资源的权限。[文档]
2. **`.app` 包体不可依赖写入**：更新会整包替换 → 可写数据必须放用户数据目录（正对应 §2.3 的 `--dataRoot` 设计）。

### 4.4 子进程生命周期必须自己管

官方明确：「Tauri 负责打包 sidecar，但**运行与退出时清理由你负责**，否则会在用户机器上留下孤儿进程」。[文档]
→ 我们 v1 已实现（`st_sidecar.rs` 的 `managed` 标记 + `taskkill /T /F`），且刻意做到**只清理自己拉起的**、外部实例绝不接管。

### 4.5 resources 与 externalBin 的分工

`bundle.resources` 支持文件与目录映射（示例：`{"workers/bun-worker.js": "workers/bun-worker.js"}`）。[文档]
→ 正好承载 ST 的目录树（路线 A）或首运行下载后落地的目标位置（路线 B 实际上不用 resources，直接落用户数据目录）。

---

## 五、推荐方案

### 5.1 分阶段路线

| 阶段 | 目标 | 做法 |
|---|---|---|
| **v1（现状）** | 自用可用 | 依赖外部 ST 目录 + 系统 Node；`sidecar.json` / 环境变量可配 |
| **v1.1** | 跨平台可分发的技术底座 | ① 抽出「运行时解析器」：`system node → 内置 runtime/node → 下载`；② 抽出「ST 目录解析器」：用户指定 → 下载解压 → 校验；③ 引入 `--dataRoot` 指向用户数据目录（**数据与源码分离**）；④ 三平台持久化路径改用 Tauri 的 `app_data_dir` / `app_config_dir` API |
| **v1.2** | 首次运行引导 UI | 引导页：检测/下载 Node（带进度）、选择或下载 ST、数据目录说明、Git 可用性提示 |
| **v2（可选）** | 一键自包含安装包 | 路线 A：按平台把 Portable Node 打进 `externalBin`、ST 源码树打进 `resources` |

### 5.2 现在就该落地的两件事（低成本、高收益）

1. **引入 `--dataRoot`**：现在就把 `data/` 从 ST 目录里摘出来指到用户数据目录。这是 v1.1 的地基，越早做迁移成本越低（ST 目录将被整体替换）。
2. **运行时与目录解析集中化**：目前 `st_sidecar.rs` 里的 `resolve_st_dir` / `resolve_node` 已经是唯一入口，下一步把「候选来源」从「环境变量/配置/资源目录/开发兜底」扩展为「+ 内置运行时 + 下载」，改动面可控。

### 5.3 三个容易漏掉的跨平台坑

| 坑 | 说明 | 应对 |
|---|---|---|
| **Git 依赖** | ST 的扩展安装走 git 命令，纯净系统没有 | 内置 PortableGit/MinGit（先例做法），或降级为「ZIP 导入扩展」 |
| **Linux glibc** | 官方 Node 二进制要求较新的 glibc | 面向发行版较新的目标；或提示用户用系统包管理器装 Node |
| **Windows 安装目录** | ST 官方明确「不要装在 Program Files，它需要写权限」 | 我们用 `installMode: currentUser` + `dataRoot` 外移，天然规避 |

---

## 六、待验证项

| 项 | 为什么重要 | 验证方式 |
|---|---|---|
| ST 除 `dataRoot` 外是否还写别的目录（如 `config.yaml`、`backups/`） | 决定只读区是否真的只读 | 干净环境跑一遍，监控写操作 |
| ST 的 `backups/`、`plugins/`、`extensions/` 是否也需可写 | 同上 | 读源码 + 实测 |
| 官方 Portable Node 的 arm64 macOS 与 Windows arm64 是否够用 | 覆盖 Apple Silicon / Windows on ARM | 各自平台跑一遍 ST 启动 |
| Bun `--compile` 对 ST 是否真的不可行（做一次否定性验证） | 排除「更省事的方案」 | 试验性打包 + 启动，记录失败点 |
| macOS 签名/公证链路（含 sidecar） | 分发必经 | 需 Apple 开发者账号，`[待验证]` |

---

## 七、参考来源

1. **Tauri 官方 · Node.js 作为 sidecar**：`https://v2.tauri.app/learn/sidecar-nodejs/` — externalBin 的 target triple 命名、capabilities 配置、JS/Rust 两种调用方式
2. **Tauri 官方 · Embedding External Binaries（sidecar）**：`https://tauri.app/develop/sidecar` — 「sidecar 只能捆绑自包含二进制；Node 应用可把 stock Node 作 sidecar、JS 作 resource」、`rustc --print host-tuple`、退出清理责任
3. **Node.js 官方 · Single executable applications**：`https://nodejs.org/api/single-executable-applications.html` — SEA 配置字段、ESM 支持、签名、跨平台注意（`useCodeCache`/`useSnapshot` 须 false）
4. **Node SEA 2026 生产指南**：`https://www.hirenodejs.com/blog/nodejs-single-executable-applications-2026` — 单文件前必须 esbuild 打包、原生模块须 external、构建→签名→分发流水线
5. **实战复盘 · 用 Tauri + Bun 分发 Node 服务**：`https://dev.to/riponcm/shipping-a-nodejs-server-as-a-native-desktop-app-with-tauri-and-bun-mok` — `find node_modules -name "*.node"` 的 go/no-go 检查；`__dirname`/静态文件失效、包体只读、target triple 命名、drag-drop 与下载处理等真实坑
6. **真实先例 · al01cn/sillyTavern-launcher（Tauri v2 + Vue 3 + Rust）**：`https://github.com/al01cn/sillyTavern-launcher` — `data/node/`、`data/sillytavern/<version>/`、`data/st_data/`、`data/mingit/` 布局；下载/安装进度事件；三平台产物
7. **真实先例 · sillytavern-launcher-mobile**：`https://github.com/al01cn/sillytavern-launcher-mobile` — Android 嵌入 libnode 作原生库（APK 500MB+），说明「重写/嵌入」路线的代价
8. **ST 官方 CLI 参数表**（含 `--dataRoot`）：SillyTavern 仓库 README「Supported arguments」

---

_文档版本 v1.0 · 调研稿 · 2026-09-22_
