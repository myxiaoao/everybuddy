import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const entryCss = readFileSync(resolve(process.cwd(), "src/App.css"), "utf8");
const css = [...entryCss.matchAll(/@import "(\.\/styles\/[^"]+)";/g)]
  .map(([, stylesheet]) => readFileSync(resolve(process.cwd(), "src", stylesheet), "utf8"))
  .join("\n");

function themeBlock(selector: string) {
  const start = css.indexOf(`${selector} {`);
  const end = css.indexOf("\n}", start);

  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);

  return css.slice(start, end);
}

describe("ChatGPT surface mapping", () => {
  it("keeps the light main canvas separate from the sidebar surface", () => {
    const light = themeBlock(":root");

    expect(light).toContain("--background: oklch(1 0 0);");
    expect(light).toContain("--color-bg-sidebar: oklch(0.982118 0 0);");
    expect(light).toContain("--secondary: oklch(0.967153 0 0);");
    expect(light).toContain("--accent: oklch(0.943089 0 0);");
  });

  it.each([':root[data-theme="dark"]', ':root[data-theme="system"]'])(
    "keeps the dark main canvas separate from the sidebar surface in %s",
    (selector) => {
      const dark = themeBlock(selector);

      expect(dark).toContain("--background: oklch(0.247759 0 0);");
      expect(dark).toContain("--color-bg-sidebar: oklch(0.204627 0 0);");
      expect(dark).toContain("--secondary: oklch(0.305191 0 0);");
      expect(dark).toContain("--accent: oklch(0.309186 0 0);");
    },
  );

  it("uses the selected surface for the active gateway", () => {
    expect(css).toMatch(/\.gateway-item\.is-selected \{[^}]*background: var\(--color-bg-selected\);/s);
  });
});
