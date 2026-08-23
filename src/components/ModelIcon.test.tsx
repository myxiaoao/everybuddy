import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { resolveModelIcon } from "@/lib/model-icon";
import { ModelIcon } from "./ModelIcon";

afterEach(() => cleanup());

describe("ModelIcon", () => {
  it.each([
    ["gpt-5.6", "GPT-5.6", "custom", "openai"],
    ["claude-sonnet-4-5", "Claude Sonnet 4.5", "anthropic", "claude"],
    ["deepseek-r1", "DeepSeek R1", "custom", "deepseek"],
    ["qwen3-coder", "Qwen3 Coder", "alibaba", "qwen"],
    ["glm-4.5", "GLM 4.5", "custom", "zhipu"],
    ["kimi-k2", "Kimi K2", "moonshot", "kimi"],
    ["moonshot-v1", "Moonshot V1", "moonshot", "moonshot"],
    ["gemini-2.5-pro", "Gemini 2.5 Pro", "google", "gemini"],
  ])("resolves %s to the %s brand", (id, name, vendor, expected) => {
    expect(resolveModelIcon({ id, name, vendor })?.brand).toBe(expected);
  });

  it("keeps a text fallback for an unknown vendor", () => {
    const { container } = render(
      <ModelIcon
        model={{ id: "private-model", name: "Private Model", vendor: "acme" }}
      />,
    );

    expect(
      container.querySelector("[data-model-brand='custom']"),
    ).toHaveTextContent("AC");
  });

  it("renders the colored DeepSeek asset without a monochrome mask", () => {
    const { container } = render(
      <ModelIcon
        model={{ id: "deepseek-r1", name: "DeepSeek R1", vendor: "deepseek" }}
      />,
    );

    expect(
      container.querySelector("[data-model-brand='deepseek'] img"),
    ).toBeInTheDocument();
    expect(
      container.querySelector(
        "[data-model-brand='deepseek'] .vendor-mark__glyph",
      ),
    ).not.toBeInTheDocument();
  });
});
