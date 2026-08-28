# 发布流程

EveryBuddy 当前发布通道为 Stable。当前版本 `0.1.1` 对应 Tag `v0.1.1`，GitHub Release 必须保持正式发行状态。

## 前置条件

- `main` 已通过 `CI Gate`。
- GitHub `release` Environment 已配置 Required Reviewer。
- `TAURI_UPDATER_PUBLIC_KEY` 已配置为 Actions Variable。
- `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 已配置为 GitHub Secrets。
- 发布 commit 已包含在 `main` 中，工作树没有遗漏的生成文件。

完整凭据名称见[技术设计](docs/TECHNICAL_DESIGN.md#12-测试与发布)。

## 发布步骤

1. 同步 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 和 `CHANGELOG.md` 的版本。
2. 执行完整验证：

   ```bash
   pnpm install --frozen-lockfile
   pnpm verify
   pnpm tauri build
   ```

3. 在 GitHub Actions 中从 `main` 手动运行 Release workflow，并批准 `release` Environment deployment。该 Preflight 只构建和检查安装包及 `.sig`，不会创建 Tag 或 Release。
4. Preflight 通过后，创建与版本一致的 annotated Tag，并把 Tag 推送到 GitHub。
5. 再次批准 `release` Environment deployment，等待 Tag 触发的 Release workflow 完成。Workflow 不配置 Apple 或 Windows signing identity，创建 Draft Release，并验证 Updater manifest、`.sig`、安装包和校验和。
6. 下载 Draft 中的安装包、`latest.json`、`.sig` 和 `SHA256SUMS.txt`，复核文件名称、版本和 SHA-256，并使用 `gh attestation verify <asset> --repo myxiaoao/everybuddy --signer-workflow myxiaoao/everybuddy/.github/workflows/release.yml` 验证来源证明。
7. 在干净环境中安装并启动 macOS 与 Windows 包，确认 Gatekeeper 和 SmartScreen 警告与 README 一致，并验证启动、配置读取和发布预览。存在上一公开版本时，还要验证应用内 Updater 能识别本次稳定版本。
8. 保持正式发行状态，手动发布 Draft。

## 当前签名策略

- macOS 和 Windows 安装包没有平台代码签名，Release 页面必须保留 `Unsigned Installers` 警告。
- Updater 资产使用 Tauri Ed25519 key 签名。Updater private key 不等同于 Apple 或 Windows 平台证书，不能消除系统安装警告。
- GitHub `releases/latest` 指向最新稳定版本，应用内 Updater 只接受通过 Ed25519 签名校验的更新。
- Updater private key 的本机备份位于受限目录，密码保存在 macOS Keychain；GitHub 只保存 Environment Secret。
- GitHub Secret 无法读取原值。加密私钥必须保留离线备份，否则本机丢失后无法继续为现有客户端签名更新。
- 获得 Apple Developer ID 和 Windows Code Signing Certificate 后，再恢复 notarization、Authenticode 和对应验证步骤。

## 失败处理

任何 Updater 签名、安装包校验或安装后 Smoke Test 失败时，不发布 Draft。不要覆盖已有 Tag 或 Release 资产；修复后递增版本号，重新执行发布流程。
