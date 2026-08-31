# EveryBuddy UI 设计规范

## 1. 设计目标

EveryBuddy 面向需要频繁切换第三方模型来源的个人开发者。界面支持快速扫描、明确比较和可恢复发布，不采用营销页面或通用 Dashboard 的表达方式。

视觉主张是「高密度 Neutral Operations Console」：以 Command Bar 统合任务上下文，用稳定的三栏结构承载高频操作，通过清晰边界、背景层级和 shadcn/ui 默认 Neutral 配色建立秩序。

参考产品只提取交互机制：

- Raycast：快速定位和键盘效率。
- TablePlus：连接列表与工作区分区。
- GitHub Desktop：目标状态、变更摘要和提交前确认。

EveryBuddy 不复制这些产品的视觉样式。

## 2. 信息架构

宽窗口使用三栏工作区：

```text
+----------------------------------------------------------------------------------+
| API: Sub2API  >  Models: 4 discovered  >  Targets: 2 selected  [Preview & publish]|
+------------------+------------------------------+------------------------+
| API sources      | Models                       | Capabilities & targets |
|                  |                              |                        |
| + Add API        | Search                       | Evidence               |
|                  | [All] [Tool] [Vision] [Rsn]  |                        |
| Gateway A        | [x] Model A   T  V  R        | Tool Call       [on]  |
| Gateway B        | [ ] Model B   T  -  -        | Vision          [on]  |
|                  | [x] Model C   -  -  R        | Reasoning       [on]  |
|                  |                              |                        |
| Backups          |                              | WorkBuddy       [x]   |
| Settings         |                              | CodeBuddy       [x]   |
|                  |                              | 2 models -> 2 targets  |
+------------------+------------------------------+------------------------+
```

- 左栏固定约 `248px`，用于 Gateway、备份和设置。
- 中栏使用剩余空间，最小内容宽度约 `380px`。
- 右栏固定约 `348px`，展示当前模型能力和发布目标。
- Command Bar 始终显示当前 API、发现模型数、已选模型数、已选目标数和唯一 Primary Action。
- 页面不使用 Card 套 Card。三栏是连续工具面板，局部选项使用轻量 Surface。

## 3. 响应式行为

窗口宽度大于 `980px` 时显示三栏。不超过 `980px` 时切换为 Master-detail：

1. Gateway 列表。
2. 当前 Gateway 的模型列表。
3. 当前模型详情和发布目标。

Command Bar 在窄窗口隐藏完整阶段导航，改为返回按钮、当前阶段标题、Refresh 和短标签 Primary Action。模型页返回 Gateway，详情页返回模型。320px 宽度下不允许出现页面级横向滚动，配置路径允许自然换行，不能依赖省略号隐藏关键字符。

## 4. 主流程

顶部 Command Bar 固定表达：

```text
API: Sub2API  >  模型: 已发现 4 个  >  目标: 已选 2 个  [预览并发布]
```

- 三个阶段是可操作的 Workspace Navigation，在窄窗口切换对应 Master-detail 页面。
- 添加 Gateway 后自动进入模型工作区；点击模型主体进入能力与目标工作区。
- Refresh 是 Secondary Action，发布是全局唯一 Primary Action。右栏不重复放置强 Primary Button。
- 选择状态使用 `180ms` 的 `transform` 和 `opacity` 反馈，并同步更新模型区、Command Bar 和右栏发布范围。
- `prefers-reduced-motion: reduce` 下不播放 Pulse Animation。

## 5. 核心交互

### 添加 API

Dialog 包含名称、API Base URL、Token Key。主要操作是「保存并发现模型」。Token 输入使用 Password Field；编辑 API 时按需加载现有 Token，默认隐藏，并保留显示/隐藏切换。关闭 Dialog 后清除前端 Token state。

保存成功后自动选择 Gateway 并执行模型发现。发现本身不显示 Token 消耗警告，因为 `/v1/models` 不调用模型。

