import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import { Cable, LoaderCircle } from "lucide-react";
import "./App.css";
import { api } from "./lib/api";
import { createTranslator, type MessageKey } from "./lib/i18n";
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
import { ErrorNotice } from "./components/ErrorNotice";
import { useAppUpdater } from "./hooks/use-app-updater";
import {
  reportFrontendError,
  reportFrontendWarning,
} from "./lib/frontend-logger";
import { useModelSelection } from "./hooks/use-model-selection";
import { asAppError, localizedError } from "./lib/app-error";
import {
  defaultSettings,
  displayTarget,
  isTargetPublishable,
} from "./lib/target-utils";
import {
  initialWorkspaceWorkflow,
  isWorkspaceBusy,
  workspaceWorkflowReducer,
} from "./lib/workspace-workflow";

type StatusMessage =
  | { key: MessageKey; values?: Record<string, string | number> }
  | { text: string };

type DialogKind =
  | "gateway"
  | "manualModel"
  | "probe"
  | "publish"
  | "settings"
  | "backups"
  | "removeGateway"
  | "restoreBackup"
  | "discard";

function App() {
  const [loading, setLoading] = useState(true);
  const [gateways, setGateways] = useState<GatewayProfile[]>([]);
  const [models, setModels] = useState<ManagedModel[]>([]);
  const [targets, setTargets] = useState<TargetStatus[]>([]);
  const [targetModelStates, setTargetModelStates] = useState<
    TargetModelState[]
  >([]);
  const [gatewayConnectionStates, setGatewayConnectionStates] = useState<
    Record<string, GatewayConnectionState>
  >({});
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [selectedGatewayId, setSelectedGatewayId] = useState<string | null>(
    null,
  );
  const [activeKey, setActiveKey] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [workflow, dispatchWorkflow] = useReducer(
    workspaceWorkflowReducer,
    initialWorkspaceWorkflow,
  );
  const [refreshingGatewayIds, setRefreshingGatewayIds] = useState<Set<string>>(
    () => new Set(),
  );
  const [statusMessage, setStatusMessage] = useState<StatusMessage | null>(
    null,
  );
  const [error, setError] = useState<AppError | null>(null);
  const [gatewayDialog, setGatewayDialog] = useState(false);
  const [manualModelDialog, setManualModelDialog] = useState(false);
  const [editingGateway, setEditingGateway] = useState<GatewayProfile | null>(
    null,
  );
  const [editingGatewayToken, setEditingGatewayToken] = useState("");
  const [probeDialog, setProbeDialog] = useState(false);
  const [applyingOpenRouter, setApplyingOpenRouter] = useState(false);
  const [openRouterModelMatches, setOpenRouterModelMatches] = useState<
    Record<string, boolean>
  >({});
  const [settingsDialog, setSettingsDialog] = useState(false);
  const [backupsDialog, setBackupsDialog] = useState(false);
  const [backups, setBackups] = useState<BackupRecord[]>([]);
  const [gatewayToDelete, setGatewayToDelete] = useState<GatewayProfile | null>(
    null,
  );
  const [backupToRestore, setBackupToRestore] = useState<BackupRecord | null>(
    null,
  );
  const [inspectorRevision, setInspectorRevision] = useState(0);
  const [compactView, setCompactView] = useState<WorkspaceView>("gateways");
  const [importReport, setImportReport] = useState<TargetImportReport | null>(
    null,
  );
  const [importDetailsExpanded, setImportDetailsExpanded] = useState(false);
  const pendingActionRef = useRef<(() => void | Promise<void>) | null>(null);
  const targetPollErrorLoggedRef = useRef(false);
  const targetRefreshGenerationRef = useRef(0);
  const targetRefreshInFlightRef = useRef<Promise<
    TargetModelState[] | null
  > | null>(null);
  const targetSettingsGenerationRef = useRef(0);
  const publishSessionGenerationRef = useRef(0);
  const selectedGatewayIdRef = useRef<string | null>(null);
  const refreshGenerationsRef = useRef(new Map<string, number>());
  const {
    currentVersion,
    availableUpdate,
    updateCheckStatus,
    installingUpdate,
    checkForUpdates,
    installUpdate,
  } = useAppUpdater();
  const handleDirtyChange = useCallback(
    (modelKey: string | null, changed: boolean) => {
      dispatchWorkflow({
        type: "dirtyChanged",
        modelKey: changed ? modelKey : null,
      });
    },
    [],
  );
  const busy = isWorkspaceBusy(workflow);
  const {
    dirtyModelKey,
    discardOpen: discardDialog,
    publishPhase,
    publishSessionId,
    publishRequest,
    publishPreview,
    publishResult,
  } = workflow;
  const publishDialog = publishPhase !== "closed";

  const t = useMemo(
    () => createTranslator(settings.language),
    [settings.language],
  );
  const selectedGateway =
    gateways.find((gateway) => gateway.id === selectedGatewayId) ?? null;
  const gatewayModels = useMemo(
    () => models.filter((model) => model.gatewayId === selectedGatewayId),
    [models, selectedGatewayId],
  );
  const filteredModels = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return gatewayModels;
    return gatewayModels.filter((model) =>
      `${model.name} ${model.id} ${model.vendor}`
        .toLocaleLowerCase()
        .includes(needle),
    );
  }, [gatewayModels, query]);
  const activeModel = models.find((model) => model.key === activeKey) ?? null;
  useEffect(() => {
    if (!activeModel) return;
    const modelKey = activeModel.key;
    let cancelled = false;
    void api
      .getOpenRouterModelMatch(modelKey)
      .then((match) => {
        if (cancelled) return;
        setOpenRouterModelMatches((current) => ({
          ...current,
          [modelKey]: Boolean(match),
        }));
      })
      .catch(() => {
        if (cancelled) return;
        setOpenRouterModelMatches((current) => ({
          ...current,
          [modelKey]: false,
        }));
      });
    return () => {
      cancelled = true;
    };
  }, [activeModel]);
  const publishableTargetKinds = useMemo(
    () => targets.filter(isTargetPublishable).map((target) => target.kind),
    [targets],
  );
  const selectedTargets = settings.selectedTargets.filter((target) =>
    publishableTargetKinds.includes(target),
  );
  const {
    selection: modelSelection,
    selectedKeys,
    selectedModelCount,
    toggleAll,
    clearSelection,
    toggleModel,
    clearOverrides: clearSelectionOverrides,
  } = useModelSelection({
    models,
    gatewayModels,
    targetModelStates,
    selectedTargets,
  });

  const loadTargets = useCallback((force = false) => {
    if (!force && targetRefreshInFlightRef.current) {
      return targetRefreshInFlightRef.current;
    }
    const generation = ++targetRefreshGenerationRef.current;
    const request = api
      .getTargetSnapshot()
      .then((snapshot) => {
        if (targetRefreshGenerationRef.current !== generation) return null;
        setTargets(snapshot.targets);
        setTargetModelStates(snapshot.targetModelStates);
        targetPollErrorLoggedRef.current = false;
        return snapshot.targetModelStates;
      })
      .catch((caught: unknown) => {
        if (targetRefreshGenerationRef.current !== generation) return null;
        if (!targetPollErrorLoggedRef.current) {
          reportFrontendWarning("target-state.refresh", caught);
          targetPollErrorLoggedRef.current = true;
        }
        return null;
      })
      .finally(() => {
        if (targetRefreshInFlightRef.current === request) {
          targetRefreshInFlightRef.current = null;
        }
      });
    targetRefreshInFlightRef.current = request;
    return request;
  }, []);

  useEffect(() => {
    const targetGeneration = ++targetRefreshGenerationRef.current;
    void (async () => {
      try {
        const data = await api.bootstrap();
        const availableTargets = data.targets
          .filter(isTargetPublishable)
          .map((target) => target.kind);
        const selectedTargets = data.settings.targetSelectionInitialized
          ? data.settings.selectedTargets
          : availableTargets;
        setGateways(data.gateways);
        setGatewayConnectionStates(
          Object.fromEntries(
            data.gateways.map((gateway) => [gateway.id, "idle" as const]),
          ),
        );
        setModels(data.models);
        if (targetRefreshGenerationRef.current === targetGeneration) {
          setTargets(data.targets);
          setTargetModelStates(data.targetModelStates);
        }
        setImportReport(
          data.importReport.importedGatewayCount > 0 ||
            data.importReport.importedModelCount > 0 ||
            data.importReport.issues.length > 0
            ? data.importReport
            : null,
        );
        if (
          data.importReport.importedGatewayCount > 0 ||
          data.importReport.importedModelCount > 0 ||
          data.importReport.issues.length > 0
        ) {
          setStatusMessage({
            key: "importAnnouncement",
            values: {
              gateways: data.importReport.importedGatewayCount,
              models: data.importReport.importedModelCount,
              issues: data.importReport.issues.length,
            },
          });
        }
        const nextSettings = {
          ...data.settings,
          selectedTargets,
          targetSelectionInitialized: true,
        };
        setSettings(nextSettings);
        if (!data.settings.targetSelectionInitialized) {
          await api.saveSettings(nextSettings);
        }
        const firstGateway = data.gateways[0] ?? null;
        setSelectedGatewayId(firstGateway?.id ?? null);
        selectedGatewayIdRef.current = firstGateway?.id ?? null;
        const firstModel =
          data.models.find((model) => model.gatewayId === firstGateway?.id) ??
          null;
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
    reportFrontendError("application.operation", nextError);
    setError(nextError);
    setStatusMessage(null);
  }

  function runAfterDiscard(action: () => void | Promise<void>) {
    if (!dirtyModelKey) {
      void action();
      return;
    }
    pendingActionRef.current = action;
    dispatchWorkflow({ type: "discardRequested" });
  }

  function discardChanges() {
    const action = pendingActionRef.current;
    pendingActionRef.current = null;
    dispatchWorkflow({ type: "discardConfirmed" });
    setInspectorRevision((current) => current + 1);
    if (action) void action();
  }

  function cancelDiscard() {
    pendingActionRef.current = null;
    dispatchWorkflow({ type: "discardCancelled" });
  }

  function selectGateway(id: string) {
    if (id === selectedGatewayId) {
      setCompactView("models");
      return;
    }
    runAfterDiscard(() => {
      setSelectedGatewayId(id);
      selectedGatewayIdRef.current = id;
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
    dispatchWorkflow({ type: "operationStarted" });
    setError(null);
    try {
      const token = (await api.getGatewayToken(gateway.id)) ?? "";
      setEditingGatewayToken(token);
      setEditingGateway(gateway);
      setGatewayDialog(true);
    } catch (caught) {
      showError(caught);
    } finally {
      dispatchWorkflow({ type: "operationFinished" });
    }
  }

  function requestEditGateway(gateway: GatewayProfile) {
    runAfterDiscard(() => openEditGateway(gateway));
  }

  function closeGatewayDialog() {
    setGatewayDialog(false);
    setEditingGateway(null);
    setEditingGatewayToken("");
  }

  async function saveGateway(input: GatewayInput) {
    dispatchWorkflow({ type: "operationStarted" });
    setError(null);
    try {
      const { profile, modelsInvalidated } = await api.saveGateway(input);
      if (modelsInvalidated) {
        const invalidatedKeys = models
          .filter((model) => model.gatewayId === profile.id)
          .map((model) => model.key);
        setModels((current) =>
          current.filter((model) => model.gatewayId !== profile.id),
        );
        clearSelectionOverrides(invalidatedKeys);
        if (selectedGatewayIdRef.current === profile.id) setActiveKey(null);
      }
      setGateways((current) => {
        const exists = current.some((gateway) => gateway.id === profile.id);
        return exists
          ? current.map((gateway) =>
              gateway.id === profile.id ? profile : gateway,
            )
          : [...current, profile].sort((a, b) => a.name.localeCompare(b.name));
      });
      setGatewayConnectionStates((current) => ({
        ...current,
        [profile.id]: "idle",
      }));
      setSelectedGatewayId(profile.id);
      selectedGatewayIdRef.current = profile.id;
      closeGatewayDialog();
      await refreshModels(profile.id);
      setCompactView("models");
    } catch (caught) {
      showError(caught);
    } finally {
      dispatchWorkflow({ type: "operationFinished" });
    }
  }

  async function refreshModels(gatewayId: string) {
    const generation = (refreshGenerationsRef.current.get(gatewayId) ?? 0) + 1;
    refreshGenerationsRef.current.set(gatewayId, generation);
    setRefreshingGatewayIds((current) => new Set(current).add(gatewayId));
    setGatewayConnectionStates((current) => ({
      ...current,
      [gatewayId]: "refreshing",
    }));
    setError(null);
    try {
      const discovered = await api.discoverModels(gatewayId);
      if (refreshGenerationsRef.current.get(gatewayId) !== generation) return;
      setModels((current) => [
        ...current.filter((model) => model.gatewayId !== gatewayId),
        ...discovered,
      ]);
      if (selectedGatewayIdRef.current === gatewayId) {
        setActiveKey(discovered[0]?.key ?? null);
      }
      setGatewayConnectionStates((current) => ({
        ...current,
        [gatewayId]: "connected",
      }));
      setStatusMessage({
        key: "modelCount",
        values: { count: discovered.length },
      });
    } catch (caught) {
      if (refreshGenerationsRef.current.get(gatewayId) !== generation) return;
      setGatewayConnectionStates((current) => ({
        ...current,
        [gatewayId]: "error",
      }));
      showError(caught);
      throw caught;
    } finally {
      if (refreshGenerationsRef.current.get(gatewayId) === generation) {
        setRefreshingGatewayIds((current) => {
          const next = new Set(current);
          next.delete(gatewayId);
          return next;
        });
      }
    }
  }

  function requestRefreshModels(gatewayId: string) {
    runAfterDiscard(() => refreshModels(gatewayId).catch(() => undefined));
  }

  async function saveManualModel(input: ManualModelInput) {
    dispatchWorkflow({ type: "operationStarted" });
    setError(null);
    try {
      const model = await api.addManualModel(input);
      setModels((current) =>
        [...current, model].sort((left, right) =>
          left.name.localeCompare(right.name),
        ),
      );
      setActiveKey(model.key);
      setManualModelDialog(false);
      setCompactView("details");
      setStatusMessage({
        key: "manualModelAdded",
        values: { name: model.name },
      });
    } catch (caught) {
      showError(caught);
    } finally {
      dispatchWorkflow({ type: "operationFinished" });
    }
  }

  function requestRemoveGateway(gateway: GatewayProfile) {
    runAfterDiscard(() => setGatewayToDelete(gateway));
  }

  async function removeGateway(gateway: GatewayProfile) {
    dispatchWorkflow({ type: "operationStarted" });
    try {
      await api.deleteGateway(gateway.id);
      const remaining = gateways.filter((item) => item.id !== gateway.id);
      setGateways(remaining);
      setModels((current) =>
        current.filter((model) => model.gatewayId !== gateway.id),
      );
      setSelectedGatewayId(remaining[0]?.id ?? null);
      selectedGatewayIdRef.current = remaining[0]?.id ?? null;
      clearSelectionOverrides(
        models
          .filter((item) => item.gatewayId === gateway.id)
          .map((model) => model.key),
      );
      setActiveKey(null);
      setGatewayConnectionStates((current) => {
        const next = { ...current };
        delete next[gateway.id];
        return next;
      });
      setGatewayToDelete(null);
      setStatusMessage({
        key: "gatewayRemoved",
        values: { name: gateway.name },
      });
    } catch (caught) {
      showError(caught);
    } finally {
      dispatchWorkflow({ type: "operationFinished" });
    }
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

  function requestApplyOpenRouter() {
    if (!activeModel) return;
    const modelKey = activeModel.key;
    runAfterDiscard(() => applyOpenRouter(modelKey));
  }

  async function applyOpenRouter(modelKey: string) {
    dispatchWorkflow({ type: "operationStarted" });
    setApplyingOpenRouter(true);
    setError(null);
    try {
      const updated = await api.applyOpenRouterModel(modelKey);
      replaceModel(updated);
      dispatchWorkflow({ type: "dirtyChanged", modelKey: null });
      setStatusMessage({ key: "openRouterApplied" });
    } catch (caught) {
      showError(caught);
    } finally {
      setApplyingOpenRouter(false);
      dispatchWorkflow({ type: "operationFinished" });
    }
  }

  async function saveModel(input: ModelUpdateInput) {
    dispatchWorkflow({ type: "operationStarted" });
    try {
      const updated = await api.updateModel(input);
      replaceModel(updated);
      dispatchWorkflow({ type: "dirtyChanged", modelKey: null });
      setStatusMessage({ key: "modelConfigSaved" });
    } catch (caught) {
      showError(caught);
    } finally {
      dispatchWorkflow({ type: "operationFinished" });
    }
  }

  async function runProbe() {
    if (!activeModel) return;
    dispatchWorkflow({ type: "operationStarted" });
    try {
      const summary = await api.probeModel(activeModel.key);
      replaceModel(summary.model);
      setProbeDialog(false);
      setStatusMessage(
        summary.notes.length
          ? { text: summary.notes.join(" ") }
          : { key: "probe" },
      );
    } catch (caught) {
      showError(caught);
    } finally {
      dispatchWorkflow({ type: "operationFinished" });
    }
  }

  function replaceModel(updated: ManagedModel) {
    setModels((current) =>
      current.map((model) => (model.key === updated.key ? updated : model)),
    );
  }

  async function toggleTarget(target: TargetKind) {
    if (!publishableTargetKinds.includes(target)) return;
    const selectedTargets = settings.selectedTargets.includes(target)
      ? settings.selectedTargets.filter((kind) => kind !== target)
      : [...settings.selectedTargets, target];
    const next = {
      ...settings,
      selectedTargets,
      targetSelectionInitialized: true,
    };
    const previous = settings;
    const generation = ++targetSettingsGenerationRef.current;
    setSettings(next);
    dispatchWorkflow({ type: "operationStarted" });
    try {
      const saved = await api.saveSettings(next);
      if (targetSettingsGenerationRef.current !== generation) return;
      setSettings(saved);
    } catch (caught) {
      if (targetSettingsGenerationRef.current !== generation) return;
      setSettings(previous);
      showError(caught);
    } finally {
      dispatchWorkflow({ type: "operationFinished" });
    }
  }

  async function previewPublish() {
    if (!selectedGateway) return;
    const request: PreparePublishRequest = {
      gatewayId: selectedGateway.id,
      modelIds: gatewayModels
        .filter((model) => selectedKeys.has(model.key))
        .map((model) => model.id),
      targets: selectedTargets,
    };
    const sessionId = ++publishSessionGenerationRef.current;
    dispatchWorkflow({ type: "publishPreviewRequested", sessionId, request });
    dispatchWorkflow({ type: "operationStarted" });
    try {
      const preview = await api.preparePublish(request);
      dispatchWorkflow({ type: "publishPreviewLoaded", sessionId, preview });
    } catch (caught) {
      dispatchWorkflow({ type: "publishPreviewFailed", sessionId });
      showError(caught);
    } finally {
      dispatchWorkflow({ type: "operationFinished" });
    }
  }

  async function executePublish(acceptConflicts: boolean) {
    if (
      publishPhase !== "ready" ||
      publishSessionId === null ||
      !publishRequest ||
      !publishPreview
    )
      return;
    const sessionId = publishSessionId;
    const request = publishRequest;
    const preview = publishPreview;
    dispatchWorkflow({ type: "publishExecutionStarted", sessionId });
    dispatchWorkflow({ type: "operationStarted" });
    try {
      const result = await api.executePublish(
        request,
        preview,
        acceptConflicts,
      );
      dispatchWorkflow({ type: "publishExecutionFinished", sessionId, result });
      setStatusMessage({
        key: result.success ? "published" : "publishFailed",
      });
      await loadTargets(true);
      if (result.success) {
        clearSelectionOverrides(
          request.modelIds.map((id) => `${request.gatewayId}::${id}`),
        );
      }
    } catch (caught) {
      dispatchWorkflow({ type: "publishExecutionFailed", sessionId });
      showError(caught);
    } finally {
      dispatchWorkflow({ type: "operationFinished" });
    }
  }

  async function saveSettings(next: AppSettings) {
    dispatchWorkflow({ type: "operationStarted" });
    try {
      const saved = await api.saveSettings(next);
      setSettings(saved);
      setSettingsDialog(false);
      await loadTargets(true);
    } catch (caught) {
      showError(caught);
    } finally {
      dispatchWorkflow({ type: "operationFinished" });
    }
  }

  async function openBackups() {
    setBackupsDialog(true);
    dispatchWorkflow({ type: "operationStarted" });
    try {
      setBackups(await api.listBackups());
    } catch (caught) {
      showError(caught);
    } finally {
      dispatchWorkflow({ type: "operationFinished" });
    }
  }

  async function restoreBackup(backup: BackupRecord) {
    dispatchWorkflow({ type: "operationStarted" });
    try {
      await api.restoreBackup(backup.id);
      setStatusMessage({ key: "restore" });
      setBackups(await api.listBackups());
      const previousKeys =
        targetModelStates.find((state) => state.target === backup.target)
          ?.matchedModelKeys ?? [];
      const nextStates = await loadTargets(true);
      const restoredKeys =
        nextStates?.find((state) => state.target === backup.target)
          ?.matchedModelKeys ?? [];
      clearSelectionOverrides(new Set([...previousKeys, ...restoredKeys]));
      setBackupToRestore(null);
    } catch (caught) {
      showError(caught);
    } finally {
      dispatchWorkflow({ type: "operationFinished" });
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
    return (
      <main className="startup-state">
        <LoaderCircle className="spin" aria-hidden="true" />
        <p>{t("loading")}</p>
      </main>
    );
  }

  if (error?.code === "DESKTOP_REQUIRED") {
    return (
      <main className="startup-state">
        <Cable aria-hidden="true" />
        <h1>{t("appName")}</h1>
        <p>{t("desktopRequired")}</p>
      </main>
    );
  }

  const hasGateway = gateways.length > 0;
  const selectedGatewayRefreshing = selectedGatewayId
    ? refreshingGatewayIds.has(selectedGatewayId)
    : false;
  const localizedErrorMessage = error ? localizedError(error, t) : null;
  const locale = settings.language === "zh-CN" ? "zh-CN" : "en-US";
  const message = statusMessage
    ? "key" in statusMessage
      ? t(statusMessage.key, statusMessage.values)
      : statusMessage.text
    : "";
  const dialogStates: Array<{ kind: DialogKind; open: boolean }> = [
    { kind: "discard", open: discardDialog },
    { kind: "restoreBackup", open: Boolean(backupToRestore) },
    { kind: "removeGateway", open: Boolean(gatewayToDelete) },
    { kind: "backups", open: backupsDialog },
    { kind: "settings", open: settingsDialog },
    { kind: "publish", open: publishDialog },
    { kind: "probe", open: probeDialog },
    { kind: "manualModel", open: manualModelDialog },
    { kind: "gateway", open: gatewayDialog },
  ];
  const activeDialog = dialogStates.find(({ open }) => open)?.kind ?? null;
  const errorNotice = localizedErrorMessage
    ? {
        ...localizedErrorMessage,
        dismissLabel: t("close"),
        onDismiss: () => setError(null),
      }
    : undefined;
  const dialogErrorNotice = (dialog: DialogKind) =>
    activeDialog === dialog ? errorNotice : undefined;

  return (
    <TooltipProvider delayDuration={350}>
      <div
        className={`app-shell compact-view-${compactView}${importReport ? " has-import-notice" : ""}`}
      >
        <a
          className="skip-link"
          href="#workspace"
          onClick={(event) => {
            event.preventDefault();
            document.getElementById("workspace")?.focus();
          }}
        >
          {t("skipWorkspace")}
        </a>
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
          onBack={() =>
            setCompactView(compactView === "details" ? "models" : "gateways")
          }
          onRefresh={() =>
            selectedGatewayId && requestRefreshModels(selectedGatewayId)
          }
          onPublish={() => runAfterDiscard(previewPublish)}
        />

        {importReport ? (
          <ImportNotice
            report={importReport}
            expanded={importDetailsExpanded}
            t={t}
            onToggle={() => setImportDetailsExpanded((current) => !current)}
            onClose={() => setImportReport(null)}
          />
        ) : null}

        <main id="workspace" className="workspace" tabIndex={-1}>
          <GatewaySidebar
            currentVersion={currentVersion}
            gateways={gateways}
            selectedId={selectedGatewayId}
            disabled={busy}
            refreshingIds={refreshingGatewayIds}
            connectionStates={gatewayConnectionStates}
            t={t}
            onSelect={selectGateway}
            onAdd={openAddGateway}
            onEdit={requestEditGateway}
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
              configuredKeys={modelSelection.configuredKeys}
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
              <div className="onboarding-rail" aria-hidden="true">
                <span>API</span>
                <span>models</span>
                <span>targets</span>
              </div>
              <div>
                <h1>{t("noGatewayTitle")}</h1>
                <p>{t("noGatewayBody")}</p>
                <Button type="button" onClick={openAddGateway}>
                  <Cable aria-hidden="true" size={17} />
                  {t("addGateway")}
                </Button>
              </div>
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
            onApplyOpenRouter={requestApplyOpenRouter}
            applyingOpenRouter={applyingOpenRouter}
            openRouterAvailable={Boolean(
              activeModel && openRouterModelMatches[activeModel.key],
            )}
            checkingOpenRouter={Boolean(
              activeModel && !(activeModel.key in openRouterModelMatches),
            )}
            onToggleTarget={(target) => void toggleTarget(target)}
            onDirtyChange={handleDirtyChange}
          />
        </main>

        <div className="live-region" role="status" aria-live="polite">
          {message}
        </div>
        {errorNotice && !activeDialog ? <ErrorNotice {...errorNotice} /> : null}
        {availableUpdate ? (
          <div className="update-banner" role="status">
            <span>
              {t("updateAvailable", { version: availableUpdate.version })}
            </span>
            <Button
              size="sm"
              type="button"
              onClick={() => void requestInstallUpdate()}
              disabled={installingUpdate}
            >
              {installingUpdate ? (
                <LoaderCircle className="spin" aria-hidden="true" size={16} />
              ) : null}
              {t("updateAndRestart")}
            </Button>
          </div>
        ) : null}
        {gatewayDialog ? (
          <GatewayDialog
            open
            busy={busy}
            gateway={editingGateway}
            initialToken={editingGatewayToken}
            t={t}
            errorNotice={dialogErrorNotice("gateway")}
            onClose={closeGatewayDialog}
            onSubmit={(input) => void saveGateway(input)}
          />
        ) : null}
        {manualModelDialog && selectedGateway ? (
          <ManualModelDialog
            open
            busy={busy}
            gateway={selectedGateway}
            t={t}
            errorNotice={dialogErrorNotice("manualModel")}
            onClose={() => setManualModelDialog(false)}
            onSubmit={(input) => void saveManualModel(input)}
          />
        ) : null}
        {probeDialog ? (
          <ProbeDialog
            open
            busy={busy}
            t={t}
            errorNotice={dialogErrorNotice("probe")}
            onClose={() => setProbeDialog(false)}
            onConfirm={() => void runProbe()}
          />
        ) : null}
        {publishDialog ? (
          <PublishDialog
            open
            busy={busy}
            preview={publishPreview}
            result={publishResult}
            t={t}
            errorNotice={dialogErrorNotice("publish")}
            onClose={() => dispatchWorkflow({ type: "publishClosed" })}
            onConfirm={(accepted) => void executePublish(accepted)}
          />
        ) : null}
        {settingsDialog ? (
          <SettingsDialog
            open
            busy={busy}
            settings={settings}
            currentVersion={currentVersion}
            availableVersion={availableUpdate?.version ?? null}
            updateCheckStatus={updateCheckStatus}
            installingUpdate={installingUpdate}
            t={t}
            errorNotice={dialogErrorNotice("settings")}
            onClose={() => setSettingsDialog(false)}
            onSubmit={(next) => void saveSettings(next)}
            onCheckForUpdates={() => void checkForUpdates()}
            onInstallUpdate={() => void requestInstallUpdate()}
          />
        ) : null}
        {backupsDialog ? (
          <BackupsDialog
            open
            busy={busy}
            backups={backups}
            locale={locale}
            t={t}
            errorNotice={dialogErrorNotice("backups")}
            onClose={() => setBackupsDialog(false)}
            onRestore={setBackupToRestore}
          />
        ) : null}
        {gatewayToDelete ? (
          <ConfirmationDialog
            open
            busy={busy}
            destructive
            title={t("deleteGatewayTitle")}
            description={t("deleteGatewayConfirm", {
              name: gatewayToDelete.name,
            })}
            confirmLabel={t("deleteGatewayAction")}
            t={t}
            errorNotice={dialogErrorNotice("removeGateway")}
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
            errorNotice={dialogErrorNotice("restoreBackup")}
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
            description={t("discardChangesBody", {
              name: activeModel?.name ?? t("unknown"),
            })}
            confirmLabel={t("discardChangesAction")}
            t={t}
            errorNotice={dialogErrorNotice("discard")}
            onClose={cancelDiscard}
            onConfirm={discardChanges}
          />
        ) : null}
      </div>
    </TooltipProvider>
  );
}

export default App;
