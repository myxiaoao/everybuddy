import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppUpdater } from "./use-app-updater";

const tauriMocks = vi.hoisted(() => ({
  isTauri: vi.fn(),
  getVersion: vi.fn(),
  check: vi.fn(),
  relaunch: vi.fn(),
  reportFrontendError: vi.fn(),
  reportFrontendWarning: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: tauriMocks.getVersion,
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: tauriMocks.isTauri,
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: tauriMocks.relaunch,
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: tauriMocks.check,
}));

vi.mock("@/lib/frontend-logger", () => ({
  reportFrontendError: tauriMocks.reportFrontendError,
  reportFrontendWarning: tauriMocks.reportFrontendWarning,
}));

beforeEach(() => {
  tauriMocks.isTauri.mockReturnValue(true);
  tauriMocks.getVersion.mockResolvedValue("0.1.0-alpha.1");
  tauriMocks.check.mockResolvedValue(null);
  tauriMocks.relaunch.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("useAppUpdater", () => {
  it("reads the runtime version and checks on launch", async () => {
    tauriMocks.getVersion.mockResolvedValue("0.1.1");

    const { result } = renderHook(() => useAppUpdater());

    await waitFor(() => {
      expect(result.current.currentVersion).toBe("0.1.1");
      expect(result.current.updateCheckStatus).toBe("latest");
    });
    expect(tauriMocks.check).toHaveBeenCalledOnce();
  });

  it("downloads an available update and relaunches", async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    tauriMocks.check.mockResolvedValue({
      version: "0.2.0",
      downloadAndInstall,
    });

    const { result } = renderHook(() => useAppUpdater());

    await waitFor(() => {
      expect(result.current.availableUpdate?.version).toBe("0.2.0");
      expect(result.current.updateCheckStatus).toBe("available");
    });
    await act(async () => result.current.installUpdate());

    expect(downloadAndInstall).toHaveBeenCalledOnce();
    expect(tauriMocks.relaunch).toHaveBeenCalledOnce();
  });

  it("keeps manual update checks inside the desktop app", async () => {
    tauriMocks.isTauri.mockReturnValue(false);

    const { result } = renderHook(() => useAppUpdater());
    await act(async () => result.current.checkForUpdates());

    expect(result.current.updateCheckStatus).toBe("desktop-required");
    expect(tauriMocks.check).not.toHaveBeenCalled();
  });

  it("reports update check failures", async () => {
    tauriMocks.check.mockRejectedValue(new Error("offline"));

    const { result } = renderHook(() => useAppUpdater());

    await waitFor(() => expect(result.current.updateCheckStatus).toBe("error"));
    expect(tauriMocks.reportFrontendWarning).toHaveBeenCalledWith(
      "updater.check",
      expect.any(Error),
    );
  });
});
