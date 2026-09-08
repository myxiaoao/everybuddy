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

describe("global publishing", () => {
  it("publishes selections from all sources regardless of the browsed API", async () => {
    const prepare = vi.spyOn(api, "preparePublish");
    render(<App />);
    await screen.findAllByText("GPT-5.6");
    fireEvent.click(screen.getByRole("button", { name: /^Local Relay,/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择模型 GLM 4.5" }));
    const publish = screen.getByRole("button", { name: /预览并发布/ });
    expect(publish).toHaveAttribute(
      "title",
      expect.stringContaining("2 个来源、2 个 Model ID"),
    );
    fireEvent.click(publish);
    await waitFor(() => expect(prepare).toHaveBeenCalledOnce());
    expect(prepare).toHaveBeenCalledWith({
      sources: [
        { gatewayId: "demo-gateway", modelIds: ["gpt-5.6"] },
        { gatewayId: "demo-relay", modelIds: ["glm-4.5"] },
      ],
      targets: ["workbuddy", "codebuddy"],
    });
    const dialog = screen.getByRole("dialog", { name: "确认配置变更" });
    expect(await within(dialog).findByText("Sub2API")).toBeInTheDocument();
    expect(within(dialog).getByText("Local Relay")).toBeInTheDocument();
  });

  it("requires one explicit source per duplicate ID before preparing or executing", async () => {
    const data = await api.bootstrap();
    const model = data.models.find((model) => model.id === "gpt-5.6")!;
    vi.spyOn(api, "bootstrap").mockResolvedValue({
      ...data,
      models: [
        ...data.models,
        { ...model, key: "demo-relay::gpt-5.6", gatewayId: "demo-relay" },
      ],
    });
    const prepare = vi.spyOn(api, "preparePublish");
    const execute = vi.spyOn(api, "executePublish").mockResolvedValue({
      success: true,
      results: data.targets.map((target) => ({
        target: target.kind,
        success: true,
        rollbackAttempted: false,
        rolledBack: false,
        message: "Verified",
      })),
    });
    vi.spyOn(api, "getTargetSnapshot").mockResolvedValue({
      targets: data.targets,
      targetModelStates: data.targetModelStates.map((state) => ({
        ...state,
        matchedModelKeys: ["demo-relay::gpt-5.6"],
      })),
    });
    render(<App />);
    await screen.findAllByText("GPT-5.6");
    fireEvent.click(screen.getByRole("button", { name: /^Local Relay,/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择模型 GPT-5.6" }));
    fireEvent.click(screen.getByRole("button", { name: /预览并发布/ }));
    let dialog = screen.getByRole("dialog", { name: "选择同名模型的发布来源" });
    expect(
      within(dialog).getByRole("button", { name: "继续预览" }),
    ).toBeDisabled();
    expect(prepare).not.toHaveBeenCalled();
    expect(execute).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(
      screen.getByRole("checkbox", { name: "选择模型 GPT-5.6" }),
    ).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: /预览并发布/ }));
    dialog = screen.getByRole("dialog", { name: "选择同名模型的发布来源" });
    fireEvent.click(within(dialog).getByRole("radio", { name: /Local Relay/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: "继续预览" }));
    await waitFor(() => expect(prepare).toHaveBeenCalledOnce());
    expect(prepare).toHaveBeenCalledWith({
      sources: [
        { gatewayId: "demo-gateway", modelIds: [] },
        { gatewayId: "demo-relay", modelIds: ["gpt-5.6"] },
      ],
      targets: ["workbuddy", "codebuddy"],
    });
    expect(execute).not.toHaveBeenCalled();
    const previewDialog = screen.getByRole("dialog", { name: "确认配置变更" });
    fireEvent.click(
      await within(previewDialog).findByRole("checkbox", {
        name: /我确认使用以上所选来源/,
      }),
    );
    fireEvent.click(
      within(previewDialog).getByRole("button", { name: "发布到 2 个目标" }),
    );
    const resultDialog = await screen.findByRole("dialog", {
      name: "模型配置已写入",
    });
    await waitFor(() => expect(execute).toHaveBeenCalledOnce());
    fireEvent.click(
      within(resultDialog).getAllByRole("button", { name: "关闭" })[0],
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /^Sub2API,/ })).toBeEnabled(),
    );
    expect(
      screen.getByRole("checkbox", { name: "选择模型 GPT-5.6" }),
    ).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: /^Sub2API,/ }));
    expect(
      screen.getByRole("checkbox", { name: "选择模型 GPT-5.6" }),
    ).not.toBeChecked();
  });

  it("retains an empty source as a removal plan when its last model is unchecked", async () => {
    const prepare = vi.spyOn(api, "preparePublish");
    render(<App />);
    fireEvent.click(
      await screen.findByRole("checkbox", { name: "选择模型 GPT-5.6" }),
    );
    const publish = screen.getByRole("button", { name: /预览并发布/ });
    expect(publish).toBeEnabled();
    fireEvent.click(publish);
    await waitFor(() =>
      expect(prepare).toHaveBeenCalledWith({
        sources: [{ gatewayId: "demo-gateway", modelIds: [] }],
        targets: ["workbuddy", "codebuddy"],
      }),
    );
  });
});
