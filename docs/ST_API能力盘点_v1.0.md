# SillyTavern API 能力盘点 v1.0

> **阶段三启动前置事项 ①**：前端功能 ↔ ST 后端 API 对照，识别缺口
> **盘点时间**：2026-09-22 凌晨
> **信息来源**（真实检索，非凭记忆）：
> - DeepWiki · SillyTavern/SillyTavern 官方仓库（AI Integration / Chat Completion System / Streaming 页）
> - DeepWiki · SillyTavernMOD fork 的 Server 路由表（fork 继承官方端点结构，作全景参考）
> - CSDN《SillyTavern API设计：RESTful接口规范》（/api/chats/get、/api/characters/all 等请求示例）
> - **标注规则**：✅ 已验证 / ◑ 路由前缀已验证、字段名待实测 / ⚠ 待实测确认

---

## 一、ST 架构速览（与本项目的关系）

```
┌────────────────────────────────────────────────┐
│ 我们的 Tauri 应用                               │
│  ┌──────────────┐      ┌─────────────────────┐ │
│  │ Vue 3 前端    │─HTTP→│ ST 后端 (Node/Express)│ │
│  │ (自研聊天UI)  │ ←SSE │  纯 API，无 UI 暴露   │ │
│  └──────────────┘      └─────────────────────┘ │
│      Tauri Rust 层：sidecar 进程管理             │
└────────────────────────────────────────────────┘
```

ST 后端 = Node.js/Express 服务，所有能力通过 `/api/*` REST 端点暴露。
**关键事实：ST 的 prompt 组装、流式解析、记忆注入等核心逻辑运行在其前端（public/scripts/）而非后端。** 后端主要负责：数据持久化、AI 提供商代理/格式转换、token 计算等"重活"。

## 二、总体结论

| 判定项 | 结论 |
|--------|------|
| Sidecar + 纯 API 方案可行性 | **可行** ✅ 后端能力完整暴露为 REST API |
| 基础聊天收发（含流式） | **有原生接口** ✅ 生成端点支持 SSE 透传 |
| 会话/角色卡/世界书/预设管理 | **有原生接口** ✅ CRUD 齐全 |
| 记忆三件套 | **部分原生** ◑ 向量存储有后端；总结记忆/作者注释是前端逻辑，需移植 |
| ST 现有扩展直接接入 | **不可直接接入** ⚠（详见第四节重大发现） |
| 自研前端整体工作量 | 中偏大：数据管理走 API 省力，**prompt 组装管线是自研重点** |

## 三、功能域逐项对照（对应需求文档 6.1–6.8）

### 6.1 对话与消息

| 前端功能 | ST API | 状态 |
|----------|--------|------|
| Chat Completion 生成（30+ 商业提供商） | `POST /api/backends/chat-completions/generate`，`stream:true` 时后端 `forwardFetchResponse` 直接透传提供商 SSE 流 | ✅ |
| Text Completion 生成（本地推理 KoboldCpp/llama.cpp/Ollama 等） | `POST /api/backends/text-completions/generate`，同样 SSE 透传 | ✅ |
| 流式解析 | ST 前端自研 `getEventSourceStream`/`parseStreamData`（因浏览器 EventSource 不支持 POST+自定义头）。**我们需自写同等解析器**（fetch + ReadableStream） | ◑ 自研 |
| 推理内容展示（DeepSeek-R1/Claude thinking 等） | 流块中的 `reasoning_content` / thinking 字段，前端提取 | ◑ |
| Token 计数 | `/api/tokenizers/*`（后端 SentencePiece/WebTokenizer）+ 前端 guesstimate 兜底 | ◑ 字段待实测 |
| 消息编辑/删除/重发 | 走 `/api/chats/save` 整体保存 JSONL（消息数组整体写回） | ✅ |
| Swipe（备选回复切换） | ST 前端逻辑 + 重新生成，消息数组含 swipe 记录 | ◑ 移植 |

### 6.2 会话管理

