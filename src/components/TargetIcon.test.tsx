import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { TargetIcon } from "./TargetIcon";

afterEach(() => cleanup());

describe("TargetIcon", () => {
  it.each(["workbuddy", "codebuddy"] as const)("renders the official %s product icon", (target) => {
    const { container } = render(<TargetIcon target={target} />);
    const icon = container.querySelector(`[data-target-kind='${target}']`);

    expect(icon?.querySelector("img")).toHaveAttribute("alt", "");
    expect(icon?.querySelector("img")?.getAttribute("src")).toContain(target);
  });
});
