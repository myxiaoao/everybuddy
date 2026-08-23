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
  });
});
