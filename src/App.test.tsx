import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { api } from "./lib/api";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("EveryBuddy workspace", () => {
  it("renders the demo gateway, models, and both configuration targets", async () => {
    render(<App />);

    await waitFor(() =>
      expect(screen.getAllByText("GPT-5.6").length).toBeGreaterThan(0),
    );
    expect(screen.getAllByText("Sub2API").length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText("v0.1.0-alpha.1")).toBeInTheDocument();
    expect(screen.getByText("Local Relay")).toBeInTheDocument();
    expect(screen.getAllByText("WorkBuddy").length).toBeGreaterThan(0);
    expect(screen.getAllByText("CodeBuddy").length).toBeGreaterThan(0);
    expect(
      screen.getByRole("checkbox", { name: "选择模型 GPT-5.6" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("banner", { name: "配置工作流" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "全部" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    const modelTable = screen.getByRole("table", { name: "模型" });
    expect(within(modelTable).getAllByRole("columnheader")).toHaveLength(3);
    within(modelTable)
      .getAllByRole("row")
      .slice(1)
      .forEach((row) => {
        expect(within(row).getAllByRole("cell")).toHaveLength(3);
      });
    expect(
      modelTable.querySelectorAll(".capability-icons [tabindex]"),
    ).toHaveLength(0);
    expect(modelTable.querySelectorAll(".capability-state-icon")).toHaveLength(
      12,
    );
    expect(
      screen.getByRole("button", { name: /GPT-5\.6.*gpt-5\.6/ }),
    ).toHaveAttribute("aria-current", "true");
    expect(document.querySelectorAll(".gateway-status-icon svg")).toHaveLength(
      2,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "发现 1 个需要关注的配置项",
    );

    fireEvent.click(screen.getByRole("button", { name: "手动添加模型" }));
    const dialog = screen.getByRole("dialog", { name: "手动添加模型" });
    expect(
      within(dialog).getByRole("button", { name: "关闭" }),
    ).toBeInTheDocument();
    fireEvent.change(
      within(dialog).getByRole("textbox", { name: /Model ID/ }),
      { target: { value: "private-model" } },
    );
    fireEvent.change(
      within(dialog).getByRole("textbox", { name: /显示名称/ }),
      { target: { value: "私有模型" } },
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "添加模型" }));

    await waitFor(() =>
      expect(screen.getAllByText("私有模型").length).toBeGreaterThanOrEqual(1),
    );
  });

  it("edits the complete WorkBuddy model configuration", async () => {
    render(<App />);

    await waitFor(() =>
      expect(screen.getAllByText("GPT-5.6").length).toBeGreaterThan(0),
    );
    fireEvent.click(screen.getByRole("button", { name: /GPT-5\.6.*gpt-5\.6/ }));
    fireEvent.click(screen.getByText("高级配置"));

    expect(
      screen.getByText("已根据模型信息自动匹配，可按 API 文档调整。"),
    ).toBeInTheDocument();

    fireEvent.change(screen.getByRole("spinbutton", { name: "输入" }), {
      target: { value: "262144" },
    });
    fireEvent.click(screen.getByRole("checkbox", { name: "超高" }));
    expect(
      screen.getByText("当前选择已覆盖自动匹配结果。"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch", { name: "自定义协议" }));
    fireEvent.click(screen.getByRole("button", { name: "保存模型配置" }));

    await waitFor(() =>
      expect(screen.getByText("模型配置已保存")).toBeInTheDocument(),
    );
  });

  it("loads an existing gateway token hidden and toggles its visibility", async () => {
    render(<App />);

    await waitFor(() =>
      expect(screen.getAllByText("Sub2API").length).toBeGreaterThan(0),
    );
    fireEvent.click(screen.getByRole("button", { name: "编辑 API" }));

    const dialog = await screen.findByRole("dialog", { name: "编辑 API" });
    const tokenInput = within(dialog).getByLabelText("Token Key");
    expect(tokenInput).toHaveAttribute("type", "password");
    expect(tokenInput).toHaveValue("demo-token-primary");

    fireEvent.click(within(dialog).getByRole("button", { name: "显示 Token" }));
    expect(tokenInput).toHaveAttribute("type", "text");
    fireEvent.click(within(dialog).getByRole("button", { name: "隐藏 Token" }));
    expect(tokenInput).toHaveAttribute("type", "password");
  });

  it("toggles a configuration target from the full card", async () => {
    render(<App />);

    await waitFor(() =>
      expect(screen.getAllByText("GPT-5.6").length).toBeGreaterThan(0),
    );
    const checkbox = screen.getByRole("checkbox", { name: "WorkBuddy" });
    const targetOption = checkbox.closest(".target-option");
    expect(targetOption).not.toBeNull();

    const initialState = (checkbox as HTMLInputElement).checked;
    fireEvent.click(within(targetOption as HTMLElement).getByText("WorkBuddy"));
    await waitFor(() =>
      expect(checkbox).toHaveProperty("checked", !initialState),
    );

    fireEvent.click(targetOption as HTMLElement);
    await waitFor(() =>
      expect(checkbox).toHaveProperty("checked", initialState),
    );
  });

  it("guards unsaved model edits before changing the active model", async () => {
    render(<App />);

    await waitFor(() =>
      expect(screen.getAllByText("GPT-5.6").length).toBeGreaterThan(0),
    );
    fireEvent.click(screen.getByRole("switch", { name: "推理模式" }));
    fireEvent.click(
      screen.getByRole("button", {
        name: /Claude Sonnet 4\.5 claude-sonnet-4-5/,
      }),
    );

    let dialog = screen.getByRole("dialog", { name: "丢弃未保存的更改" });
    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(document.querySelector(".model-summary h3")).toHaveTextContent(
      "GPT-5.6",
    );

    fireEvent.click(
      screen.getByRole("button", {
        name: /Claude Sonnet 4\.5 claude-sonnet-4-5/,
      }),
    );
    dialog = screen.getByRole("dialog", { name: "丢弃未保存的更改" });
    fireEvent.click(within(dialog).getByRole("button", { name: "丢弃更改" }));
    await waitFor(() =>
      expect(document.querySelector(".model-summary h3")).toHaveTextContent(
        "Claude Sonnet 4.5",
      ),
    );
  });

  it("guards unsaved model edits before preparing a publish", async () => {
    const preparePublish = vi.spyOn(api, "preparePublish");
    render(<App />);

    await screen.findAllByText("GPT-5.6");
    fireEvent.click(screen.getByRole("switch", { name: "推理模式" }));
    const publishButton = screen.getByRole("button", { name: /预览并发布/ });
    publishButton.focus();
    fireEvent.click(publishButton);

    const dialog = screen.getByRole("dialog", {
      name: "丢弃未保存的更改",
    });
    expect(preparePublish).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "丢弃更改" }));
    await waitFor(() => expect(preparePublish).toHaveBeenCalledOnce());
    const publishDialog = await screen.findByRole("dialog", {
      name: "确认配置变更",
    });
    fireEvent.click(
      within(publishDialog).getByRole("button", { name: "关闭" }),
    );
    await waitFor(() => expect(publishButton).toHaveFocus());
  });

  it("returns to models when the selected API source is chosen again", async () => {
    render(<App />);

    await screen.findAllByText("GPT-5.6");
    const shell = document.querySelector(".app-shell");
    fireEvent.click(screen.getByRole("button", { name: /API.*Sub2API/ }));
    expect(shell).toHaveClass("compact-view-gateways");

    fireEvent.click(
      screen.getByRole("button", {
        name: /Sub2API, https:\/\/api\.example\.dev\/v1/,
      }),
    );
    expect(shell).toHaveClass("compact-view-models");
  });

  it("moves focus into the workspace from the skip link", async () => {
    render(<App />);

    await screen.findAllByText("GPT-5.6");
    const skipLink = screen.getByRole("link", { name: "跳到工作区" });
    fireEvent.click(skipLink);

    expect(screen.getByRole("main")).toHaveFocus();
  });

  it("restores focus to the button that opened a dialog", async () => {
    render(<App />);

    await screen.findAllByText("GPT-5.6");
    const settingsButton = screen.getByRole("button", { name: "设置" });
    settingsButton.focus();
    fireEvent.click(settingsButton);
    const dialog = screen.getByRole("dialog", { name: "设置" });
    fireEvent.click(within(dialog).getByRole("button", { name: "关闭" }));

    await waitFor(() => expect(settingsButton).toHaveFocus());
  });

  it("does not change the active model when selecting models for publish", async () => {
    render(<App />);

    await waitFor(() =>
      expect(screen.getAllByText("GPT-5.6").length).toBeGreaterThan(0),
    );
    fireEvent.click(
      screen.getByRole("checkbox", { name: /模型 Claude Sonnet 4\.5/ }),
    );

    expect(document.querySelector(".model-summary h3")).toHaveTextContent(
      "GPT-5.6",
    );
  });

  it("hydrates checked and indeterminate selection without publishing partial matches", async () => {
    const data = await api.bootstrap();
    vi.spyOn(api, "bootstrap").mockResolvedValueOnce({
      ...data,
      settings: {
        ...data.settings,
        selectedTargets: ["workbuddy", "codebuddy"],
      },
      targetModelStates: [
        {
          target: "workbuddy",
          fingerprint: "work",
          matchedModelKeys: [
            "demo-gateway::gpt-5.6",
            "demo-gateway::claude-sonnet-4-5",
          ],
          unmatchedCount: 0,
          skippedCount: 0,
        },
        {
          target: "codebuddy",
          fingerprint: "code",
          matchedModelKeys: ["demo-gateway::gpt-5.6"],
          unmatchedCount: 0,
          skippedCount: 0,
        },
      ],
    });
    const preparePublish = vi.spyOn(api, "preparePublish");
    render(<App />);

    const checked = await screen.findByRole("checkbox", {
      name: "选择模型 GPT-5.6",
    });
    const partial = screen.getByRole("checkbox", {
      name: /模型 Claude Sonnet 4\.5 已存在于 WorkBuddy/,
    });
    expect(checked).toHaveAttribute("data-state", "checked");
    expect(partial).toHaveAttribute("aria-checked", "mixed");

    fireEvent.click(screen.getByRole("button", { name: /预览并发布/ }));

    await waitFor(() => expect(preparePublish).toHaveBeenCalled());
    expect(preparePublish).toHaveBeenCalledWith(
      expect.objectContaining({ modelIds: ["gpt-5.6"] }),
    );
  });

  it("promotes an indeterminate model to checked for all selected targets", async () => {
    render(<App />);

    const partial = await screen.findByRole("checkbox", {
      name: /模型 Claude Sonnet 4\.5 已存在于 WorkBuddy/,
    });
    expect(partial).toHaveAttribute("aria-checked", "mixed");
    fireEvent.click(partial);

    expect(
      screen.getByRole("checkbox", { name: "选择模型 Claude Sonnet 4.5" }),
    ).toHaveAttribute("data-state", "checked");
  });

  it("shows a filter-specific empty state and clears all filters", async () => {
    render(<App />);

    const search = await screen.findByRole("searchbox", {
      name: "搜索名称、模型 ID 或提供商",
    });
    fireEvent.change(search, { target: { value: "no-such-model" } });

    expect(
      await screen.findByRole("heading", { name: "没有匹配的模型" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "清除筛选" }));
    expect(search).toHaveValue("");
    expect(screen.getAllByText("GPT-5.6").length).toBeGreaterThan(0);
  });

  it("announces a localized error once and includes recovery guidance", async () => {
    render(<App />);

    await waitFor(() =>
      expect(screen.getAllByText("GPT-5.6").length).toBeGreaterThan(0),
    );
    fireEvent.click(screen.getByRole("button", { name: "手动添加模型" }));
    const dialog = screen.getByRole("dialog", { name: "手动添加模型" });
    fireEvent.change(
      within(dialog).getByRole("textbox", { name: /Model ID/ }),
      { target: { value: "gpt-5.6" } },
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "添加模型" }));

    const alert = await screen.findByRole("alert");
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(alert).toHaveTextContent("模型已存在");
    expect(alert).toHaveTextContent("当前 API 来源中已有相同的 Model ID");
    expect(alert).toHaveTextContent("使用其他 Model ID，或编辑已有模型");
    expect(screen.getByRole("status")).not.toHaveTextContent("模型已存在");
  });

  it("uses app dialogs for destructive confirmations", async () => {
    const nativeConfirm = vi.spyOn(window, "confirm");
    render(<App />);

    await waitFor(() =>
      expect(screen.getAllByText("Sub2API").length).toBeGreaterThan(0),
    );
    fireEvent.click(screen.getByRole("button", { name: "移除" }));

    const deleteDialog = screen.getByRole("dialog", { name: "移除 API 来源" });
    expect(deleteDialog).toBeInTheDocument();
    fireEvent.click(within(deleteDialog).getByRole("button", { name: "取消" }));
    fireEvent.click(screen.getByRole("button", { name: "备份与恢复" }));
    fireEvent.click(await screen.findByRole("button", { name: "恢复" }));
    expect(
      screen.getByRole("dialog", { name: "恢复配置备份" }),
    ).toBeInTheDocument();
    expect(nativeConfirm).not.toHaveBeenCalled();
  });

  it("excludes unavailable targets from publish requests", async () => {
    const data = await api.bootstrap();
    vi.spyOn(api, "bootstrap").mockResolvedValueOnce({
      ...data,
      targets: data.targets.map((target) =>
        target.kind === "codebuddy" ? { ...target, writable: false } : target,
      ),
      settings: {
        ...data.settings,
        selectedTargets: ["workbuddy", "codebuddy"],
      },
    });
    const preparePublish = vi.spyOn(api, "preparePublish");
    render(<App />);

    const modelCheckbox = await screen.findByRole("checkbox", {
      name: "选择模型 GPT-5.6",
    });
    fireEvent.click(modelCheckbox);
    fireEvent.click(screen.getByRole("button", { name: /预览并发布/ }));

    await waitFor(() => expect(preparePublish).toHaveBeenCalled());
    expect(preparePublish).toHaveBeenCalledWith(
      expect.objectContaining({ targets: ["workbuddy"] }),
    );
  });

  it("keeps newly saved settings when target availability is refreshed", async () => {
    const current = await api.bootstrap();
    const data = {
      ...current,
      settings: {
        ...current.settings,
        selectedTargets: ["workbuddy", "codebuddy"] as Array<
          "workbuddy" | "codebuddy"
        >,
      },
    };
    vi.spyOn(api, "bootstrap").mockResolvedValueOnce(data);
    vi.spyOn(api, "getTargetStatuses").mockResolvedValue(
      data.targets.map((target) =>
        target.kind === "codebuddy" ? { ...target, writable: false } : target,
      ),
    );
    vi.spyOn(api, "getTargetModelStates").mockResolvedValue(
      data.targetModelStates,
    );
    const saveSettings = vi
      .spyOn(api, "saveSettings")
      .mockImplementation(async (settings) => settings);
    render(<App />);

    await screen.findAllByText("GPT-5.6");
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    const dialog = screen.getByRole("dialog", { name: "设置" });
    fireEvent.click(within(dialog).getByRole("radio", { name: "深色" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "保存" }));

    await waitFor(() => expect(saveSettings).toHaveBeenCalledTimes(2));
    expect(
      saveSettings.mock.calls[saveSettings.mock.calls.length - 1]?.[0],
    ).toMatchObject({
      theme: "dark",
      selectedTargets: ["workbuddy"],
    });
  });

  it("disables conflicting API and model changes while a gateway is refreshing", async () => {
    let finishRefresh:
      | ((models: Awaited<ReturnType<typeof api.discoverModels>>) => void)
      | undefined;
    vi.spyOn(api, "discoverModels").mockImplementation(
      () =>
        new Promise((resolve) => {
          finishRefresh = resolve;
        }),
    );
    render(<App />);

    await screen.findAllByText("GPT-5.6");
    fireEvent.click(screen.getAllByRole("button", { name: "刷新模型" })[0]);

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "手动添加模型" }),
      ).toBeDisabled(),
    );
    expect(screen.getByRole("button", { name: "添加 API" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "编辑 API" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "移除" })).toBeDisabled();
    for (const button of screen.getAllByRole("button", { name: "刷新模型" })) {
      expect(button).toBeDisabled();
    }
    finishRefresh?.([]);
  });
});
