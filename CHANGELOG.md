# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 的结构。

## [Unreleased]

## [0.1.2] - 2026-08-31

### Added

- 编辑 API 来源时可读取已有 Token；输入框默认隐藏内容，并保留显示与隐藏切换。

### Changed

- 将 API 来源 Token 以明文保存到 EveryBuddy 本地 SQLite 数据库，并限制数据库、WAL、SHM、备份及目标文件的本机访问权限。
- API 来源导入时通过同一事务保存来源、Token、模型和来源信息，避免部分写入。
- 整合目标检查、模型生命周期和目标文件 Codec，强化预览、写入、Drift 检测与回滚流程。

### Fixed

- 在并发刷新、发布会话和条件写入期间拒绝过期结果，避免外部改动被旧状态覆盖。
- 强化双目标发布的补偿回滚和首次创建文件时的失败清理。

## [0.1.1] - 2026-08-28

### Added

- 支持从 OpenRouter 模型详情直接应用 Capability 和自动模型配置。

### Changed

- WorkBuddy 和 CodeBuddy 的发布名称增加 API 来源前缀，Model ID 保持不变。
- 每次启动时根据当前目标文件重新计算模型的「已配置」状态。
- 同步更新 README、设计、安全、故障排查说明和界面截图。

## [0.1.0] - 2026-08-27

### Changed

- 优先采用 Gateway metadata，并通过 OpenRouter 精确匹配补齐缺失能力，避免把非文本模型投影为聊天模型。
- 完善 Custom Protocol、Reasoning 和旧配置的校验与提示，使 WorkBuddy、CodeBuddy 的模型配置保持一致。
- 补充核心流程图、发布预览和最新应用截图。

### Fixed

- 再次发布时移除当前 API 来源中未选中的托管模型，同时保留无法确认归属的本地或外部配置。
- 修复弹窗错误反馈、表单草稿保留、响应式布局和中英文文案问题。
- 对齐工作区边距与发布目标图标，并修正 macOS Dock 图标尺寸。

## [0.1.0-alpha.1] - 2026-08-25

### Added

- 管理多个 OpenAI-compatible API 来源和本机系统凭据。
- 发现或手动添加模型，解析 Capability Evidence 和完整模型配置。
- 启动时导入 WorkBuddy、CodeBuddy 配置并恢复三态模型选择。
- 单目标与双目标发布、差异预览、Drift 检测、备份恢复和补偿回滚。
- 简体中文、English，以及 Light、Dark、System 主题。
- macOS Universal、Windows x64、Tauri Updater 签名和 Unsigned Alpha 发布工作流。

### Fixed

- 在 Gateway 刷新、模型 Probe 和配置发布期间检测并发修改，避免旧请求覆盖新状态。
- 强化 Target 路径、Credential 来源、发布状态和备份回滚校验，避免部分失败留下不一致状态。

[Unreleased]: https://github.com/myxiaoao/everybuddy/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/myxiaoao/everybuddy/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/myxiaoao/everybuddy/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/myxiaoao/everybuddy/compare/v0.1.0-alpha.1...v0.1.0
[0.1.0-alpha.1]: https://github.com/myxiaoao/everybuddy/releases/tag/v0.1.0-alpha.1
