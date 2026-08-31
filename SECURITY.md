# 安全策略

## 支持版本

EveryBuddy 只为最新稳定版本提供安全修复。旧版本发现安全问题后，应先升级到最新稳定版本再复现。

## 报告漏洞

不要通过公开 Issue、Discussion 或日志附件报告漏洞，也不要提交真实 API Token 或包含 Token 的 `models.json`。

使用 GitHub 仓库的 [Security Advisories](https://github.com/myxiaoao/everybuddy/security/advisories/new) 私下报告问题。报告中请提供：

- 受影响版本和操作系统。
- 可复现的最小步骤。
- 预期结果和实际结果。
- 已脱敏的错误信息或截图。
- 已知影响范围和建议修复方式。

项目不承诺固定响应时限。维护者会在确认问题后，通过 Security Advisory 协调修复和披露。

## 本地 Threat Model

EveryBuddy 是本地桌面应用，不提供远程账户、云同步或团队权限。主要信任边界包括：

- **Gateway transport and response**：远程 Gateway 和模型级 Endpoint Override 必须使用 HTTPS，HTTP 仅允许 loopback 地址，HTTP Redirect 不会被自动跟随。`/v1/models` 和 `/chat/completions` 返回不受信任的数据；EveryBuddy 限制响应大小、校验响应结构，在持久化前递归移除 secret-like metadata，并拒绝回显当前 Token 的模型响应。
- **公开模型目录**：首次模型发现或手动添加模型时可能请求 `GET https://openrouter.ai/api/v1/models?output_modalities=all`，并在本机按 Model ID 匹配能力。目录请求不携带用户 Token、Gateway Base URL、模型选择或 Gateway metadata；成功响应缓存在应用数据目录 6 小时，失败后 15 分钟内不重复请求。
- **OpenRouter 模型详情**：用户主动选择「从 OpenRouter 设置」且模型已在公开目录中匹配时，EveryBuddy 请求 `GET https://openrouter.ai/api/v1/model/{author}/{slug}`。请求 URL 包含匹配后的 OpenRouter Model ID，但不携带用户 Token、Gateway Base URL 或 Gateway metadata。
- **本地数据库**：API Token 以明文保存在 EveryBuddy 的 SQLite 数据库中。用户打开「编辑 API」时，专用 Tauri IPC 会按 Gateway ID 把当前 Token 返回到该 Dialog，默认以 Password Field 隐藏，关闭 Dialog 后清除前端状态。Token 不进入 bootstrap 数据。能够读取当前用户应用数据目录的本机进程，也能够读取数据库中的 Token。
- **Target config**：WorkBuddy 和 CodeBuddy 的 `models.json` 是外部可修改文件。EveryBuddy 使用 Fingerprint 检测 Drift，并在每次写入前重新校验；条件回滚不会覆盖发布后发生的外部修改。
- **本机文件系统**：EveryBuddy 数据库、SQLite WAL/SHM、migration backup、目标文件和目标备份在 Unix 下使用 `0600` 权限，在 Windows 下使用仅允许当前用户访问的受保护 DACL。有效 symlink 会保留并写入真实目标，dangling symlink 会被拒绝。
- **Updater pipeline**：安装包暂未使用 Apple notarization 或 Windows Authenticode，操作系统会显示未验证开发者警告。Updater 资产使用独立 Ed25519 key 签名；Release workflow 在生成 Draft 后验证 Updater manifest、`.sig`、安装包和 SHA-256 校验和，并为发布资产生成 GitHub provenance attestation。
- **诊断日志**：前端 Render Crash、未处理 Promise rejection、Updater 和操作错误经过统一结构化脱敏后，只按 `warn/error` 写入应用日志目录。日志文件最大 2 MiB，最多保留 3 个归档。

Gateway 响应上限为 4 MiB，模型发现最多接受 10,000 条记录。单个 Target 配置上限为 8 MiB 和 10,000 个模型；重复 Model ID 会被拒绝。

## 明文 Token 限制

WorkBuddy 和 CodeBuddy 的模型配置协议要求 `models.json` 包含明文 `apiKey`。EveryBuddy 无法消除这一目标产品限制。

不要把 EveryBuddy 数据库或以下目录放入 Git 仓库、公开诊断包或未经保护的同步目录：

```text
<Tauri app_data_dir>/everybuddy.db
~/.workbuddy/
~/.codebuddy/
```

Windows 对应目录位于 `%USERPROFILE%\.workbuddy\` 和 `%USERPROFILE%\.codebuddy\`。

诊断日志会移除 secret-like 字段、Authorization Header、URL Query Value、URL Credential 和常见 Token 形态。提交日志前仍需人工检查；不要直接上传未经确认的完整日志。
