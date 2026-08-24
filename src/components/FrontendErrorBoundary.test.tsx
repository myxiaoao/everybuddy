import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FrontendErrorBoundary } from "./FrontendErrorBoundary";

const { reportFrontendError } = vi.hoisted(() => ({
  reportFrontendError: vi.fn(),
}));

vi.mock("@/lib/frontend-logger", () => ({ reportFrontendError }));

function BrokenView(): never {
  throw new Error("render failed");
}

describe("FrontendErrorBoundary", () => {
  beforeEach(() => {
    reportFrontendError.mockClear();
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    document.documentElement.lang = "zh-CN";
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("replaces a render crash with a recovery action and reports it", () => {
    render(
      <FrontendErrorBoundary>
        <BrokenView />
      </FrontendErrorBoundary>,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("界面无法继续显示");
    expect(
      screen.getByRole("button", { name: "重新加载界面" }),
    ).toBeInTheDocument();
    expect(reportFrontendError).toHaveBeenCalledWith(
      "react.error-boundary",
      expect.any(Error),
      expect.any(String),
    );
  });
});
