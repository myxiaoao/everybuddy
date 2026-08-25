# 贡献指南

感谢参与 EveryBuddy。提交代码前，请先阅读 [行为准则](CODE_OF_CONDUCT.md) 和[安全策略](SECURITY.md)。安全漏洞不要提交到公开 Issue。

## 开始开发

前置条件与启动方式见 [README](README.md#本地开发)。安装依赖后，使用以下命令完成本地验证：

```bash
pnpm verify
```

修改安装包、Tauri capability、Updater 或平台相关代码时，还需要执行：

```bash
pnpm tauri build
```

## 提交修改

- 功能和行为修改先创建 Issue，说明问题、预期结果和适用范围。
- 一个 Pull Request 只处理一个主题，不混入无关重构。
- 用户可见文案同时更新简体中文和 English。
- 修改 IPC command 时，同时更新 Rust command registry、`src/lib/api.ts` 和相关类型。
- 修改数据格式、凭据处理、目标写入或恢复逻辑时，补充回归测试。
- 不提交真实 Token、`models.json`、日志、数据库、签名证书或本机路径。

Commit 使用 Conventional Commits：

```text
feat(gateway): add model discovery filter
fix(publish): preserve unknown reasoning fields
docs(release): clarify signing prerequisites
```

## Pull Request

Pull Request 需要说明修改内容、原因、验证结果和安全影响。维护者可能要求补充 macOS 或 Windows 的实际运行证据。