左栏允许同时保存多个 API Profile。标题栏使用带 Plus Icon 的紧凑「添加」Button，并通过 Tooltip 补充完整的「添加 API」含义，使创建入口明确但不挤占列表空间。切换 Profile 时只切换当前模型上下文，其他 API 的模型、Token 和 Capability Evidence 保持独立。

### 模型列表

- 支持按名称、模型 ID 和 Vendor 搜索。
- 「添加模型」打开手动模型 Dialog，Model ID 必填，显示名称和 Vendor 可选。保存时复用 OpenRouter 本地缓存，缓存缺失或过期时读取公共目录；不调用模型，不产生 Token 费用。
- 手动模型保存后进入当前模型详情，用户可继续确认 Capability、执行 Probe 或直接人工覆盖。
- 手动模型显示「手动」来源标记；刷新当前 API 时仍保留未被 `/v1/models` 返回的手动模型。
- 来源标记只表示模型来自「手动」添加，不再把 Target 匹配状态显示为「已导入」。
- 「已配置」只表示模型当前准确存在于 WorkBuddy 或 CodeBuddy 的 `models.json`。每次启动、5 秒轮询、发布完成和备份恢复后都重新计算，不沿用上次会话的结果。
- 使用紧凑 Segmented Filter 按 All、Tool Call、Vision 和 Reasoning 筛选当前模型列表。
- Checkbox 用于批量发布选择。
- 启动后，Checkbox 根据当前已选且可发布的 Target 恢复为 Checked、Indeterminate 或 Unchecked，不再统一初始化为空。
- Checked 表示模型存在于全部当前 Target；Indeterminate 表示只存在于部分 Target；Unchecked 表示不存在于任何当前 Target。
- Indeterminate 的 Tooltip 列出已存在的 Target。点击后变为 Checked，表示本次发布到全部当前 Target。
- Indeterminate 不计入已选数量，也不进入发布请求。取消勾选只排除本次发布，不删除任何 `models.json` 条目。
- 本次会话的人工选择使用独立 Override。切换 Target、Gateway 或检测到外部文件变化时重新计算基础状态，但不覆盖人工 Override。
- 固定高度的 Selection Slot 显示已选数量和清除入口，避免批选状态出现时引发布局跳动。
- 点击模型主体只改变右侧详情，不隐式切换 Checkbox。
- Tool Call、Vision、Reasoning 使用 Lucide 图标；可用状态同时通过颜色、透明度和 Tooltip 表达。

### Capability 与模型配置