| 前端功能 | ST API | 状态 |
|----------|--------|------|
| 会话列表 | `/api/characters/chats`（角色名下会话列表，返回名称/大小/日期） | ◑ |
| 读取会话 | `POST /api/chats/get`（avatar_url + file_name → JSONL 消息数组） | ✅ |
| 保存会话 | `POST /api/chats/save`（消息数组整体写回，`force` 字段控制覆盖） | ✅ |
| 搜索/重命名/删除 | `/api/chats/search`、`/api/chats/rename`、`/api/chats/delete` | ◑ |
| 会话元数据 | JSONL 文件头（chat_metadata：分支/书签等） | ◑ |

### 6.3 角色卡管理（联系人）

| 前端功能 | ST API | 状态 |
|----------|--------|------|
| 角色列表（含标签） | `POST /api/characters/all`（返回 name/avatar/create_date/tags 等） | ✅ |
| 读取单个角色 | `POST /api/characters/get`（PNG base64 内嵌 V2/V3 卡数据） | ◑ |
| 新建/编辑 | `POST /api/characters/create`、`/edit`（multipart/form-data） | ✅ |
| 删除 | `POST /api/characters/delete` | ◑ |
| PNG/JSON 导入导出 | `/api/characters/import`、`/api/characters/export`（与 ST 生态互通，tEXt chunk 规范） | ✅ |
| 头像缩略图 | `/api/thumbnails`（GET，按类型+文件名取图） | ✅ |

### 6.4 世界书

| 前端功能 | ST API | 状态 |
|----------|--------|------|
| 世界书列表/读取 | `/api/worldinfo/get` | ◑ |
| 导入（外部 JSON） | `/api/worldinfo/import` | ◑ |
| 新建/编辑/保存 | `/api/worldinfo/edit` 等 worldinfo 路由组 | ◑ |
| **条目注入进 prompt** | **后端不管注入**——扫描/激活/注入逻辑在 ST 前端（world-info.js） | ⚠ 自研重点 |

### 6.5 预设与 Prompt

| 前端功能 | ST API | 状态 |
|----------|--------|------|
| 预设 CRUD（Chat Completion / Text Completion 两套） | `/api/presets/*`（保存至 preset 文件） | ◑ |
| 温度/top_p/最大 tokens 等参数 | 随生成请求 body 传入，预设管理在前端 | ◑ |
| **Prompt 组装管线**（系统提示+世界书+作者注释+聊天历史→消息数组） | **纯前端逻辑**（openai.js 的 ChatCompletion / prompt assembly pipeline） | ⚠ 自研重点 |

### 6.6 API 与模型（设置页核心）

| 前端功能 | ST API | 状态 |
|----------|--------|------|
| API Key 安全存储 | `/api/secrets/*`——服务端 secrets.json 加密存储；客户端只拿到 `secret_state` 布尔标志（不回传明文） | ✅（比"存本地前端"更安全，采纳 ST 方案） |
| 各提供商模型列表/连接测试 | 各 backend 路由（openai/anthropic/google/openrouter…）+ custom endpoint（OpenAI 兼容） | ◑ |
| 全局设置读写 | `/api/settings/get`、`/api/settings/save`（整个 settings.json） | ✅ |
| 背景图管理 | `/api/backgrounds/*` | ◑ |

### 6.7 Persona

| 前端功能 | ST API | 状态 |
|----------|--------|------|
| Persona 列表/头像 | `/api/avatars/*` | ◑ |
| Persona 数据 | 存于 settings.json（由前端管理） | ◑ |

### 6.8 扩展与插件 / 记忆三件套

| 功能 | ST 实现 | 我们的路线 |
|------|---------|-----------|
| 向量存储（Vector Storage） | 后端 `/api/vectors/*` 端点（向量化+检索）+ 前端编排 | **后端直接用**；检索编排逻辑移植 ◑ |
| 总结记忆（Summarize） | **纯前端扩展**（public/scripts/extensions/summarize）：定期用生成 API 摘要→存设置→注入 prompt | 前端自研移植（逻辑不复杂：阈值触发→摘要请求→注入） |
| 作者注释（Author's Note） | **纯前端**：固定深度插入消息流 | 前端自研（实现简单） |
| 表情（classify/sprites） | 后端 `/api/classify`（情感分类）+ `/api/sprites`（表情图） | P2，后续版本 |
| ST UI 扩展（第三方） | **重大发现，见第四节** | 按需移植，不直接运行 |

