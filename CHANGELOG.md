# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 的结构。

## [Unreleased]

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

[Unreleased]: https://github.com/myxiaoao/everybuddy/compare/v0.1.0-alpha.1...HEAD
[0.1.0-alpha.1]: https://github.com/myxiaoao/everybuddy/releases/tag/v0.1.0-alpha.1
