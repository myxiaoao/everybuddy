import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranslator } from "../lib/i18n";
import { defaultSettings } from "../lib/target-utils";
import type { PublishResult } from "../types";
import { PublishDialog, SettingsDialog } from "./Dialogs";

afterEach(() => cleanup());

describe("PublishDialog", () => {
  it("distinguishes rolled back and failed targets", () => {
    const result: PublishResult = {
      success: false,
      results: [
        {
          target: "workbuddy",
          success: false,
          rollbackAttempted: true,
          rolledBack: true,
          message: "Published changes were rolled back",
        },
        {
          target: "codebuddy",
          success: false,
          rollbackAttempted: false,
          rolledBack: false,
          message: "Write failed",
        },
      ],
    };

    render(
      <PublishDialog
        open
        busy={false}
        preview={null}
        result={result}
        t={createTranslator("zh-CN")}
        onClose={() => undefined}
        onConfirm={() => undefined}
      />,
    );

    expect(
      screen
        .getByText("其他目标发布失败，已恢复发布前配置。")
        .closest(".is-rolled-back"),
    ).not.toBeNull();
    expect(
      screen
        .getByText("写入失败，请检查目标路径和权限后重试。")
        .closest(".is-failure"),
    ).not.toBeNull();
  });

  it("distinguishes rollback failures from the target write failure", () => {
    const result: PublishResult = {
      success: false,
      results: [
        {
          target: "workbuddy",
          success: false,
          rollbackAttempted: true,
          rolledBack: false,
          message: "Rollback failed",
        },
      ],
    };

    render(
      <PublishDialog
        open
        busy={false}
        preview={null}
        result={result}
        t={createTranslator("zh-CN")}
        onClose={() => undefined}
        onConfirm={() => undefined}
      />,
    );

    expect(
      screen
        .getByText("发布和回滚均失败，请立即检查目标配置文件。")
        .closest(".is-rollback-failed"),
    ).not.toBeNull();
  });
});

describe("SettingsDialog", () => {
  it("shows the current version and checks for updates", () => {
    const onCheckForUpdates = vi.fn();

    render(
      <SettingsDialog
        open
        busy={false}
        settings={defaultSettings}
        currentVersion="0.1.0"
        availableVersion={null}
        updateCheckStatus="latest"
        installingUpdate={false}
        t={createTranslator("zh-CN")}
        onClose={() => undefined}
        onSubmit={() => undefined}
        onCheckForUpdates={onCheckForUpdates}
        onInstallUpdate={() => undefined}
      />,
    );

    const dialog = screen.getByRole("dialog", { name: "设置" });
    expect(within(dialog).getByText("v0.1.0")).toBeInTheDocument();
    expect(within(dialog).getByRole("status")).toHaveTextContent(
      "当前已是最新版本",
    );

    fireEvent.click(within(dialog).getByRole("button", { name: "检查更新" }));
    expect(onCheckForUpdates).toHaveBeenCalledOnce();
  });

  it("does not present prerelease channel results as globally latest", () => {
    render(
      <SettingsDialog
        open
        busy={false}
        settings={defaultSettings}
        currentVersion="0.1.0-alpha.1"
        availableVersion={null}
        updateCheckStatus="latest"
        installingUpdate={false}
        t={createTranslator("zh-CN")}
        onClose={() => undefined}
        onSubmit={() => undefined}
        onCheckForUpdates={() => undefined}
        onInstallUpdate={() => undefined}
      />,
    );

    expect(screen.getByRole("status")).toHaveTextContent(
      "当前更新通道未发现新版本。预发布版本请通过 GitHub Releases 获取更新。",
    );
  });

  it("disables repeated checks while checking", () => {
    render(
      <SettingsDialog
        open
        busy={false}
        settings={defaultSettings}
        currentVersion="0.1.0-alpha.1"
        availableVersion={null}
        updateCheckStatus="checking"
        installingUpdate={false}
        t={createTranslator("zh-CN")}
        onClose={() => undefined}
        onSubmit={() => undefined}
        onCheckForUpdates={() => undefined}
        onInstallUpdate={() => undefined}
      />,
    );

    expect(screen.getByRole("button", { name: "正在检查更新" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("正在检查更新");
  });

  it("offers installation when an update is available", () => {
    const onInstallUpdate = vi.fn();

    render(
      <SettingsDialog
        open
        busy={false}
        settings={defaultSettings}
        currentVersion="0.1.0-alpha.1"
        availableVersion="0.2.0"
        updateCheckStatus="available"
        installingUpdate={false}
        t={createTranslator("zh-CN")}
        onClose={() => undefined}
        onSubmit={() => undefined}
        onCheckForUpdates={() => undefined}
        onInstallUpdate={onInstallUpdate}
      />,
    );

    expect(screen.getByRole("status")).toHaveTextContent(
      "EveryBuddy 0.2.0 已可用",
    );
    fireEvent.click(screen.getByRole("button", { name: "更新并重启" }));
    expect(onInstallUpdate).toHaveBeenCalledOnce();
  });

  it("reports update check failures without blocking settings", () => {
    render(
      <SettingsDialog
        open
        busy={false}
        settings={defaultSettings}
        currentVersion="0.1.0-alpha.1"
        availableVersion={null}
        updateCheckStatus="error"
        installingUpdate={false}
        t={createTranslator("zh-CN")}
        onClose={() => undefined}
        onSubmit={() => undefined}
        onCheckForUpdates={() => undefined}
        onInstallUpdate={() => undefined}
      />,
    );

    expect(screen.getByRole("status")).toHaveTextContent(
      "检查更新失败，请稍后重试。",
    );
    expect(screen.getByRole("button", { name: "保存" })).toBeEnabled();
  });
});
