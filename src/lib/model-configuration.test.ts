import { describe, expect, it } from "vitest";
import type { ModelConfiguration } from "../types";
import { isValidModelConfiguration } from "./model-configuration";

const base: ModelConfiguration = {
  endpointOverride: null,
  maxInputTokens: null,
  maxOutputTokens: null,
  temperature: null,
  onlyReasoning: false,
  reasoning: {
    effort: null,
    defaultEffort: null,
    supportedEfforts: [],
    summary: null,
    canDisableThinking: true,
  },
  useCustomProtocol: false,
};

describe("model configuration validation", () => {
  it("rejects values that JSON would coerce or cannot represent safely", () => {
    expect(isValidModelConfiguration({ ...base, temperature: Infinity })).toBe(
      false,
    );
    expect(isValidModelConfiguration({ ...base, temperature: -0.1 })).toBe(
      false,
    );
    expect(
      isValidModelConfiguration({
        ...base,
        maxInputTokens: Number.MAX_SAFE_INTEGER + 1,
      }),
    ).toBe(false);
    expect(isValidModelConfiguration({ ...base, maxOutputTokens: 1.5 })).toBe(
      false,
    );
  });

  it("accepts finite non-negative temperatures and positive safe integers", () => {
    expect(
      isValidModelConfiguration({
        ...base,
        maxInputTokens: 200_000,
        maxOutputTokens: 32_000,
        temperature: 0,
      }),
    ).toBe(true);
  });

  it("reads legacy summaries but requires a supported value before saving", () => {
    for (const summary of ["always", "never"] as const) {
      expect(
        isValidModelConfiguration({
          ...base,
          reasoning: { ...base.reasoning, summary },
        }),
      ).toBe(false);
    }
    expect(
      isValidModelConfiguration({
        ...base,
        reasoning: { ...base.reasoning, summary: "detailed" },
      }),
    ).toBe(true);
  });

  it("requires a complete endpoint override for custom protocol", () => {
    expect(
      isValidModelConfiguration({
        ...base,
        useCustomProtocol: true,
      }),
    ).toBe(false);
    expect(
      isValidModelConfiguration({
        ...base,
        endpointOverride: "https://gateway.example/v1/images/generations",
        useCustomProtocol: true,
      }),
    ).toBe(true);
  });
});