## 四、⚠ 重大发现：ST"扩展"无法直接接入自研前端

ST 的扩展生态分两类，接入方式完全不同：

1. **UI 扩展**（绝大多数，含 summarize）：manifest.json + index.js，**运行在 ST Web UI 的前端运行时里**，深度依赖其 event bus、聊天状态、DOM 结构。我们的自研前端**没有**这个运行时 → **无法直接运行任何 UI 扩展**。
2. **Server 插件**（server plugins）：Node 包，ST 启动时挂载到 `/api/plugins/*`，纯后端。→ **可以直接使用**。

**对"最小可接入插件"决策的修正（不推翻，细化）：**
- 首版记忆三件套 = 自研前端实现业务逻辑（参考 ST 扩展实现，AGPL 合规）+ 直接调用 ST 后端端点（vectors/generate/tokenizers）
- "最小可接入插件"首版落地为 **Server 插件接入**（后端能力扩展）
- 自建插件体系（阶段二预留的演进路径）= 前端插件运行时（event bus + 生命周期钩子），这是"万物可插件"的真正载体，列入 v2 规划
- **皮肤/主题扩展点不受影响**（本就是前端能力）

## 五、关键集成机制

| 机制 | 说明 | 应对 |
|------|------|------|
| SSE 流式 | POST + fetch ReadableStream 手动解析 `text/event-stream`（EventSource 不可用） | 自研 `sseStream.ts` 工具（参考 ST sse-stream.js 行为） |
| 认证/CSRF | 默认单用户模式仅监听 localhost，无登录；multi-user/BasicAuth 开启时有登录中间件与 CSRF 校验 | sidecar 固定 localhost + 关闭 multi-user，规避登录流；**开发首日实测确认请求头要求** |
| CORS | ST 默认不返回 CORS 头；Tauri WebView 的 origin（tauri://localhost）与 127.0.0.1 不同源 | 走 Tauri HTTP 通道（详见技术选型文档） |
| 数据目录 | ST 数据在 `data/<user>/`（characters/chats/worlds/settings/secrets…） | 随应用数据目录打包，升级锁版本 |

## 六、缺口清单与优先级

| 级别 | 缺口/工作量项 | 应对策略 |
|------|--------------|----------|
| P0 | Prompt 组装管线（世界书扫描+作者注释+聊天历史→消息数组） | 移植 ST 前端模块（AGPL 义务：本项目同样开源，合规 ✅） |
| P0 | SSE 流式解析器 + 流式 UI 不中断（对应设计文档"推挤不中断"约束） | 自研，阶段三第一个 spike 验证 |
| P0 | Tauri ↔ ST HTTP 通道（CORS/SSE 兼容） | 技术选型文档已定方案 + spike |
| P1 | 记忆三件套前端逻辑（summarize/author's note/vector 编排） | v1 按需求文档范围移植实现 |
| P1 | Swipe/书签/分支 | 需求文档 v1 未列必做，排 v1.1 |
| P2 | 表情/TTS/翻译/图像生成 | 不在首版，接入点已确认存在 |
| ⚠ | 各端点字段名、SSE 事件格式、CSRF 头细节 | **阶段三第一周逐个实测**，以 ST release 源码为准 |

## 七、合规与生态

- **AGPL-3.0**：ST 采用 AGPL-3.0。二开分发必须同协议开源——与老板"后续开源"计划一致，方向无冲突；移植 ST 前端代码须保留版权声明
- 角色卡：遵循 ST 社区 PNG 内嵌规范（tEXt chunk, chara_card_v2/v3），导入导出天然互通
- 版本策略：锁定 ST release 大版本，升级前跑数据迁移验证
