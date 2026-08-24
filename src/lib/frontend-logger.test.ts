import { describe, expect, it } from "vitest";
import { formatFrontendLog, redactLogText } from "./frontend-logger";

describe("frontend logger", () => {
  it("redacts headers, query values, credentials, and known token shapes", () => {
    const output = redactLogText(
      "https://user:pass@example.dev/v1?token=visible&model=gpt Authorization: Bearer sk-secretvalue123",
    );

    expect(output).not.toContain("user:pass");
    expect(output).not.toContain("visible");
    expect(output).not.toContain("gpt");
    expect(output).not.toContain("sk-secretvalue123");
    expect(output).toContain(REDACTED_MARKER);
  });

  it("redacts nested secret fields and opaque credential values", () => {
    const opaque = "abcDEF0123456789abcDEF0123456789";
    const output = formatFrontendLog("test", {
      request: { apiKey: "secret", credentials: { token: "nested" } },
      value: opaque,
      safe: "model unavailable",
    });

    expect(output).not.toContain("secret");
    expect(output).not.toContain("nested");
    expect(output).not.toContain(opaque);
    expect(output).toContain("model unavailable");
  });

  it("handles circular values without throwing", () => {
    const value: Record<string, unknown> = {};
    value.self = value;

    expect(formatFrontendLog("circular", value)).toContain("[Circular]");
  });
});

const REDACTED_MARKER = "[REDACTED]";
