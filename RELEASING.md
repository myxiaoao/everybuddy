# 发布流程

EveryBuddy 首个发布通道为 Alpha。当前版本 `0.1.0-alpha.1` 对应 Tag `v0.1.0-alpha.1`，GitHub Release 必须保持 Prerelease 状态。

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

3. 创建与版本一致的 annotated Tag，并把 Tag 推送到 GitHub。
4. 在 GitHub Actions 中批准 `release` Environment deployment。
5. 等待 Release workflow 完成。Workflow 不配置 Apple 或 Windows signing identity，创建 Draft Prerelease，并验证 Updater manifest、`.sig`、安装包和校验和。
6. 下载 Draft 中的安装包、`latest.json`、`.sig` 和 `SHA256SUMS.txt`，复核文件名称、版本和 SHA-256。
7. 在干净环境中安装并启动 macOS 与 Windows 包，确认 Gatekeeper 和 SmartScreen 警告与 README 一致，并验证启动、配置读取和发布预览。Alpha Prerelease 使用手动更新，不把应用内 Updater 检查作为发布门槛。
8. 保持 Prerelease 状态，手动发布 Draft。

## 当前签名策略

- macOS 和 Windows Alpha 安装包没有平台代码签名，Release 页面必须保留 `Unsigned Alpha` 警告。
- Updater 资产使用 Tauri Ed25519 key 签名。Updater private key 不等同于 Apple 或 Windows 平台证书，不能消除系统安装警告。
- GitHub `releases/latest` 不会选择 Prerelease。Alpha 用户通过 Release 页面手动更新；稳定更新通道启用前，不宣称应用内自动更新可用。
- Updater private key 的本机备份位于受限目录，密码保存在 macOS Keychain；GitHub 只保存 Environment Secret。
- GitHub Secret 无法读取原值。首个 Release 前必须把加密私钥另存到离线安全介质，否则本机丢失后无法继续为现有客户端签名更新。
- 获得 Apple Developer ID 和 Windows Code Signing Certificate 后，再恢复 notarization、Authenticode 和对应验证步骤。

## 失败处理

任何 Updater 签名、安装包校验或安装后 Smoke Test 失败时，不发布 Draft。不要覆盖已有 Tag 或 Release 资产；修复后递增 Prerelease 序号，重新执行发布流程。
