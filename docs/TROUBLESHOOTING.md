# Troubleshooting

## API 无法连接

1. 确认 API Base URL 使用 HTTPS。HTTP 只允许 loopback 地址。
2. 确认接口支持 `GET /v1/models` 和 Bearer Token。
3. 编辑 API，重新保存 Token，再执行「刷新模型」。
4. 如果错误为响应格式不兼容，检查上游是否返回 OpenAI-compatible `data` 数组。

## Token 无法读取

EveryBuddy 把 Token 保存在本地 SQLite 数据库中。数据库缺少 Token 时，应用会在启动导入期间尝试从 WorkBuddy 或 CodeBuddy 的 `models.json` 恢复；如果目标配置中也没有对应来源，编辑该 API 并重新保存 Token。不要把 Token、EveryBuddy 数据库或完整 `models.json` 写入 Issue、截图或日志附件。

## 无法从 OpenRouter 设置

1. 检查当前 Model ID 和 Vendor 是否与 OpenRouter 中的记录一致。
2. 刷新当前 API 的模型列表，让 EveryBuddy 重新匹配 OpenRouter 公共模型目录。
3. 确认可以访问 `https://openrouter.ai`。只有存在于 `GET /api/v1/models?output_modalities=all` 返回结果中的模型才能使用「从 OpenRouter 设置」。

目录匹配成功后，EveryBuddy 还会请求对应模型的详情接口。该请求失败时不会修改当前模型配置，可以在网络恢复后重试。

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

确认可以访问 GitHub Releases，并检查应用是否使用有效的 Updater public key。当前稳定版本通过 GitHub `releases/latest` 获取更新清单；本地开发构建不生成 Updater artifact。

## 系统阻止打开安装包

当前安装包没有 Apple Developer ID 或 Windows Authenticode 平台签名。只从项目 GitHub Releases 下载并先核对 `SHA256SUMS.txt`。

- macOS：把 `EveryBuddy.app` 移动到「应用程序」目录后，先在 Finder 中对应用选择「打开」，或在「系统设置 → 隐私与安全性」中确认打开。如果 Gatekeeper 仍然阻止启动，并且已经完成 SHA-256 校验，执行：

  ```bash
  sudo xattr -cr "/Applications/EveryBuddy.app"
  open "/Applications/EveryBuddy.app"
  ```

  `xattr -cr` 会递归清除应用包的扩展属性。不要把命令目标改成 `/Applications` 或其他目录。

- Windows：在 SmartScreen 中选择「更多信息 → 仍要运行」。

如果下载来源或 SHA-256 不一致，不要绕过系统警告。
