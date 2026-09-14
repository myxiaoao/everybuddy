import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { api } from "./lib/api";

const updater = vi.hoisted(() => ({
  currentVersion: "0.1.3",
  availableUpdate: null as { version: string } | null,
  updateCheckStatus: "latest",
  installingUpdate: false,
  restartRequired: false,
  checkForUpdates: vi.fn(),
  installUpdate: vi.fn(),
}));
vi.mock("./hooks/use-app-updater", () => ({ useAppUpdater: () => updater }));

beforeEach(() => {
  updater.availableUpdate = null;
  updater.installingUpdate = false;
  updater.installUpdate.mockReset().mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("workspace operation recovery", () => {
  it("requires discard confirmation before updating with unsaved model edits", async () => {
    updater.availableUpdate = { version: "0.2.0" };
    render(<App />);
    await screen.findAllByText("GPT-5.6");
    fireEvent.click(screen.getByText("高级配置"));
    fireEvent.change(screen.getByRole("textbox", { name: "模型名称" }), {
      target: { value: "Unsaved name" },
    });
    fireEvent.click(screen.getByRole("button", { name: "更新并重启" }));
    const confirmation = screen.getByRole("dialog", {
      name: "丢弃未保存的更改",
    });
    expect(updater.installUpdate).not.toHaveBeenCalled();
    fireEvent.click(within(confirmation).getByRole("button", { name: "取消" }));
    expect(screen.getByRole("textbox", { name: "模型名称" })).toHaveValue(
      "Unsaved name",
    );
    fireEvent.click(screen.getByRole("button", { name: "更新并重启" }));
    fireEvent.click(screen.getByRole("button", { name: "丢弃更改" }));
    await waitFor(() => expect(updater.installUpdate).toHaveBeenCalledOnce());
  });

  it("blocks updating while a gateway request is in flight and editing while installing", async () => {
    updater.availableUpdate = { version: "0.2.0" };
    let finish!: (
      models: Awaited<ReturnType<typeof api.discoverModels>>,
    ) => void;
    vi.spyOn(api, "discoverModels").mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const view = render(<App />);
    await screen.findAllByText("GPT-5.6");
    fireEvent.click(screen.getAllByRole("button", { name: "刷新模型" })[0]);
    expect(screen.getByRole("button", { name: "更新并重启" })).toBeDisabled();
    await act(async () => {
      finish([]);
    });
    updater.installingUpdate = true;
    view.rerender(<App />);
    expect(document.getElementById("workspace")).toHaveAttribute("inert");
    expect(screen.getByRole("button", { name: "更新并重启" })).toBeDisabled();
  });

  it("distinguishes an unavailable OpenRouter catalog and retries explicitly", async () => {
    const match = vi
      .spyOn(api, "getOpenRouterModelMatch")
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValue("openai/gpt-test");
    render(<App />);
    const retry = await screen.findByRole("button", {
      name: "重新查询 OpenRouter",
    });
    expect(
      screen.getByText("暂时无法查询 OpenRouter。请检查网络后重试。"),
    ).toBeInTheDocument();
    fireEvent.click(retry);
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "从 OpenRouter 设置" }),
      ).toBeEnabled(),
    );
    expect(match).toHaveBeenLastCalledWith("demo-gateway::gpt-5.6", true);
  });

  it("keeps a committed restore successful when refreshing backups fails", async () => {
    const backups = await api.listBackups();
    vi.spyOn(api, "listBackups")
      .mockResolvedValueOnce(backups)
      .mockRejectedValue(new Error("storage temporarily unavailable"));
    const restore = vi.spyOn(api, "restoreBackup").mockResolvedValue(undefined);
    render(<App />);
    await screen.findAllByText("GPT-5.6");
    fireEvent.click(screen.getByRole("button", { name: "备份与恢复" }));
    fireEvent.click(await screen.findByRole("button", { name: "恢复" }));
    fireEvent.click(screen.getByRole("button", { name: "恢复备份" }));
    await screen.findByText(/配置已恢复，但备份列表刷新失败/);
    expect(
      screen.queryByRole("dialog", { name: "恢复配置备份" }),
    ).not.toBeInTheDocument();
    expect(restore).toHaveBeenCalledOnce();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("marks a failed target poll stale and clears it on a successful retry", async () => {
    const snapshot = await api.getTargetSnapshot();
    vi.spyOn(api, "getTargetSnapshot")
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValue(snapshot);
    vi.useFakeTimers({ shouldAdvanceTime: true });
    render(<App />);
    await screen.findAllByText("GPT-5.6");
    await act(async () => {
      vi.advanceTimersByTime(5_000);
    });
    expect(await screen.findByText(/目标状态暂时无法刷新/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    await waitFor(() =>
      expect(
        screen.queryByText(/目标状态暂时无法刷新/),
      ).not.toBeInTheDocument(),
    );
  });

  it("confirms and runs unmatched target cleanup with a backup", async () => {
    const cleanupTarget = vi
      .spyOn(api, "cleanupUnmatchedModels")
      .mockResolvedValue({ target: "codebuddy", removedCount: 1 });
    render(<App />);
    await screen.findAllByText("GPT-5.6");

    fireEvent.click(
      screen.getByRole("button", {
        name: "清理 CodeBuddy 中 1 个未匹配模型",
      }),
    );
    const confirmation = screen.getByRole("dialog", {
      name: "清理未匹配模型",
    });
    expect(confirmation).toHaveTextContent("将从 CodeBuddy 配置中移除 1 个");
    fireEvent.click(
      within(confirmation).getByRole("button", { name: "清理并备份" }),
    );

    await waitFor(() =>
      expect(cleanupTarget).toHaveBeenCalledWith("codebuddy"),
    );
    expect(document.querySelector(".live-region")).toHaveTextContent(
      "已从 CodeBuddy 清理 1 个未匹配模型",
    );
  });

  it("retries a failed pending write recovery in the current session", async () => {
    const data = await api.bootstrap();
    vi.spyOn(api, "bootstrap").mockResolvedValue({
      ...data,
      importReport: {
        ...data.importReport,
        issues: [
          {
            target: "workbuddy",
            modelId: null,
            code: "interruptedWriteFailed",
            message: "recovery failed",
          },
        ],
      },
    });
    const recover = vi.spyOn(api, "recoverPendingWrites").mockResolvedValue([
      {
        target: "workbuddy",
        modelId: null,
        code: "interruptedWriteRecovered",
        message: "recovered",
      },
    ]);
    render(<App />);
    await screen.findAllByText("GPT-5.6");

    fireEvent.click(screen.getByRole("button", { name: "重试恢复写入" }));
    await waitFor(() => expect(recover).toHaveBeenCalledOnce());
    expect(
      screen.queryByRole("button", { name: "重试恢复写入" }),
    ).not.toBeInTheDocument();
  });

  it("offers recovery after a publish failure leaves a pending write", async () => {
    vi.spyOn(api, "executePublish").mockRejectedValue(
      new Error("write failed"),
    );
    const recover = vi.spyOn(api, "recoverPendingWrites").mockResolvedValue([
      {
        target: "workbuddy",
        modelId: null,
        code: "interruptedWriteFailed",
        message: "still pending",
      },
    ]);
    render(<App />);
    await screen.findAllByText("GPT-5.6");

    fireEvent.click(screen.getByRole("button", { name: /预览并发布/ }));
    const preview = await screen.findByRole("dialog", {
      name: "确认配置变更",
    });
    fireEvent.click(
      await within(preview).findByRole("checkbox", {
        name: /我确认使用以上所选来源/,
      }),
    );
    fireEvent.click(
      within(preview).getByRole("button", { name: "发布到 2 个目标" }),
    );

    await waitFor(() => expect(recover).toHaveBeenCalledOnce());
    fireEvent.click(within(preview).getByRole("button", { name: "取消" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "确认配置变更" }),
      ).not.toBeInTheDocument(),
    );
    expect(
      await screen.findByRole("button", { name: "重试恢复写入" }),
    ).toBeInTheDocument();
  });
});
