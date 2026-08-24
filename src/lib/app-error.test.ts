import { describe, expect, it } from "vitest";
import { createTranslator } from "./i18n";
import { localizedError } from "./app-error";

describe("localizedError", () => {
  const t = createTranslator("zh-CN");

  it.each([
    ["Enter a valid HTTP or HTTPS API URL", "API 地址无效"],
    ["Select at least one model to publish", "发布范围不完整"],
    ["Temperature must be a finite number", "模型参数无效"],
    ["Gateway profile not found", "本地数据已变化"],
    ["Unsupported theme", "设置无法保存"],
    ["Model ID is required", "缺少必填内容"],
  ])("maps validation detail %s to %s", (message, title) => {
    expect(localizedError({ code: "VALIDATION_ERROR", message }, t).title).toBe(
      title,
    );
  });

  it("keeps a safe fallback for unknown validation details", () => {
    expect(
      localizedError(
        { code: "VALIDATION_ERROR", message: "Unknown validation" },
        t,
      ),
    ).toEqual({
      title: "无法提交当前内容",
      message: "当前输入或选择不符合操作要求。",
      recovery: "检查必填项、模型选择和发布目标后重试。",
    });
  });
});
