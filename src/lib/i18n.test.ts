import { describe, expect, it } from "vitest";
import { createTranslator } from "./i18n";

describe("translator", () => {
  it("interpolates numeric values in both languages", () => {
    expect(createTranslator("zh-CN")("modelCount", { count: 4 })).toBe(
      "4 个可用模型",
    );
    expect(createTranslator("en")("modelCount", { count: 4 })).toBe(
      "4 available",
    );
    expect(createTranslator("zh-CN")("supported")).toBe("支持");
    expect(createTranslator("zh-CN")("toolCall")).toBe("工具调用");
    expect(createTranslator("zh-CN")("supportedEfforts")).toBe(
      "支持的思考强度",
    );
    expect(createTranslator("en")("unsupported")).toBe("Unsupported");
    expect(createTranslator("zh-CN")("reasoningEffortsUnknown")).toBe(
      "未发现可靠范围，请按 API 文档确认。",
    );
  });

  it("pluralizes English count messages", () => {
    const t = createTranslator("en");

    expect(t("selectedTargetsCount", { count: 1 })).toBe("1 target selected");
    expect(t("selectedTargetsCount", { count: 2 })).toBe("2 targets selected");
    expect(t("publishScope", { models: 1, targets: 2 })).toBe(
      "1 model → 2 targets",
    );
    expect(t("publishScope", { models: 2, targets: 1 })).toBe(
      "2 models → 1 target",
    );
    expect(t("confirmPublish", { count: 1 })).toBe("Publish to 1 target");
    expect(t("importSucceeded", { gateways: 1, models: 1 })).toBe(
      "Imported 1 API and 1 model from target configuration.",
    );
    expect(t("importAnnouncement", { gateways: 1, models: 1, issues: 1 })).toBe(
      "Startup configuration recovery completed: imported 1 API and 1 model, with 1 configuration item needing attention.",
    );
    expect(t("importNoticeSummary", { count: 1 })).toBe(
      "1 configuration item was skipped or differs between targets.",
    );
  });

  it("describes protocol model IDs without assuming chat completions", () => {
    const t = createTranslator("en");

    expect(t("manualModelIdHint")).toBe(
      "Enter the model value used by the protocol. For standard OpenAI-compatible endpoints, this is the model field in the /chat/completions request.",
    );
    expect(t("probe")).toBe("Run probe");
  });
});