- 每项使用 shadcn Switch。
- 辅助文字显示当前最高优先级 Evidence。
- 用户修改后才显示「保存模型配置」。
- Capability 标题行并排放置两个紧凑 Secondary Action：「从 OpenRouter 设置」和「主动 Probe」，避免操作按钮占满右栏宽度。
- 「从 OpenRouter 设置」只在当前模型存在于 OpenRouter 目录时可用；未匹配时禁用并通过 Tooltip 说明原因。目录检查或详情应用期间显示 Loading Indicator，避免重复操作。
- 「主动 Probe」打开确认 Dialog，明确说明会发送 3 个请求并可能产生少量 Token 费用；Custom Protocol 模式下禁用，并通过 Tooltip 说明协议不兼容。
- 应用 OpenRouter 模型详情会替换 Capability、Evidence 和自动配置，但保留 `endpointOverride` 与 `useCustomProtocol`。
- 「高级模型配置」使用原生 Details 渐进披露，不新增 Modal。默认收起，保持右栏可扫描。
- 「模型标识」允许修改显示名称和 Vendor；Model ID 保持为上游请求 ID，不允许在此修改。
- 「调用参数」覆盖请求地址、最大输入 Token、最大输出 Token、Temperature 和自定义协议。
- 「Reasoning 配置」覆盖 `onlyReasoning`、`canDisableThinking`、默认 Effort、兼容 Effort、支持的 Effort 和 Summary。
- `url` 和 `apiKey` 默认来自当前 API Profile；标准模型的请求地址覆盖用于发布和主动 Probe，不改变模型发现地址。Custom Protocol 必须填写完整请求 URL，且不执行主动 Probe。请求地址或协议模式变化后清除旧 Probe Evidence。
- `supportedEfforts` 使用 Checkbox Group，包含 `minimal`、`low`、`medium`、`high`、`xhigh` 和 `max`。默认 Effort 和兼容 Effort 只能从已选档位中选择。
- Gateway metadata 的明确 Capability 字段优先，OpenRouter 精确匹配只补缺失字段；两者都没有时使用保守默认值。辅助文字展示 OpenRouter、API metadata、Probe、Target 导入或人工覆盖等实际 Evidence 来源。
- OpenRouter 的 Batch/Free 变体、Alias 和 Canonical slug 可以关联到基础记录，但精确记录保留自身字段，只从基础记录补缺失值。界面与发布预览始终显示 Gateway 实际 Model ID；Pro 等独立型号不得通过前缀模糊匹配到基础型号。
- OpenRouter 明确返回 `reasoning.supported_efforts` 时自动勾选对应强度；`none` 转换为「允许关闭思考」，不显示为强度选项。只有 `reasoning_effort` 参数名称而没有明确范围时不推测档位。界面分别提示「已自动匹配」「未发现可靠范围」和「当前选择已覆盖自动匹配结果」；未知模型不默认勾选任何档位。
- Summary 只提供 `auto`、`concise`、`detailed`。历史数据中的 `always`、`never` 可读取，但必须选择受支持值后才能保存或发布。
- 手动添加与自动发现复用同一 OpenRouter 匹配逻辑。API 刷新只更新未人工修改的自动配置，不覆盖手动添加、Target 导入或带人工 Evidence 的配置。
- 关闭 Reasoning 时清除不再有效的 Reasoning 参数；开启「仅 Reasoning 模式」时同步关闭「允许关闭 Reasoning」。

### 发布目标

WorkBuddy 和 CodeBuddy 始终分别展示：`可发布`、`未检测到`、`配置已变化` 或 `配置无效`。状态不能只依赖颜色，必须同时显示文字和 Icon。

每个 Target 同时展示实际配置路径。窄窗口允许路径换行，保证用户在发布前可以检查完整目标位置。

首次加载只预选已检测、可写且 schema 有效的目标；后续记忆用户选择。右栏底部只显示「模型数 → 目标数」发布范围和差异预览提示，Primary Action 固定在 Command Bar。

发布成功后重新读取两个 Target 的模型匹配状态，并清除已发布模型的临时 Override。恢复备份后重新读取受影响 Target，并清除该 Target 涉及模型的临时 Override。

### 启动导入提示

- Target 中的 API 来源不存在时，按手动添加 API 的结果创建来源并导入其模型。API 来源已存在时不改动模型库，只恢复精确匹配的选择状态。
- 成功导入 Gateway 或模型时，显示可关闭的非阻塞提示，列出导入数量。
- 存在跳过、歧义或双 Target 参数冲突时，提示显示结构化 Issue 数量。
- 「查看详情」按 Target 和 Model ID 展开本地化原因。界面不显示 Rust 原始错误，不显示 Token。
- 导入提示不阻断 Gateway、模型查看或发布操作。用户可以关闭提示，关闭只影响当前界面状态。

### 发布预览

Preview Dialog 分目标显示路径、新增、更新和不变数量。存在模型 ID 冲突时，主按钮保持不可用，直到用户勾选明确替换确认。

发布完成后显示逐目标结果。部分失败必须同时展示失败目标和已回滚目标，不能只显示全局「失败」。

## 6. shadcn/ui 组件边界

基础控件使用本地维护的 shadcn/ui 源码：

- `Button`
- `Dialog`
- `Input`
- `Checkbox`
- `Switch`
- `Tooltip`

高级配置使用语义化 `details`、`fieldset` 和原生 `select`，视觉 token 与 shadcn/ui 控件保持一致。

业务组件保留在 `src/components`，不把 Gateway、模型或 Target 语义写入 `components/ui`。所有图标统一使用 Lucide。

## 7. Design Tokens

### 圆角

