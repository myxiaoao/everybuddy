import { useCallback, useEffect, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";

export function useAppUpdater() {
  const [availableUpdate, setAvailableUpdate] = useState<Awaited<ReturnType<typeof check>>>(null);
  const [installingUpdate, setInstallingUpdate] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    void check()
      .then(setAvailableUpdate)
      .catch(() => undefined);
  }, []);

  const installUpdate = useCallback(async () => {
    if (!availableUpdate) return;
    setInstallingUpdate(true);
    try {
      await availableUpdate.downloadAndInstall();
      await relaunch();
    } catch (error) {
      setInstallingUpdate(false);
      throw error;
    }
  }, [availableUpdate]);

  return { availableUpdate, installingUpdate, installUpdate };
}
