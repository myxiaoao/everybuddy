import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { createTranslator } from "../lib/i18n";
import type { PublishResult } from "../types";
import { PublishDialog } from "./Dialogs";

afterEach(() => cleanup());

describe("PublishDialog", () => {
  it("distinguishes rolled back and failed targets", () => {
    const result: PublishResult = {
      success: false,
      results: [
        { target: "workbuddy", success: false, rollbackAttempted: true, rolledBack: true, message: "Published changes were rolled back" },
        { target: "codebuddy", success: false, rollbackAttempted: false, rolledBack: false, message: "Write failed" },
      ],
    };

    render(
      <PublishDialog
        open
        busy={false}
        preview={null}
        result={result}
        t={createTranslator("zh-CN")}
        onClose={() => undefined}
        onConfirm={() => undefined}
      />,
    );

    expect(screen.getByText("其他目标发布失败，已恢复发布前配置。").closest(".is-rolled-back")).not.toBeNull();
    expect(screen.getByText("写入失败，请检查目标路径和权限后重试。").closest(".is-failure")).not.toBeNull();
  });

  it("distinguishes rollback failures from the target write failure", () => {
    const result: PublishResult = {
      success: false,
      results: [
        { target: "workbuddy", success: false, rollbackAttempted: true, rolledBack: false, message: "Rollback failed" },
      ],
    };

    render(
      <PublishDialog
        open
        busy={false}
        preview={null}
        result={result}
        t={createTranslator("zh-CN")}
        onClose={() => undefined}
        onConfirm={() => undefined}
      />,
    );

    expect(screen.getByText("发布和回滚均失败，请立即检查目标配置文件。").closest(".is-rollback-failed")).not.toBeNull();
  });
});
