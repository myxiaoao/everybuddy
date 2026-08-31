# EveryBuddy 技术设计

> 状态：v0.1 实现基线
> 目标平台：macOS、Windows
> 配置目标：WorkBuddy、CodeBuddy

## 1. 文档范围

EveryBuddy 是一个本地优先的桌面应用。用户添加 OpenAI-compatible API 后，EveryBuddy 发现模型、解析模型能力，并把模型配置发布到 WorkBuddy、CodeBuddy 或同时发布到两个产品。

本文说明首版的系统边界、模块职责、数据协议、发布事务、安全模型和验证要求。UI 规范见 [UI_DESIGN.md](./UI_DESIGN.md)。

## 2. 事实来源

| 类型     | 来源                                                                                                                       | 用途                             |
| -------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| 官方资料 | [WorkBuddy 模型功能说明](https://www.codebuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Model) | 理解第三方模型配置入口和能力字段 |
| 本机观察 | WorkBuddy 的 `~/.workbuddy/models.json`                                                                                    | 确认当前数组格式和实际字段       |

本机观察不是 WorkBuddy 或 CodeBuddy 的稳定公开协议。EveryBuddy 使用独立 Target Adapter 隔离未来变化。

## 3. 产品边界

首版包含：

- OpenAI-compatible Bearer Token Gateway。
- `GET /v1/models` 模型发现。
- 同时保存多个 Gateway Profile，每个 Gateway 独立维护模型集合和 Token 引用。
- 在 `/v1/models` 未返回完整列表时，允许用户在指定 Gateway 下手动添加模型。
- 用户主动触发的 `/v1/chat/completions` 能力 Probe。
- OpenRouter 公开模型目录、Gateway metadata、Probe 和人工覆盖。
- WorkBuddy、CodeBuddy 单目标或双目标发布。
- 配置预览、未知字段保留、原子写入、备份恢复和 Drift 检测。
- 启动时从 WorkBuddy 和 CodeBuddy 恢复 API、模型和发布选择状态。
- 简体中文和 English、Light、Dark、System。

首版不包含非 OpenAI-compatible 协议、非 Bearer 认证、本地 Proxy、Failover、云同步、用量统计、团队权限、MCP、Skills 和系统托盘切换。

## 4. 系统架构

```text
React + TypeScript + shadcn/ui
                 |
          Typed Tauri IPC
                 |
+----------------v------------------------------------+
|                     Rust Core                       |
|                                                     |
| Target Import Service ---> Capability Resolver      |
|          |                    ^    |                |
| Gateway Client ---> OpenRouter Directory Cache      |
|          |                         |                |
|          +----------------> Model Library <-> SQLite |
|                                    |                |
|                           Publish Coordinator       |
|                         /             \             |
|          WorkBuddy Adapter       CodeBuddy Adapter  |
|                 |                    |              |
|             Backup / Restore / Drift Service        |
+-----------------|--------------------|--------------+
                  |                    |
  ~/.workbuddy/models.json   ~/.codebuddy/models.json

Token -> macOS Keychain / Windows Credential Manager
```

### 4.1 前端

- React 负责交互状态和视图，不直接访问文件系统、SQLite 或系统凭据库。
- shadcn/ui 提供 Dialog、Button、Input、Checkbox、Switch 和 Tooltip 等基础控件。
- `src/lib/api.ts` 是唯一 IPC 入口。正式 Tauri 环境调用 Rust command；`?demo=1` 仅用于浏览器视觉验收。
- Target 状态和模型匹配状态每 5 秒轮询一次，用于识别外部文件修改。轮询不导入凭据，不调用 Gateway API，也不执行 Probe。

### 4.2 Rust Core

| 模块                 | 职责                                                                          |
| -------------------- | ----------------------------------------------------------------------------- |
| `gateway.rs`         | URL 规范化、模型发现、主动 Probe、网络错误分类                                |
| `market_catalog.rs`  | OpenRouter 公开模型目录与模型详情读取、边界校验、精确匹配、磁盘缓存和请求合并 |
| `gateway_service.rs` | Gateway Profile 与系统凭据的补偿式保存和删除                                  |
| `capability.rs`      | Capability Evidence 合并、Gateway metadata 解析和 Vendor 推断                 |
| `store.rs`           | SQLite connection、transaction 边界和 Repository facade                       |
| `store/migration.rs` | 版本化 schema migration 和迁移前备份                                          |
| `store/queries.rs`   | Gateway、模型的 SQL row codec 和 JSON serialization                           |
| `secrets.rs`         | Keychain / Credential Manager 读写                                            |
| `target.rs`          | Target Adapter、schema codec、摘要、权限和原子写入                            |
| `target_import.rs`   | 启动导入、Target 模型匹配、结构化跳过原因和 Token 隔离                        |
| `publish.rs`         | 发布预览、冲突确认、备份、补偿回滚和恢复                                      |
| `commands.rs`        | 可序列化的 Tauri command 边界                                                 |

## 5. 数据模型

SQLite 文件位于 Tauri `app_data_dir/everybuddy.db`。

数据库使用 `PRAGMA user_version` 管理 schema，当前 `SCHEMA_VERSION` 为 `3`。已有旧版数据库升级前会通过 SQLite Backup API 保存到同级 `migration-backups/` 目录；v3 会清空旧版已截断且无法可靠还原的 Custom Protocol `endpointOverride`，要求用户重新填写完整请求 URL。高于当前版本的数据库会被拒绝打开，避免旧版本应用写坏新 schema。

- `gateway_profiles`：保存名称、规范化 `api_root`、`token_ref` 和时间戳，不保存 Token。
- `models`：主键为 `{gateway_id}::{upstream_model_id}`。相同上游模型 ID 在不同 Gateway 中保持独立。`capabilities_json` 保存能力证据结果，`configuration_json` 保存完整模型调用参数。手动模型在 `metadata.everybuddySource` 中标记为 `manual`，Target 导入模型标记为 `targetImport`。
- `target_states`：记录两个目标的路径、最后读取摘要、最后发布摘要和 schema。
- `backups`：记录来源、SHA-256 Fingerprint 和创建时间，每个目标保留最近 10 份。
- `app_settings`：保存语言、主题、最近目标选择和自定义路径。
- `gateway_source_identities`：保存 Gateway 来源的 keyed digest，不保存 API Token。一个 Gateway 可以登记 Base URL 和模型级 `endpointOverride` 对应的多个来源。
- `deleted_gateway_sources`：保存已删除或轮换来源的 tombstone，避免启动导入从 Target 恢复旧 Gateway。
- `stale_gateway_models`：来源变化后临时隔离旧模型。刷新成功前，旧模型不参与 UI、Target 匹配、编辑和发布。

模型记录包含上游 ID、名称、Vendor、Capability、Model Configuration、Evidence 和清理后的原始 metadata。清理逻辑递归遍历 Object 和 Array，字段名转为小写并忽略 `_`、`-` 后，移除 `apiKey`、`token`、`accessToken`、`refreshToken`、`authorization`、`password`、`secret`、`clientSecret` 和 `credentials` 等 secret-like 字段。响应或导入 metadata 回显当前 Token 时直接拒绝，错误信息不包含原值。模型刷新会更新上游 metadata 和 Capability；未人工修改的发现模型重新解析 Model Configuration，手动添加、Target 导入或存在 `Manual` Evidence 的模型保留本地配置。`Imported`、`Probe`、`Manual` Evidence 在请求目标不变的刷新中继续保留；模型 `endpointOverride` 或 `useCustomProtocol` 变化时删除旧 `Probe` Evidence 并重新解析 Capability。OpenRouter 的 text-output eligibility 随匹配结果持久化，保证后续 Probe 和模型更新继续执行同一 non-text hard guard。名称和 Vendor 的人工覆盖按字段保存，只保护用户明确提供或修改的字段。来源变化时，刷新使用包含 stale 模型的快照恢复人工配置；成功替换后再解除 stale 状态。写入前同时比较 API Profile 与模型版本快照，避免刷新期间的本地编辑被旧响应覆盖。

## 6. Gateway 协议

### 6.1 Base URL 规范化

| 用户输入                            | 规范化结果                         |
| ----------------------------------- | ---------------------------------- |
| `https://api.example.com`           | `https://api.example.com/v1`       |
| `https://api.example.com/v1/`       | `https://api.example.com/v1`       |
| `https://api.example.com/v1/models` | `https://api.example.com/v1`       |
| `https://api.example.com/proxy`     | `https://api.example.com/proxy/v1` |

URL 只允许 `http` 或 `https`，不能包含 User Info、Query 或 Fragment。远程 Gateway 必须使用 HTTPS；HTTP 仅允许 `localhost`、`127.0.0.1`、`::1` 等 loopback 地址。标准模型的 `endpointOverride` 按 API root 规范化；Custom Protocol 保留完整请求路径。启动导入的 Target URL 根据 `useCustomProtocol` 使用相同分支。Gateway Client 不自动跟随 HTTP Redirect，3xx 响应按协议错误处理。

### 6.2 模型发现

```http
GET {apiRoot}/models
Authorization: Bearer {token}
```

响应必须包含 `data` 数组，每个模型至少包含非空且不重复的 `id`。可选 `name` 或 `display_name` 用作显示名称。模型发现与 Probe 的单次响应体上限均为 4 MiB；模型发现最多接受 10,000 条记录，超过限制时拒绝整次响应。发现操作不调用模型，不产生模型 Token 消耗。

Gateway 返回的结构化 metadata 代表当前 API 来源，明确字段优先于公共 OpenRouter 目录；OpenRouter 只补齐缺失字段。EveryBuddy 支持顶层和 `capabilities` 内的 Capability boolean、`supported_parameters`、`input_modalities`、`architecture.input_modalities`、`features`，以及 `reasoning.supportedEfforts` 等常见 OpenAI-compatible 扩展字段。Provider 依次从 `vendor`、`provider`、`owned_by`、`ownedBy`、`organization` 等 metadata 读取并规范化。

OpenRouter Directory 使用 lazy load：打开应用和启动配置恢复不发起请求，首次模型发现或手动添加模型时才请求 `GET https://openrouter.ai/api/v1/models?output_modalities=all`。成功响应同时写入内存和 Tauri `app_data_dir/openrouter-models-cache.json`，有效期为 6 小时；同一进程的并发调用通过 single-flight 串行合并。请求失败后 15 分钟内不重复尝试，并优先继续使用过期磁盘快照；没有任何快照时才回退 Gateway metadata 和保守默认值。因此连续刷新多个 Gateway 不会重复下载目录。

目录请求不携带用户 Token、Gateway Base URL、模型选择或其他 Gateway metadata。EveryBuddy 只对当前 Gateway 返回或用户手动输入的 Model ID 做本机匹配，不把全量模型导入模型库。目录请求超时为 5 秒，响应上限为 8 MiB 和 10,000 个模型。

模型解析将能力字段来源与实际调用 ID 分开。匹配顺序为：完整 Model ID、Provider namespace + leaf ID、全目录唯一 leaf ID；查询值没有精确记录时，再通过共享 `canonical_slug` 定位无变体基础记录。精确命中的 Batch/Free 变体或 Alias 保留自身明确字段，并按字段从 `alias_target.slug`、共享 `canonical_slug` 或同名基础记录补齐缺失值，不再整体替换成基础记录。禁止使用前缀模糊匹配，例如 `openai/gpt-5.6-sol` 和 `openai/gpt-5.6-sol-pro` 始终独立。发布到 Target 的 `id` 保持 Gateway 实际返回值。Gateway 未提供显示名称时，才使用匹配记录的 OpenRouter `name`。

只有已在目录中匹配的模型才能执行「从 OpenRouter 设置」。用户触发后，EveryBuddy 请求 `GET https://openrouter.ai/api/v1/model/{author}/{slug}`；Alias 和 `:free` 等 Variant suffix 保留在匹配后的路径中。模型详情请求超时为 5 秒，响应上限为 1 MiB，URL 包含匹配后的 OpenRouter Model ID，但不携带用户 Token。

成功响应会替换当前 Capability、Evidence 和自动模型配置，同时保留用户的 `endpointOverride` 与 `useCustomProtocol`。远程请求在 mutation lock 外执行，写入前重新比较模型快照；模型在请求期间被修改时拒绝旧响应，避免覆盖并发编辑。

### 6.3 主动 Probe

Probe 只能由用户确认后执行，一次最多发送 3 个最小请求：

1. 提供单个 Function Tool，检查响应是否返回 `tool_calls`。
2. 提供一个固定的四色条 Data URL 图片，仅在响应准确返回 `MAGENTA-GREEN-BLUE-YELLOW` 时确认 Vision。
3. 发送 `reasoning_effort: low`，仅在响应报告 Reasoning Token 或 Reasoning Content 时确认能力。

请求成功但没有可验证证据时，EveryBuddy 不把「参数未报错」当作能力确认。Vision challenge 的文本提示不包含期望答案。每次 Probe 先移除旧 Probe Evidence，再写入本次可验证结果；没有新结果时回退其他 Evidence 来源。Custom Protocol 不保证 Chat Completions 合同，因此不执行主动 Probe。

## 7. Capability Resolver

证据优先级固定为：`manual` > `probe` > `imported` > Gateway `metadata` > `openRouter` > `default`。Gateway 明确字段描述当前 API 来源，OpenRouter 只作为公共目录 fallback。明确的非 text-output 资格是硬约束，即使存在 `Manual`、`Probe` 或 `Imported` positive Evidence，也不会投影聊天能力或调用参数。未知模型在没有 OpenRouter 快照和 Gateway metadata 时，三项能力均默认为 `false`。Target 导入、Probe 和人工覆盖会写入独立 Evidence；Probe 仅在实际请求目标不变时保留。

Capability 表达模型是否具备某项能力，Model Configuration 表达 WorkBuddy 调用模型时使用的参数。两者分开持久化，避免模型刷新覆盖人工配置。

Gateway metadata 的明确自动配置优先于 OpenRouter 精确匹配，OpenRouter 只补缺失字段；已有 Target 导入配置、手动添加模型配置和人工覆盖仍按原值保留。字段映射如下：

| OpenRouter 字段                                          | EveryBuddy / `models.json` 字段 | 映射规则                                                                                         |
| -------------------------------------------------------- | ------------------------------- | ------------------------------------------------------------------------------------------------ |
| Model ID namespace                                       | `vendor`                        | 已知别名规范化为稳定标识；其他合法 OpenRouter namespace 原样保留；不修改 Gateway 实际 Model ID   |
| `architecture.input_modalities`、`output_modalities`     | `supportsImages`                | 输入包含 `image` 且输出包含 `text` 时为 `true`                                                   |
| `supported_parameters` 中的 `tools` 或 `tool_choice`     | `supportsToolCall`              | 仅对输出包含 `text` 的模型启用                                                                   |
| `reasoning` 对象或 Reasoning 相关 `supported_parameters` | `supportsReasoning`             | `reasoning`、`reasoning_effort` 或 `include_reasoning` 任一明确出现且输出包含 `text` 时为 `true` |
| `context_length`                                         | `maxInputTokens`                | 仅对 text-output 模型接受大于 0 的值；缺失时使用 `top_provider.context_length`                   |
| `top_provider.max_completion_tokens`                     | `maxOutputTokens`               | 仅对 text-output 模型接受大于 0 的值；参数支持标记本身不生成 Token 上限                          |
| 非空 `default_parameters.temperature`                    | `temperature`                   | 仅对 text-output 模型映射；Gateway metadata 缺失时才使用                                         |
| `reasoning.supported_efforts`                            | `reasoning.supportedEfforts`    | 保留目标支持的六个强度，去重并保持 OpenRouter 顺序；`none` 不作为强度写入                        |
| `reasoning.default_effort`                               | `reasoning.defaultEffort`       | 必须是支持的强度；`none` 映射为 `null`                                                           |
| `reasoning.mandatory`                                    | `reasoning.canDisableThinking`  | 取逻辑反值；缺失时可用 Effort 中明确的 `none` 判断可以关闭                                       |

Reasoning 强度不使用内置模型 Preset。只有 OpenRouter 的 `reasoning.supported_efforts`、Gateway 明确返回的 `reasoning.supportedEfforts`、Target 导入值或人工设置才能生成具体档位；仅出现 `reasoning_effort` 参数名称时不会推测档位。`reasoning.default_enabled` 只证明 Reasoning metadata 存在，但没有语义等价的 WorkBuddy 配置字段；不能将它映射成 `onlyReasoning`。同理，`pricing`、`benchmarks`、`knowledge_cutoff`、`description`、`supported_voices`、`links`、`default_parameters` 中除 Temperature 外的采样参数，以及没有目标字段的 `supported_parameters` 均不写入 `models.json`。`canonical_slug` 和 `alias_target` 只参与能力来源解析，不作为 Target 配置字段。

2026-08-27 对 OpenRouter 实时响应的覆盖审计包含 562 个模型和 20 个顶层字段，其中 417 个模型输出包含 `text`，145 个为非 text-output 模型。WorkBuddy/CodeBuddy Custom Model schema 没有 Audio、Video、File output、Embedding、Pricing、Benchmark、Voice 或生命周期字段，因此这些字段不能被等价投影；非 text-output 模型也不会写入 Token、Temperature 或 Reasoning 调用配置。57 个返回零值 Token 上限的记录按缺失处理。Provider 不依赖固定枚举，目录出现新的合法 namespace 时可直接作为 `vendor`。这里的“覆盖”指所有具有目标 schema 语义等价项的字段都完成映射，不代表复制 OpenRouter 原始对象。

这套映射由 OpenRouter 返回的结构化字段驱动，不枚举模型名称。后续新模型只要继续返回相同字段即可自动适配；新增 OpenRouter 字段只有在 WorkBuddy 或 CodeBuddy 出现语义等价字段后才扩展对应 Adapter。

Reasoning Probe 只验证 `low` 参数是否被接受以及响应中是否出现可验证的 Reasoning 输出，不枚举全部强度。枚举会产生额外请求和 Token 消耗，因此不能把单次 Probe 结果声明为完整 `supportedEfforts`。

| 参数来源                | `models.json` 字段                                                                                                               |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| 模型记录                | `id`、`name`、`vendor`                                                                                                           |
| API Profile             | `url`、`apiKey`                                                                                                                  |
| Capability              | `supportsToolCall`、`supportsImages`、`supportsReasoning`                                                                        |
| Model Configuration     | `maxInputTokens`、`maxOutputTokens`、`temperature`、`onlyReasoning`、`useCustomProtocol`                                         |
| Reasoning Configuration | `reasoning.effort`、`reasoning.defaultEffort`、`reasoning.supportedEfforts`、`reasoning.summary`、`reasoning.canDisableThinking` |

模型级 `endpointOverride` 可覆盖 API Profile 的 `url`。`useCustomProtocol: true` 时必须提供完整请求 URL；WorkBuddy 和 CodeBuddy 直接使用该地址，不自动追加 `/v1` 或 `/chat/completions`。

Reasoning Effort 支持 `minimal`、`low`、`medium`、`high`、`xhigh` 和 `max`。WorkBuddy 5.3.14 runtime 与共享的 CodeBuddy Target Adapter 仅保留 `auto | concise | detailed` Reasoning Summary。Rust 和 TypeScript 数据类型保留 `always | never`，仅用于读取历史数据库；编辑保存和发布前必须替换为受支持值。

## 8. Configuration Target

| Target    | 默认路径                                                                           |
| --------- | ---------------------------------------------------------------------------------- |
| WorkBuddy | macOS：`~/.workbuddy/models.json`；Windows：`%USERPROFILE%\.workbuddy\models.json` |
| CodeBuddy | macOS：`~/.codebuddy/models.json`；Windows：`%USERPROFILE%\.codebuddy\models.json` |

新文件写为模型数组：

```json
[
  {
    "id": "gpt-5.6",
    "name": "Main API · GPT-5.6",
    "vendor": "openai",
    "url": "https://api.example.com/v1",
    "apiKey": "<written-at-publish-time>",
    "maxInputTokens": 262144,
    "maxOutputTokens": 32768,
    "temperature": 0.7,
    "supportsToolCall": true,
    "supportsImages": true,
    "supportsReasoning": true,
    "onlyReasoning": false,
    "reasoning": {
      "effort": "low",
      "defaultEffort": "high",
      "supportedEfforts": ["low", "medium", "high", "xhigh", "max"],
      "summary": "auto",
      "canDisableThinking": true
    },
    "useCustomProtocol": false
  }
]
```

旧包装格式必须包含 `models` 数组。Codec 更新已管理字段时保留未知模型字段、未知 Reasoning 子字段、未知顶层字段和原有 Envelope。用户清空可选的已管理字段时，Codec 会从目标模型中移除对应旧值。单个 Target 配置上限为 8 MiB 和 10,000 个模型；同一文件出现重复 Model ID 时拒绝解析，避免目标产品采用不确定条目。

WorkBuddy 与 CodeBuddy 的默认路径不同，但共用 `model_config` 和 `ConfigDocument` 序列化逻辑。同一次双目标发布对相同输入生成一致的模型配置内容，各自的现有 envelope 和未知字段仍按目标文件独立保留。两个 Target 的显示名称均使用 `API 来源名称 · 模型名称`，Model ID 保持不变；模型名称已经带有同一来源前缀时不重复添加。

### 8.1 启动导入与匹配

`bootstrap` 把两个 Target 的当前 `models.json` 作为外部事实来源。每次启动都重新执行匹配；导入只在应用启动时执行，不发送 `/models` 或 `/chat/completions` 请求。

1. 按 WorkBuddy、CodeBuddy 的固定顺序读取数组或 wrapped schema。
2. 只接受 `useCustomProtocol: false`、符合远程 HTTPS 或本机 loopback HTTP 规则的 URL、非空 Model ID 和非空 `apiKey` 的条目。
3. 按规范化 `url + apiKey` 聚合 Gateway。相同 Model ID 只有在有效 URL、Token 和 `useCustomProtocol` 均匹配时才关联到本地模型。
4. 唯一同 URL Gateway 缺少凭据时，EveryBuddy 可以从 Target 修复凭据。存在多个候选或系统凭据库不可用时，EveryBuddy 跳过条目并报告原因。
5. API 来源不存在时，EveryBuddy 按手动添加的数据边界创建 Gateway，并导入该新 Gateway 在 Target 中的模型。API 来源已存在时，EveryBuddy 不补写或覆盖本地模型，只用 Model ID、有效 URL 和 Token 恢复匹配状态。
6. 两个 Target 的同一模型参数不一致时，首次导入保留 WorkBuddy 参数，并报告 CodeBuddy 差异。
7. 启动导入在 SQLite `BEGIN IMMEDIATE` transaction 中重新读取 Gateway 和模型快照。应用使用 single-instance plugin 限制为单实例运行，transaction 仍用于防止重复调用并发写入。
8. SQLite 批量写入失败时，EveryBuddy 删除本次新写入或修复的凭据；凭据清理失败会作为独立错误上报，不静默忽略。

只有新建 API 来源时才导入 Target 模型，其字段覆盖名称、Vendor、Capability、Reasoning 和 Model Configuration。导入 metadata 在写入 SQLite 前递归移除 secret-like 字段。

只读 `TargetModelState` 按 Target 返回 Fingerprint、`matchedModelKeys`、未匹配数量和跳过数量。「已配置」只表示模型当前准确存在于 WorkBuddy 或 CodeBuddy 的 `models.json`。启动、5 秒轮询、发布完成和备份恢复都会重新计算该状态；轮询、发布和恢复不再次执行自动导入。

## 9. 发布事务

1. Preview 读取目标配置，记录配置路径、symlink 解析后的实际写入路径、Fingerprint、API Profile revision、Keychain credential revision 和模型 revision，并计算新增、更新、不变和模型 ID 冲突。
2. 用户确认目标、明文 Token 提示和冲突替换。
3. Execute 重新读取 Gateway、Keychain Token、模型和 Target 设置，并比较 Preview 中的全部 revision、路径和 Fingerprint。
4. API、Token、模型或路径不一致时返回 `CONFLICT_ERROR`；文件内容不一致时返回 `DRIFT_ERROR`。两种错误都不写文件。
5. 对所有已存在的目标分别创建备份。
6. 每个目标写入前再次读取文件；内容与 Execute 阶段快照不一致时返回 `DRIFT_ERROR`，保留外部修改。
7. 在目标目录中写入临时文件并执行原子替换。
8. 重新读取文件，验证 schema 和选中模型 ID。
9. 任一目标失败时，按逆序恢复已经写入的目标；回滚只在文件仍等于本次发布输出时执行。文件已被外部修改时不覆盖，并报告回滚失败；本次新创建且未被修改的文件在回滚时移除。
10. 所有文件校验成功后，在单个 SQLite transaction 中保存 `gateway_source_identities` 和两个 `target_states`。状态 transaction 失败时，来源身份不会残留，并按相同条件恢复所有已写入的目标文件。
11. 每个 Target 返回独立的 Success、Failure、Rolled Back 和 Rollback Failed 状态。

跨两个文件系统操作不存在单一原子提交。EveryBuddy 使用 Preview Fingerprint、写前二次检查、目标内原子替换和条件式补偿回滚实现可恢复的一致性。外部进程仍可在最后一次检查与原子替换之间修改文件，因此发布前备份和逐目标结果始终保留。

写入前解析目标路径。有效 symlink 会解析为真实目标后执行同目录原子替换，symlink 本身保持不变；dangling symlink 会返回 `TARGET_ERROR`，不替换链接。WorkBuddy 和 CodeBuddy 的路径解析为同一个实际文件时，设置保存失败。

## 10. 安全模型

- Token 保存到 macOS Keychain 或 Windows Credential Manager，SQLite 只保存 `token_ref`。
- 安装级随机 source identity key 保存在系统凭据库中。SQLite 只保存基于该 key 计算的 HMAC digest，用于来源匹配和 tombstone；不能从数据库副本直接验证候选 Token。数据库已有来源记录但 source identity key 缺失时，操作失败，不自动生成新 key。
- Tauri identifier 固定为 `com.everybuddy.desktop`。Gateway 凭据使用 `com.everybuddy.desktop.gateway` service；读取不到凭据时会检查旧 service `com.everybuddy.app.gateway`，把命中的凭据迁移到新 service，并删除旧项。删除 Gateway 时同时清理 current 和 legacy service。
- 应用限制为单实例运行，并使用进程内 mutation lock 串行化 Gateway、凭据、Model、Settings、发布和恢复操作。模型发现、OpenRouter 查询和 Probe 在锁外执行网络请求，提交结果前重新加锁并比较 Gateway、Credential 和 Model 快照，拒绝旧响应覆盖新状态。保存 Gateway 时先写凭据再写 SQLite。SQLite 失败时，已有 Gateway 恢复旧 Token，新 Gateway 删除新 Token。删除 Gateway 时 SQLite 失败会恢复已删除的 Token；系统凭据库不可用时，操作在修改 SQLite 前终止。
- 编辑 API Profile 时按需从系统凭据库读取 Token，仅保留在当前 Dialog 的内存状态中；默认隐藏，关闭 Dialog 后清除。
- Token 不进入日志、错误对象、metadata、诊断输出或前端持久化状态。前端 Error、Promise rejection、Updater 和操作错误经统一结构化脱敏后，只按 `warn/error` 写入滚动日志。
- OpenRouter 目录请求不携带用户 Token、Gateway Base URL、模型选择或 Gateway metadata；仅在本机以 Model ID 查询已下载的公开目录快照。模型详情请求的 URL 包含匹配后的 OpenRouter Model ID，但同样不携带用户 Token。
- 启动导入期间，Token 只在 Rust 内存中用于 Gateway 匹配和凭据写入。`BootstrapData`、`TargetModelState` 和 `TargetImportReport` 不包含 Token。
- 只有系统凭据库明确报告凭据缺失时，EveryBuddy 才从 Target 修复 Token。凭据库不可用时停止该条目导入。
- WorkBuddy 和 CodeBuddy 要求 Token 出现在 `models.json`，发布前必须展示该限制。
- Unix 配置和备份权限设置为 `0600`；Windows 写入受保护 DACL，仅授予当前用户访问权限。
- 配置写入使用同目录临时文件和原子替换。
- CSP 仅允许应用自身资源、Tauri IPC 和本地 Asset Protocol。
- 删除 Gateway 不会删除已发布到目标产品的模型配置。删除和 Token 轮换会保存 Base URL 及已发布 `endpointOverride` 的来源 tombstone，避免旧配置在下次启动时恢复为新 Gateway。

## 11. IPC 接口

| Command                               | 作用                                                                                       |
| ------------------------------------- | ------------------------------------------------------------------------------------------ |
| `bootstrap`                           | 执行一次启动导入，返回 Gateway、模型、目标、模型匹配状态、导入报告和设置                   |
| `save_gateway` / `delete_gateway`     | 管理 Gateway Profile 和系统凭据                                                            |
| `discover_models`                     | 调用 `/v1/models`，更新发现快照并保留未被上游返回的手动模型                                |
| `add_manual_model`                    | 在指定 Gateway 下创建模型，复用 OpenRouter 缓存解析初始 Capability；未匹配时使用保守默认值 |
| `get_openrouter_model_match`          | 查询当前模型是否存在于 OpenRouter 目录，并返回匹配后的 Model ID                            |
| `apply_openrouter_model`              | 读取 OpenRouter 模型详情，替换 Capability、Evidence 和自动配置                             |
| `probe_model`                         | 执行 3 个用户确认的能力请求                                                                |
| `update_model`                        | 保存模型名称、Vendor、人工能力覆盖和完整 Model Configuration                               |
| `get_target_snapshot`                 | 从同一批文件快照读取 schema、权限、Fingerprint、Drift 和模型匹配状态，不导入凭据或模型     |
| `prepare_publish` / `execute_publish` | 执行两阶段发布                                                                             |
| `list_backups` / `restore_backup`     | 查询和恢复备份                                                                             |
| `save_settings`                       | 保存语言、主题、目标和路径                                                                 |

错误使用 `{ code, message }` 返回，禁止携带请求 Header、Token 或完整响应 Body。

## 12. 测试与发布

```bash
pnpm format:check
pnpm lint
pnpm typecheck
pnpm ipc:check
pnpm test
pnpm test:coverage
pnpm build
pnpm release:check
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm tauri build
```

Fake Gateway 测试实际验证 HTTP Path、Bearer Header 和模型响应解析。Frontend 检查在 Linux 上执行一次；Rust 和 Tauri Bundle 在 macOS、Windows 上分别验证。稳定的 `CI Gate` 聚合所有适用 Job，作为 Branch Ruleset 的 Required Status Check。

模型库测试覆盖多个 Gateway 保存相同上游 Model ID、手动和导入模型在 Refresh 后保留、来源变化后的 stale 隔离与恢复、刷新期间本地编辑的并发冲突，以及上游后来返回同一 ID 时不生成重复记录。Capability 测试覆盖 Gateway metadata 对 OpenRouter 的字段优先级、基础 ID、Delivery Variant、Alias 与 Canonical slug 的字段级 fallback、Pro 型号隔离、动态 Provider namespace、Reasoning Effort alias、`none` 转换、`mandatory`、默认 Effort、Temperature、Token 上限和非 text-output 硬约束；OpenRouter Client 测试覆盖 catalog gate、模型详情接口、Variant URL、进程内复用和跨启动磁盘缓存，并验证应用详情时保留本地路由配置。Gateway 测试覆盖远程 HTTP 拒绝、本机 HTTP、4 MiB 响应上限、10,000 模型上限、重复 Model ID、短 Token 回显隔离和 Vision challenge 响应校验。Target Import 测试覆盖数组和 wrapped schema、重复启动幂等、序列化导入、WorkBuddy 冲突优先级、Custom Protocol 精确匹配、同 URL 不同 Token、缺失或歧义凭据、来源轮换和删除 tombstone、损坏 JSON、非法参数、凭据清理失败和 Token 隔离。

发布测试覆盖 WorkBuddy 单目标、CodeBuddy 单目标、双目标成功、第二目标失败补偿、首次创建文件的失败清理、API 与 Keychain revision、路径和 symlink 变化、写前 Drift、外部修改后的条件回滚、备份恢复、恢复状态保存失败回滚、每个目标保留 10 份备份，以及 SQLite 状态 transaction 失败后的文件回滚。Target 测试覆盖 8 MiB/10,000 条限制和重复 Model ID。Gateway Service 测试覆盖保存、删除和补偿失败，错误文本不得包含 Token。

CI 固定使用 pnpm `11.22.0`、Node.js 22 和 Rust `1.91.1`。Release workflow 只接受属于 `main` 的版本 Tag，并在 `release` Environment 审批后构建 macOS Universal 和 Windows x64 安装包。当前稳定版本为 `0.1.1`；workflow 创建 Draft（非 Prerelease），并验证 Tauri Updater Artifact、`latest.json`、`.sig`、安装包和 `SHA256SUMS.txt`。安装包暂未使用 Apple notarization 或 Windows Authenticode，Release 和 README 必须明确显示未验证开发者警告。

Updater private key 使用 GitHub Secrets：

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

Updater public key 不是 secret，保存为 GitHub Actions Variable `TAURI_UPDATER_PUBLIC_KEY`。Release job 在构建前检查所有 Updater 签名输入，缺少任一项都会停止。Updater 签名用于防止更新资产被篡改，不能替代操作系统平台代码签名。GitHub `releases/latest` 指向最新稳定版本，应用通过该通道检查并安装签名有效的更新。获得 Apple Developer ID 和 Windows Code Signing Certificate 后，再启用 notarization、Authenticode 和对应验证步骤。

## 13. 兼容性假设

当前设计假设 WorkBuddy 和 CodeBuddy 接受兼容的 `models.json` 字段。如果任一产品改变路径、Envelope、字段或 reload 行为，只修改对应 Adapter 和版本化 Codec，不修改 Gateway、Capability Resolver、Model Library 或主 UI 流程。
