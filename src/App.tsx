import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Cable, LoaderCircle } from "lucide-react";
import "./App.css";
import { api } from "./lib/api";
import { createTranslator } from "./lib/i18n";
import type {
  AppError,
  AppSettings,
  BackupRecord,
  GatewayConnectionState,
  GatewayInput,
  GatewayProfile,
  ManagedModel,
  ManualModelInput,
  ModelUpdateInput,
  PreparePublishRequest,
  PublishPreview,
  PublishResult,
  TargetKind,
  TargetImportReport,
  TargetModelState,
  TargetStatus,
} from "./types";
import { GatewaySidebar } from "./components/GatewaySidebar";
import { ModelList } from "./components/ModelList";
import { InspectorPanel } from "./components/InspectorPanel";
import { CommandBar, type WorkspaceView } from "./components/CommandBar";
import {
  BackupsDialog,
  ConfirmationDialog,
  GatewayDialog,
  ManualModelDialog,
  ProbeDialog,
  PublishDialog,
  SettingsDialog,
} from "./components/Dialogs";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ImportNotice } from "./components/ImportNotice";
import { deriveModelSelection } from "./lib/model-selection";
import { useAppUpdater } from "./hooks/use-app-updater";

function App() {
  const [loading, setLoading] = useState(true);
  const [gateways, setGateways] = useState<GatewayProfile[]>([]);
  const [models, setModels] = useState<ManagedModel[]>([]);
  const [targets, setTargets] = useState<TargetStatus[]>([]);
  const [targetModelStates, setTargetModelStates] = useState<TargetModelState[]>([]);
  const [gatewayConnectionStates, setGatewayConnectionStates] = useState<Record<string, GatewayConnectionState>>({});
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [selectedGatewayId, setSelectedGatewayId] = useState<string | null>(null);
  const [selectionOverrides, setSelectionOverrides] = useState<Map<string, boolean>>(() => new Map());
  const [activeKey, setActiveKey] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [busyGatewayId, setBusyGatewayId] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [error, setError] = useState<AppError | null>(null);
  const [gatewayDialog, setGatewayDialog] = useState(false);
  const [manualModelDialog, setManualModelDialog] = useState(false);
  const [editingGateway, setEditingGateway] = useState<GatewayProfile | null>(null);
  const [editingGatewayToken, setEditingGatewayToken] = useState("");
  const [probeDialog, setProbeDialog] = useState(false);
  const [publishDialog, setPublishDialog] = useState(false);
  const [publishPreview, setPublishPreview] = useState<PublishPreview | null>(null);
  const [publishResult, setPublishResult] = useState<PublishResult | null>(null);
  const [publishRequest, setPublishRequest] = useState<PreparePublishRequest | null>(null);
  const [settingsDialog, setSettingsDialog] = useState(false);
  const [backupsDialog, setBackupsDialog] = useState(false);
  const [backups, setBackups] = useState<BackupRecord[]>([]);
  const [gatewayToDelete, setGatewayToDelete] = useState<GatewayProfile | null>(null);
  const [backupToRestore, setBackupToRestore] = useState<BackupRecord | null>(null);
  const [discardDialog, setDiscardDialog] = useState(false);
  const [dirtyModelKey, setDirtyModelKey] = useState<string | null>(null);
  const [inspectorRevision, setInspectorRevision] = useState(0);
  const [compactView, setCompactView] = useState<WorkspaceView>("gateways");
  const [importReport, setImportReport] = useState<TargetImportReport | null>(null);
  const [importDetailsExpanded, setImportDetailsExpanded] = useState(false);
  const pendingActionRef = useRef<(() => void | Promise<void>) | null>(null);
  const { availableUpdate, installingUpdate, installUpdate } = useAppUpdater();
  const handleDirtyChange = useCallback((modelKey: string | null, changed: boolean) => {
    setDirtyModelKey(changed ? modelKey : null);
  }, []);

  const t = useMemo(() => createTranslator(settings.language), [settings.language]);
  const selectedGateway = gateways.find((gateway) => gateway.id === selectedGatewayId) ?? null;
  const gatewayModels = useMemo(
    () => models.filter((model) => model.gatewayId === selectedGatewayId),
    [models, selectedGatewayId],
  );
  const filteredModels = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return gatewayModels;
    return gatewayModels.filter((model) =>
      `${model.name} ${model.id} ${model.vendor}`.toLocaleLowerCase().includes(needle),
    );
  }, [gatewayModels, query]);
  const activeModel = models.find((model) => model.key === activeKey) ?? null;
  const publishableTargetKinds = useMemo(
    () => targets.filter(isTargetPublishable).map((target) => target.kind),
    [targets],
  );
  const selectedTargets = settings.selectedTargets.filter((target) => publishableTargetKinds.includes(target));
  const modelSelection = useMemo(
    () => deriveModelSelection(models, targetModelStates, selectedTargets, selectionOverrides),
    [models, selectionOverrides, selectedTargets, targetModelStates],
  );
  const selectedKeys = modelSelection.checkedKeys;
  const selectedModelCount = [...selectedKeys].filter((key) =>
    gatewayModels.some((model) => model.key === key),
  ).length;

  const loadTargets = useCallback(async (settingsSnapshot: AppSettings = settings) => {
    try {
      const [nextTargets, nextModelStates] = await Promise.all([
        api.getTargetStatuses(),
        api.getTargetModelStates(),
      ]);
      setTargets(nextTargets);
      setTargetModelStates(nextModelStates);
      const available = nextTargets.filter(isTargetPublishable).map((target) => target.kind);
      const nextSelectedTargets = settingsSnapshot.selectedTargets.filter((target) => available.includes(target));
      if (!sameTargets(settingsSnapshot.selectedTargets, nextSelectedTargets)) {
        const nextSettings = { ...settingsSnapshot, selectedTargets: nextSelectedTargets };
        setSettings(nextSettings);
        await api.saveSettings(nextSettings);
      }
      return nextModelStates;
    } catch {
      // Polling failures stay quiet; the next direct action reports the error.
      return null;
    }
  }, [settings]);

  useEffect(() => {
    void (async () => {
      try {
        const data = await api.bootstrap();
        const availableTargets = data.targets.filter(isTargetPublishable).map((target) => target.kind);
        const selectedTargets = data.settings.selectedTargets.length
          ? data.settings.selectedTargets.filter((target) => availableTargets.includes(target))
          : availableTargets;
        setGateways(data.gateways);
        setGatewayConnectionStates(Object.fromEntries(
          data.gateways.map((gateway) => [gateway.id, "idle" as const]),
        ));
        setModels(data.models);
        setTargets(data.targets);
        setTargetModelStates(data.targetModelStates);
        setImportReport(
          data.importReport.importedGatewayCount > 0
            || data.importReport.importedModelCount > 0
            || data.importReport.issues.length > 0
            ? data.importReport
            : null,
        );
        if (data.importReport.importedGatewayCount > 0 || data.importReport.importedModelCount > 0) {
          const bootstrapT = createTranslator(data.settings.language);
          setMessage(bootstrapT("importSucceeded", {
            gateways: data.importReport.importedGatewayCount,
            models: data.importReport.importedModelCount,
          }));
        }
        const nextSettings = { ...data.settings, selectedTargets };
        setSettings(nextSettings);
        if (!sameTargets(data.settings.selectedTargets, selectedTargets)) {
          await api.saveSettings(nextSettings);
        }
        const firstGateway = data.gateways[0] ?? null;
        setSelectedGatewayId(firstGateway?.id ?? null);
        const firstModel = data.models.find((model) => model.gatewayId === firstGateway?.id) ?? null;
        setActiveKey(firstModel?.key ?? null);
        if (firstGateway) setCompactView("models");
      } catch (caught) {
        setError(asAppError(caught));
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  useEffect(() => {
    const interval = window.setInterval(() => void loadTargets(), 5_000);
    return () => window.clearInterval(interval);
  }, [loadTargets]);

  useEffect(() => {
    const root = document.documentElement;
    root.lang = settings.language === "zh-CN" ? "zh-CN" : "en";
    root.dataset.theme = settings.theme;
  }, [settings.language, settings.theme]);

  useEffect(() => {
    if (!dirtyModelKey) return;
    const preventUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", preventUnload);
    return () => window.removeEventListener("beforeunload", preventUnload);
  }, [dirtyModelKey]);

  function showError(caught: unknown) {
    const nextError = asAppError(caught);
    setError(nextError);
    setMessage("");
  }

  function runAfterDiscard(action: () => void | Promise<void>) {
    if (!dirtyModelKey) {
      void action();
      return;
    }
    pendingActionRef.current = action;
    setDiscardDialog(true);
  }

  function discardChanges() {
    const action = pendingActionRef.current;
    pendingActionRef.current = null;
    setDiscardDialog(false);
    setDirtyModelKey(null);
    setInspectorRevision((current) => current + 1);
    if (action) void action();
  }

  function cancelDiscard() {
    pendingActionRef.current = null;
    setDiscardDialog(false);
  }

  function selectGateway(id: string) {
    if (id === selectedGatewayId) return;
    runAfterDiscard(() => {
      setSelectedGatewayId(id);
      setQuery("");
      const firstModel = models.find((model) => model.gatewayId === id);
      setActiveKey(firstModel?.key ?? null);
      setCompactView("models");
    });
  }

  function openAddGateway() {
    runAfterDiscard(() => {
      setEditingGateway(null);
      setEditingGatewayToken("");
      setGatewayDialog(true);
    });
  }

  async function openEditGateway(gateway: GatewayProfile) {
    setBusy(true);
    setError(null);
    try {
      const token = await api.getGatewayToken(gateway.id);
      setEditingGateway(gateway);
      setEditingGatewayToken(token);
      setGatewayDialog(true);
    } catch (caught) {
      showError(caught);
    } finally {
      setBusy(false);
    }
  }

  function closeGatewayDialog() {
    setGatewayDialog(false);
    setEditingGateway(null);
    setEditingGatewayToken("");
  }

  async function saveGateway(input: GatewayInput) {
    setBusy(true);
    setError(null);
    try {
      const profile = await api.saveGateway(input);
      setGateways((current) => {
        const exists = current.some((gateway) => gateway.id === profile.id);
        return exists
          ? current.map((gateway) => (gateway.id === profile.id ? profile : gateway))
          : [...current, profile].sort((a, b) => a.name.localeCompare(b.name));
      });
      setGatewayConnectionStates((current) => ({ ...current, [profile.id]: "idle" }));
      setSelectedGatewayId(profile.id);
      closeGatewayDialog();
      await refreshModels(profile.id);
      setCompactView("models");
    } catch (caught) {
      showError(caught);
    } finally {
      setBusy(false);
    }
  }

  async function refreshModels(gatewayId: string) {
    setBusyGatewayId(gatewayId);
    setGatewayConnectionStates((current) => ({ ...current, [gatewayId]: "refreshing" }));
    setError(null);
    try {
      const discovered = await api.discoverModels(gatewayId);
      setModels((current) => [
        ...current.filter((model) => model.gatewayId !== gatewayId),
        ...discovered,
      ]);
      setActiveKey(discovered[0]?.key ?? null);
      setGatewayConnectionStates((current) => ({ ...current, [gatewayId]: "connected" }));
      setMessage(t("modelCount", { count: discovered.length }));
    } catch (caught) {
      setGatewayConnectionStates((current) => ({ ...current, [gatewayId]: "error" }));
      showError(caught);
      throw caught;
    } finally {
      setBusyGatewayId(null);
    }
  }

  function requestRefreshModels(gatewayId: string) {
    runAfterDiscard(() => refreshModels(gatewayId).catch(() => undefined));
  }

  async function saveManualModel(input: ManualModelInput) {
    setBusy(true);
    setError(null);
    try {
      const model = await api.addManualModel(input);
      setModels((current) => [...current, model].sort((left, right) => left.name.localeCompare(right.name)));
      setActiveKey(model.key);
      setManualModelDialog(false);
      setCompactView("details");
      setMessage(t("manualModelAdded", { name: model.name }));
    } catch (caught) {
      showError(caught);
    } finally {
      setBusy(false);
    }
  }

  function requestRemoveGateway(gateway: GatewayProfile) {
    runAfterDiscard(() => setGatewayToDelete(gateway));
  }

  async function removeGateway(gateway: GatewayProfile) {
    setBusy(true);
    try {
      await api.deleteGateway(gateway.id);
      const remaining = gateways.filter((item) => item.id !== gateway.id);
      setGateways(remaining);
      setModels((current) => current.filter((model) => model.gatewayId !== gateway.id));
      setSelectedGatewayId(remaining[0]?.id ?? null);
      setSelectionOverrides((current) => {
        const next = new Map(current);
        for (const model of models.filter((item) => item.gatewayId === gateway.id)) next.delete(model.key);
        return next;
      });
      setActiveKey(null);
      setGatewayConnectionStates((current) => {
        const next = { ...current };
        delete next[gateway.id];
        return next;
      });
      setGatewayToDelete(null);
      setMessage(t("gatewayRemoved", { name: gateway.name }));
    } catch (caught) {
      showError(caught);
    } finally {
      setBusy(false);
    }
  }

  function toggleAll(visibleModels: ManagedModel[]) {
    const allSelected = visibleModels.every((model) => selectedKeys.has(model.key));
    setSelectionOverrides((current) => {
      const next = new Map(current);
      for (const model of visibleModels) next.set(model.key, !allSelected);
      return next;
    });
  }

  function clearSelection() {
    setSelectionOverrides((current) => {
      const next = new Map(current);
      for (const model of gatewayModels) next.set(model.key, false);
      return next;
    });
  }

  function activateModel(key: string) {
    if (key === activeKey) {
      setCompactView("details");
      return;
    }
    runAfterDiscard(() => {
      setActiveKey(key);
      setCompactView("details");
    });
  }

  function openManualModel() {
    runAfterDiscard(() => setManualModelDialog(true));
  }

  function openProbe() {
    runAfterDiscard(() => setProbeDialog(true));
  }

  function toggleModel(key: string) {
    setSelectionOverrides((current) => {
      const next = new Map(current);
      next.set(key, !selectedKeys.has(key));
      return next;
    });
  }

  function clearSelectionOverrides(keys: Iterable<string>) {
    setSelectionOverrides((current) => {
      const next = new Map(current);
      for (const key of keys) next.delete(key);
      return next;
    });
  }

  async function saveModel(input: ModelUpdateInput) {
    setBusy(true);
    try {
      const updated = await api.updateModel(input);
      replaceModel(updated);
      setDirtyModelKey(null);
      setMessage(t("modelConfigSaved"));
    } catch (caught) {
      showError(caught);
    } finally {
      setBusy(false);
    }
  }

  async function runProbe() {
    if (!activeModel) return;
    setBusy(true);
    try {
      const summary = await api.probeModel(activeModel.key);
      replaceModel(summary.model);
      setProbeDialog(false);
      setMessage(summary.notes.length ? summary.notes.join(" ") : t("probe"));
    } catch (caught) {
      showError(caught);
    } finally {
      setBusy(false);
    }
  }

  function replaceModel(updated: ManagedModel) {
    setModels((current) => current.map((model) => (model.key === updated.key ? updated : model)));
  }

  async function toggleTarget(target: TargetKind) {
    if (!publishableTargetKinds.includes(target)) return;
    const selectedTargets = settings.selectedTargets.includes(target)
      ? settings.selectedTargets.filter((kind) => kind !== target)
      : [...settings.selectedTargets, target];
    const next = { ...settings, selectedTargets };
    setSettings(next);
    try {
      await api.saveSettings(next);
    } catch (caught) {
      setSettings(settings);
      showError(caught);
    }
  }

  async function previewPublish() {
    if (!selectedGateway) return;
    const request: PreparePublishRequest = {
      gatewayId: selectedGateway.id,
      modelIds: gatewayModels.filter((model) => selectedKeys.has(model.key)).map((model) => model.id),
      targets: selectedTargets,
    };
    setPublishRequest(request);
    setPublishPreview(null);
    setPublishResult(null);
    setPublishDialog(true);
    setBusy(true);
    try {
      setPublishPreview(await api.preparePublish(request));
    } catch (caught) {
      setPublishDialog(false);
      showError(caught);
    } finally {
      setBusy(false);
    }
  }

  async function executePublish(acceptConflicts: boolean) {
    if (!publishRequest || !publishPreview) return;
    setBusy(true);
    try {
      const result = await api.executePublish(publishRequest, publishPreview, acceptConflicts);
      setPublishResult(result);
      setMessage(result.success ? t("published") : t("publishFailed"));
      await loadTargets();
      if (result.success) {
        clearSelectionOverrides(
          publishRequest.modelIds.map((id) => `${publishRequest.gatewayId}::${id}`),
        );
      }
    } catch (caught) {
      showError(caught);
    } finally {
      setBusy(false);
    }
  }

  async function saveSettings(next: AppSettings) {
    setBusy(true);
    try {
      const saved = await api.saveSettings(next);
      setSettings(saved);
      setSettingsDialog(false);
      await loadTargets(saved);
    } catch (caught) {
      showError(caught);
    } finally {
      setBusy(false);
    }
  }

  async function openBackups() {
    setBackupsDialog(true);
    setBusy(true);
    try {
      setBackups(await api.listBackups());
    } catch (caught) {
      showError(caught);
    } finally {
      setBusy(false);
    }
  }

  async function restoreBackup(backup: BackupRecord) {
    setBusy(true);
    try {
      await api.restoreBackup(backup.id);
      setMessage(t("restore"));
      setBackups(await api.listBackups());
      const previousKeys = targetModelStates.find((state) => state.target === backup.target)?.matchedModelKeys ?? [];
      const nextStates = await loadTargets();
      const restoredKeys = nextStates?.find((state) => state.target === backup.target)?.matchedModelKeys ?? [];
      clearSelectionOverrides(new Set([...previousKeys, ...restoredKeys]));
      setBackupToRestore(null);
    } catch (caught) {
      showError(caught);
    } finally {
      setBusy(false);
    }
  }

  async function requestInstallUpdate() {
    try {
      await installUpdate();
    } catch (caught) {
      showError(caught);
    }
  }

  if (loading) {
    return <main className="startup-state"><LoaderCircle className="spin" aria-hidden="true" /><p>{t("loading")}</p></main>;
  }

  if (error?.code === "DESKTOP_REQUIRED") {
    return <main className="startup-state"><Cable aria-hidden="true" /><h1>{t("appName")}</h1><p>{t("desktopRequired")}</p></main>;
  }

  const hasGateway = gateways.length > 0;
  const selectedGatewayRefreshing = busyGatewayId === selectedGatewayId;
  const localizedErrorMessage = error ? localizedError(error, t) : null;
  const locale = settings.language === "zh-CN" ? "zh-CN" : "en-US";

  return (
    <TooltipProvider delayDuration={350}>
    <div className={`app-shell compact-view-${compactView}`}>
      <a className="skip-link" href="#workspace">{t("skipWorkspace")}</a>
      <CommandBar
        gateway={selectedGateway}
        modelCount={gatewayModels.length}
        selectedModelCount={selectedModelCount}
        selectedTargetCount={selectedTargets.length}
        view={compactView}
        refreshing={selectedGatewayRefreshing}
        busy={busy}
        t={t}
        onNavigate={setCompactView}
        onBack={() => setCompactView(compactView === "details" ? "models" : "gateways")}
        onRefresh={() => selectedGatewayId && requestRefreshModels(selectedGatewayId)}
        onPublish={() => void previewPublish()}
      />

      <main id="workspace" className="workspace">
        <GatewaySidebar
          gateways={gateways}
          selectedId={selectedGatewayId}
          busyId={busyGatewayId}
          connectionStates={gatewayConnectionStates}
          t={t}
          onSelect={selectGateway}
          onAdd={openAddGateway}
          onEdit={(gateway) => void openEditGateway(gateway)}
          onRefresh={requestRefreshModels}
          onDelete={requestRemoveGateway}
          onOpenSettings={() => setSettingsDialog(true)}
          onOpenBackups={() => void openBackups()}
        />

        {hasGateway ? (
          <ModelList
            key={selectedGatewayId ?? "no-gateway"}
            models={filteredModels}
            totalModelCount={gatewayModels.length}
            query={query}
            selectedKeys={selectedKeys}
            indeterminateKeys={modelSelection.indeterminateKeys}
            presentTargetsByKey={modelSelection.presentTargetsByKey}
            selectedCount={selectedModelCount}
            activeKey={activeKey}
            disabled={selectedGatewayRefreshing}
            t={t}
            onQueryChange={setQuery}
            onToggleAll={toggleAll}
            onToggle={toggleModel}
            onClearSelection={clearSelection}
            onAddManual={openManualModel}
            onActivate={activateModel}
          />
        ) : (
          <section className="model-panel onboarding-state">
            <div className="onboarding-rail" aria-hidden="true"><span>API</span><span>models</span><span>targets</span></div>
            <div><h1>{t("noGatewayTitle")}</h1><p>{t("noGatewayBody")}</p><Button type="button" onClick={openAddGateway}><Cable aria-hidden="true" size={17} />{t("addGateway")}</Button></div>
          </section>
        )}

        <InspectorPanel
          key={`${activeModel?.key ?? "none"}-${activeModel?.updatedAt ?? "none"}-${inspectorRevision}`}
          model={activeModel}
          selectedCount={selectedModelCount}
          targets={targets}
          selectedTargets={selectedTargets}
          busy={busy || selectedGatewayRefreshing}
          t={t}
          onSaveModel={(input) => void saveModel(input)}
          onProbe={openProbe}
          onToggleTarget={(target) => void toggleTarget(target)}
          onDirtyChange={handleDirtyChange}
        />
      </main>

      <div className="live-region" role="status" aria-live="polite">{message}</div>
      {localizedErrorMessage && error?.code !== "DESKTOP_REQUIRED" ? (
        <div className="error-toast" role="alert">
          <strong>{localizedErrorMessage.title}</strong>
          <span>{localizedErrorMessage.message}</span>
          <small>{localizedErrorMessage.recovery}</small>
          <Button variant="ghost" type="button" onClick={() => setError(null)}>{t("close")}</Button>
        </div>
      ) : null}
      {availableUpdate ? (
        <div className="update-banner" role="status">
          <span>{t("updateAvailable", { version: availableUpdate.version })}</span>
          <Button size="sm" type="button" onClick={() => void requestInstallUpdate()} disabled={installingUpdate}>
            {installingUpdate ? <LoaderCircle className="spin" aria-hidden="true" size={16} /> : null}
            {t("updateAndRestart")}
          </Button>
        </div>
      ) : null}
      {importReport ? (
        <ImportNotice
          report={importReport}
          expanded={importDetailsExpanded}
          t={t}
          onToggle={() => setImportDetailsExpanded((current) => !current)}
          onClose={() => setImportReport(null)}
        />
      ) : null}

      {gatewayDialog ? <GatewayDialog open busy={busy} gateway={editingGateway} initialToken={editingGatewayToken} t={t} onClose={closeGatewayDialog} onSubmit={(input) => void saveGateway(input)} /> : null}
      {manualModelDialog && selectedGateway ? <ManualModelDialog open busy={busy} gateway={selectedGateway} t={t} onClose={() => setManualModelDialog(false)} onSubmit={(input) => void saveManualModel(input)} /> : null}
      {probeDialog ? <ProbeDialog open busy={busy} t={t} onClose={() => setProbeDialog(false)} onConfirm={() => void runProbe()} /> : null}
      {publishDialog ? <PublishDialog key={publishPreview ? JSON.stringify(publishPreview.targets) : "loading"} open busy={busy} preview={publishPreview} result={publishResult} t={t} onClose={() => setPublishDialog(false)} onConfirm={(accepted) => void executePublish(accepted)} /> : null}
      {settingsDialog ? <SettingsDialog open busy={busy} settings={settings} t={t} onClose={() => setSettingsDialog(false)} onSubmit={(next) => void saveSettings(next)} /> : null}
      {backupsDialog ? <BackupsDialog open busy={busy} backups={backups} locale={locale} t={t} onClose={() => setBackupsDialog(false)} onRestore={setBackupToRestore} /> : null}
      {gatewayToDelete ? (
        <ConfirmationDialog
          open
          busy={busy}
          destructive
          title={t("deleteGatewayTitle")}
          description={t("deleteGatewayConfirm", { name: gatewayToDelete.name })}
          confirmLabel={t("deleteGatewayAction")}
          t={t}
          onClose={() => setGatewayToDelete(null)}
          onConfirm={() => void removeGateway(gatewayToDelete)}
        />
      ) : null}
      {backupToRestore ? (
        <ConfirmationDialog
          open
          busy={busy}
          title={t("restoreBackupTitle")}
          description={t("restoreConfirm", {
            target: displayTarget(backupToRestore.target),
            date: new Date(backupToRestore.createdAt).toLocaleString(locale),
          })}
          confirmLabel={t("restoreBackupAction")}
          t={t}
          onClose={() => setBackupToRestore(null)}
          onConfirm={() => void restoreBackup(backupToRestore)}
        />
      ) : null}
      {discardDialog ? (
        <ConfirmationDialog
          open
          busy={false}
          destructive
          title={t("discardChangesTitle")}
          description={t("discardChangesBody", { name: activeModel?.name ?? t("unknown") })}
          confirmLabel={t("discardChangesAction")}
          t={t}
          onClose={cancelDiscard}
          onConfirm={discardChanges}
        />
      ) : null}
    </div>
    </TooltipProvider>
  );
}

function asAppError(error: unknown): AppError {
  if (typeof error === "object" && error !== null && "code" in error && "message" in error) {
    return { code: String(error.code), message: String(error.message) };
  }
  return { code: "UNEXPECTED_ERROR", message: error instanceof Error ? error.message : String(error) };
}

function isTargetPublishable(target: TargetStatus) {
  return target.installed && target.writable && target.schema !== "invalid";
}

function sameTargets(left: TargetKind[], right: TargetKind[]) {
  return left.length === right.length && left.every((target) => right.includes(target));
}

function localizedError(error: AppError, t: ReturnType<typeof createTranslator>) {
  switch (error.code) {
    case "AUTHENTICATION_ERROR":
      return { title: t("errorAuthenticationTitle"), message: t("errorAuthenticationMessage"), recovery: t("errorAuthenticationRecovery") };
    case "NETWORK_ERROR":
      return { title: t("errorNetworkTitle"), message: t("errorNetworkMessage"), recovery: t("errorNetworkRecovery") };
    case "PROTOCOL_ERROR":
      return { title: t("errorProtocolTitle"), message: t("errorProtocolMessage"), recovery: t("errorProtocolRecovery") };
    case "TARGET_ERROR":
      return { title: t("errorTargetTitle"), message: t("errorTargetMessage"), recovery: t("errorTargetRecovery") };
    case "DRIFT_ERROR":
      return { title: t("errorDriftTitle"), message: t("errorDriftMessage"), recovery: t("errorDriftRecovery") };
    case "CONFLICT_ERROR":
      return { title: t("errorConflictTitle"), message: t("errorConflictMessage"), recovery: t("errorConflictRecovery") };
    case "SECRET_STORE_ERROR":
      return { title: t("errorSecretTitle"), message: t("errorSecretMessage"), recovery: t("errorSecretRecovery") };
    case "STORAGE_ERROR":
      return { title: t("errorStorageTitle"), message: t("errorStorageMessage"), recovery: t("errorStorageRecovery") };
    case "VALIDATION_ERROR":
    case "VALIDATION":
      return { title: t("errorValidationTitle"), message: t("errorValidationMessage"), recovery: t("errorValidationRecovery") };
    default:
      return { title: t("connectionError"), message: t("errorUnexpectedMessage"), recovery: t("errorUnexpectedRecovery") };
  }
}

function displayTarget(target: TargetKind) {
  return target === "workbuddy" ? "WorkBuddy" : "CodeBuddy";
}

const defaultSettings: AppSettings = {
  language: "zh-CN",
  theme: "system",
  selectedTargets: [],
  targetPaths: {
    workbuddy: "~/.workbuddy/models.json",
    codebuddy: "~/.codebuddy/models.json",
  },
};

export default App;
