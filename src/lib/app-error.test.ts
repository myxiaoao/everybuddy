import { describe, expect, it } from "vitest";
import { createTranslator } from "./i18n";
import { localizedError } from "./app-error";

describe("localizedError", () => {
  it("explains when a model is absent from the OpenRouter catalog", () => {
    expect(
      localizedError(
        {
          code: "VALIDATION_ERROR",
          message:
            "This model is not available in the OpenRouter model catalog",
        },
        createTranslator("zh-CN"),
      ),
    ).toEqual({
      title: "OpenRouter 未找到此模型",
      message: "当前 Model ID 未在 OpenRouter 公共模型目录中匹配到。",
      recovery: "确认 Model ID 和 vendor，刷新模型列表后重试。",
    });
  });

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

  it("explains how to recover a missing SQLite credential", () => {
    expect(
      localizedError(
        {
          code: "CREDENTIAL_ERROR",
          message: "The gateway token is missing from the local database",
        },
        t,
      ),
    ).toEqual({
      title: "无法读取 Token",
      message: "本地数据库中没有此 API 的 Token。",
      recovery: "重新编辑并保存 API Token 后重试。",
    });
  });
});