| Token         | 值     | 用途                    |
| ------------- | ------ | ----------------------- |
| `--radius-xs` | `3px`  | 状态和能力图标          |
| `--radius-sm` | `6px`  | Button、Input、列表选择 |
| `--radius-md` | `8px`  | Dialog 和主要 Surface   |
| `--radius-lg` | `12px` | 独立大 Surface          |

### 颜色角色

- `background` / `foreground`：应用底色和主要文字。
- `card`：模型主工作区和 Dialog。
- `muted` / `muted-foreground`：Gateway 导航区、次级 Surface 和辅助文字。
- `primary` / `primary-foreground`：当前视图唯一的主要操作。
- `accent` / `accent-foreground`：Hover、选中和次级强调。
- `destructive`：Drift、冲突、请求失败和移除操作。

颜色使用 shadcn/ui 默认 Neutral OKLCH token。组件只引用 EveryBuddy 的 semantic mapping，不直接使用原始色值；状态同时使用 Icon 和文字，不能只依赖颜色。

### Typography

字体使用平台 UI Font Stack，不额外加载 Web Font：

```css
system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI Variable Text",
"Segoe UI", "PingFang SC", "Microsoft YaHei UI", sans-serif
```

界面使用五级语义字号：

| Token            | 字号   | 行高   | 用途                          |
| ---------------- | ------ | ------ | ----------------------------- |
| `--text-caption` | `12px` | `1.4`  | 状态、Evidence、计数          |
| `--text-label`   | `13px` | `1.4`  | Button、表头、Section Label   |
| `--text-body`    | `14px` | `1.55` | 模型名称、Input、说明正文     |
| `--text-heading` | `16px` | `1.25` | Panel Heading、Dialog Heading |
| `--text-title`   | `18px` | `1.25` | 当前主视图标题                |

代码、API URL、模型 ID 和配置路径使用 `SFMono-Regular`、`Cascadia Code`、`Consolas` 回退，并关闭编程连字。动态计数使用 Tabular Numbers。界面不按 viewport 缩放字体，中文说明文字使用 `1.55` 行高。

## 8. 状态与文案

需要单独处理：没有 Gateway、远程 HTTP 地址、认证失败、网络超时、响应格式错误、响应或 Target 配置超过安全上限、重复 Model ID、模型列表为空、不同 Capability Evidence、OpenRouter 查询中、OpenRouter 未匹配、OpenRouter 应用成功或失败、启动导入结果、Target 未检测或 Drift、发布失败、成功回滚、回滚失败、没有备份。

错误文案说明「发生了什么」和「如何恢复」，不能只写「操作失败」。常规更新使用 `role="status"`，错误使用 `role="alert"`。

## 9. Accessibility

- 所有操作使用原生 Button 或 Radix Primitive，不使用 `div onClick`。
- Icon-only Button 必须有 `aria-label` 和 Tooltip。
- Keyboard Focus 使用至少 `2px` 可见 Focus Ring。
- Dialog 由 Radix 管理 Focus Trap、Escape 和触发点恢复。
- Checkbox、Switch、Input 均有可见 Label。
- 三态 Checkbox 使用 Radix 原生 `checked="indeterminate"` 和 `aria-checked="mixed"`。部分匹配状态同时提供 accessible name 和 Tooltip，不能只依赖横线图标。
- 点击目标最小为 40×40px；紧凑 Icon Button 不低于 32×32px，且相邻区域不重叠。
- 状态同时使用文字、Icon 和颜色。
- 200% Zoom 和 320px 宽度下保留完整主操作。
- Forced Colors 下 Focus、选中轨道和状态线使用系统 Highlight。

## 10. 视觉验收

- `1280×780` 简体中文 Light。
- `1280×780` English Dark。
- `980×720` 切换点。
- `375×812` 和 `320×720` Master-detail。
- 长 API URL、模型 ID 和路径不覆盖相邻控件。
- Dialog 在窄窗口内可滚动，Footer 操作始终可达。
- Reduced Motion、Keyboard-only 和 Focus 顺序可用。
- WorkBuddy 与 CodeBuddy 的状态、路径和结果始终独立呈现。
