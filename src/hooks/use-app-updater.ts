import { useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import packageJson from "../../package.json";
import {
  reportFrontendError,
  reportFrontendWarning,
} from "@/lib/frontend-logger";

export type UpdateCheckStatus =
  "idle" | "checking" | "latest" | "available" | "error" | "desktop-required";

export function useAppUpdater() {
  const [currentVersion, setCurrentVersion] = useState(packageJson.version);
  const [availableUpdate, setAvailableUpdate] =
    useState<Awaited<ReturnType<typeof check>>>(null);
  const [updateCheckStatus, setUpdateCheckStatus] = useState<UpdateCheckStatus>(
    () => (isTauri() ? "checking" : "idle"),
  );
  const [installingUpdate, setInstallingUpdate] = useState(false);
  const [restartRequired, setRestartRequired] = useState(false);
  const installedRef = useRef(false);
  const installationInFlightRef = useRef(false);

  const storeUpdate = useCallback(
    (update: Awaited<ReturnType<typeof check>>) => {
      setAvailableUpdate(update);
      setUpdateCheckStatus(update ? "available" : "latest");
    },
    [],
  );

  const checkForUpdates = useCallback(async () => {
    if (installedRef.current || installationInFlightRef.current) return;
    if (!isTauri()) {
      setUpdateCheckStatus("desktop-required");
      return;
    }

    setUpdateCheckStatus("checking");
    try {
      const update = await check();
      storeUpdate(update);
    } catch (error) {
      setUpdateCheckStatus("error");
      reportFrontendWarning("updater.check", error);
    }
  }, [storeUpdate]);

  useEffect(() => {
    if (!isTauri()) return;
    void getVersion()
      .then(setCurrentVersion)
      .catch((error) => reportFrontendWarning("updater.version", error));
    void check()
      .then(storeUpdate)
      .catch((error) => {
        setUpdateCheckStatus("error");
        reportFrontendWarning("updater.check", error);
      });
  }, [storeUpdate]);

  const installUpdate = useCallback(async () => {
    if (!availableUpdate || installationInFlightRef.current) return;
    installationInFlightRef.current = true;
    setInstallingUpdate(true);
    try {
      if (!installedRef.current) {
        await availableUpdate.downloadAndInstall();
        installedRef.current = true;
        setRestartRequired(true);
      }
      await relaunch();
    } catch (error) {
      installationInFlightRef.current = false;
      setInstallingUpdate(false);
      reportFrontendError(
        installedRef.current ? "updater.restart" : "updater.install",
        error,
      );
      throw error;
    }
  }, [availableUpdate]);

  return {
    currentVersion,
    availableUpdate,
    updateCheckStatus,
    installingUpdate,
    restartRequired,
    checkForUpdates,
    installUpdate,
  };
}
