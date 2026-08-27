import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const shellCss = readFileSync(
  resolve(process.cwd(), "src/styles/shell.css"),
  "utf8",
);
const dialogsCss = readFileSync(
  resolve(process.cwd(), "src/styles/dialogs.css"),
  "utf8",
);
const workspaceCss = readFileSync(
  resolve(process.cwd(), "src/styles/workspace.css"),
  "utf8",
);
const tokensCss = readFileSync(
  resolve(process.cwd(), "src/styles/tokens.css"),
  "utf8",
);

function rule(css: string, selector: string) {
  const match = css.match(new RegExp(`${selector} \\{([^}]*)\\}`, "s"));

  expect(match).not.toBeNull();
  return match?.[1] ?? "";
}

describe("workspace leading edge", () => {
  it("aligns desktop chrome, import feedback, and sidebar content", () => {
    expect(tokensCss).toContain("--space-2: 8px;");
    expect(tokensCss).toContain("--space-4: 16px;");
    expect(rule(shellCss, "\\.command-bar")).toContain(
      "padding-inline-start: var(--space-2);",
    );
    expect(rule(shellCss, "\\.command-stage")).toContain(
      "padding: 4px var(--space-2);",
    );
    expect(rule(dialogsCss, "\\.import-notice")).toContain(
      "padding: 10px var(--space-4);",
    );
    expect(rule(shellCss, "\\.gateway-panel")).toContain(
      "padding: var(--space-4) var(--space-4) var(--space-3);",
    );
  });
});

describe("target icon geometry", () => {
  it("presents both target brands with the same circular silhouette", () => {
    expect(workspaceCss).toMatch(
      /\.target-option__icon \{[^}]*border-radius: 50%;/s,
    );
    expect(
      rule(
        workspaceCss,
        '\\.target-option__icon\\[data-target-kind="workbuddy"\\] img',
      ),
    ).toContain("transform: scale(1.23);");
    expect(workspaceCss).not.toContain(
      '.target-option__icon[data-target-kind="codebuddy"] img',
    );
  });
});
