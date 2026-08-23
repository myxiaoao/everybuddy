# Troubleshooting

## API 无法连接

1. 确认 API Base URL 使用 HTTPS。HTTP 只允许 loopback 地址。
2. 确认接口支持 `GET /v1/models` 和 Bearer Token。
3. 编辑 API，重新保存 Token，再执行「刷新模型」。
4. 如果错误为响应格式不兼容，检查上游是否返回 OpenAI-compatible `data` 数组。

## Token 无法读取

EveryBuddy 把 Token 保存到 macOS Keychain 或 Windows Credential Manager。系统凭据缺失时，编辑对应 API 并重新保存 Token。不要把 Token 写入 Issue、截图或日志附件。

## Target 不可发布

在「设置」中检查 WorkBuddy 和 CodeBuddy 的配置路径。确认当前用户能够访问父目录和 `models.json`。损坏的 JSON、dangling symlink、无写权限或超过大小限制的文件会停止发布。

## 配置已变化

EveryBuddy 在预览和写入之间检测到外部修改时会停止发布。重新加载 Target 状态，检查差异后再次预览。不要在 WorkBuddy、CodeBuddy 和 EveryBuddy 中同时编辑同一个 `models.json`。

## 界面无法继续显示

选择「重新加载界面」。EveryBuddy 会把脱敏后的 `warn/error` 写入滚动日志：

- macOS：`~/Library/Logs/com.everybuddy.desktop/everybuddy.log`
- Windows：`%LOCALAPPDATA%\com.everybuddy.desktop\logs\everybuddy.log`

每个日志文件最大 2 MiB，最多保留 3 个归档。提交日志前仍需人工检查并移除 Token、请求 Header、完整 `models.json` 和其他隐私信息。

## Updater 无法检查更新

Alpha Prerelease 使用手动下载更新，因为 GitHub `releases/latest` 不会选择 Prerelease。诊断日志中出现 `updater.check` 记录不影响 Alpha 安装包使用。稳定更新通道启用后，再检查 GitHub Releases 连通性和有效 Updater public key；本地开发构建不生成 Updater artifact。

## 系统阻止打开 Alpha 安装包

当前 Alpha 安装包没有 Apple Developer ID 或 Windows Authenticode 平台签名。只从项目 GitHub Releases 下载并先核对 `SHA256SUMS.txt`。

- macOS：在 Finder 中对应用选择「打开」，或在「系统设置 → 隐私与安全性」中确认打开。
- Windows：在 SmartScreen 中选择「更多信息 → 仍要运行」。

如果下载来源或 SHA-256 不一致，不要绕过系统警告。
