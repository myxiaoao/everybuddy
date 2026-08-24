import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  AppError,
  AppSettings,
  BackupRecord,
  BootstrapData,
  GatewayInput,
  GatewayProfile,
  ManagedModel,
  ManualModelInput,
  ModelConfiguration,
  ModelUpdateInput,
  PreparePublishRequest,
  ProbeSummary,
  PublishPreview,
  PublishResult,
  ReasoningEffort,
  TargetKind,
  TargetImportReport,
  TargetModelState,
  TargetStatus,
} from "../types";

const demoEnabled =
  !isTauri() && new URLSearchParams(window.location.search).has("demo");

export const api = {
  bootstrap: () => call<BootstrapData>("bootstrap"),
  saveGateway: (input: GatewayInput) =>
    call<GatewayProfile>("save_gateway", { input }),
  getGatewayToken: (id: string) => call<string>("get_gateway_token", { id }),
  deleteGateway: (id: string) => call<void>("delete_gateway", { id }),
  discoverModels: (gatewayId: string) =>
    call<ManagedModel[]>("discover_models", { gatewayId }),
  addManualModel: (input: ManualModelInput) =>
    call<ManagedModel>("add_manual_model", { input }),
  probeModel: (modelKey: string) =>
    call<ProbeSummary>("probe_model", { modelKey }),
  updateModel: (input: ModelUpdateInput) =>
    call<ManagedModel>("update_model", { input }),
  getTargetStatuses: () => call<TargetStatus[]>("get_target_statuses"),
  getTargetModelStates: () =>
    call<TargetModelState[]>("get_target_model_states"),
  preparePublish: (request: PreparePublishRequest) =>
    call<PublishPreview>("prepare_publish", { request }),
  executePublish: (
    request: PreparePublishRequest,
    preview: PublishPreview,
    acceptConflicts: boolean,
  ) =>
    call<PublishResult>("execute_publish", {
      request: {
        ...request,
        expectations: preview.targets.map((target) => ({
          target: target.target,
          fingerprint: target.fingerprint,
        })),
        acceptConflicts,
      },
    }),
  listBackups: (target?: TargetKind) =>
    call<BackupRecord[]>("list_backups", { target: target ?? null }),
  restoreBackup: (id: string) => call<void>("restore_backup", { id }),
  saveSettings: (settings: AppSettings) =>
    call<AppSettings>("save_settings", { input: settings }),
};

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    if (demoEnabled) {
      return (await demoCall(command, args)) as T;
    }
    if (!isTauri()) {
      throw {
        code: "DESKTOP_REQUIRED",
        message:
          "Open EveryBuddy as a desktop app, or append ?demo=1 for the UI demo.",
      } satisfies AppError;
    }
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeError(error);
  }
}

function normalizeError(error: unknown): AppError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error
  ) {
    return {
      code: String(error.code),
      message: String(error.message),
    };
  }
  return {
    code: "UNEXPECTED_ERROR",
    message: error instanceof Error ? error.message : String(error),
  };
}

const now = "2026-08-20T08:30:00Z";
const demoGateway: GatewayProfile = {
  id: "demo-gateway",
  name: "Sub2API",
  apiRoot: "https://api.example.dev/v1",
  tokenRef: "demo-gateway",
  createdAt: now,
  updatedAt: now,
};

const demoRelay: GatewayProfile = {
  id: "demo-relay",
  name: "Local Relay",
  apiRoot: "http://127.0.0.1:8080/v1",
  tokenRef: "demo-relay",
  createdAt: now,
  updatedAt: now,
};

let demoGateways = [demoGateway, demoRelay];
const demoTokens = new Map<string, string>([
  [demoGateway.id, "demo-token-primary"],
  [demoRelay.id, "demo-token-relay"],
]);

let demoModels: ManagedModel[] = [
  createDemoModel(
    demoGateway.id,
    "gpt-5.6",
    "GPT-5.6",
    "openai",
    true,
    true,
    true,
    "imported",
    ["low", "medium", "high", "xhigh", "max"],
    "high",
  ),
  createDemoModel(
    demoGateway.id,
    "claude-sonnet-4-5",
    "Claude Sonnet 4.5",
    "anthropic",
    true,
    true,
    false,
    "openRouter",
  ),
  createDemoModel(
    demoGateway.id,
    "deepseek-r1",
    "DeepSeek R1",
    "deepseek",
    false,
    false,
    true,
    "openRouter",
    [],
    null,
  ),
  createDemoModel(
    demoGateway.id,
    "qwen3-coder",
    "Qwen3 Coder",
    "qwen",
    true,
    false,
    false,
    "probe",
  ),
  createDemoModel(
    demoRelay.id,
    "glm-4.5",
    "GLM 4.5",
    "zhipu",
    true,
    false,
    true,
    "metadata",
    [],
    null,
  ),
  createDemoModel(
    demoRelay.id,
    "moonshot-v1",
    "Moonshot V1",
    "moonshot",
    true,
    true,
    false,
    "metadata",
  ),
];

let demoSettings: AppSettings = {
  language: "zh-CN",
  theme: "system",
  selectedTargets: ["workbuddy", "codebuddy"],
  targetPaths: {
    workbuddy: "~/.workbuddy/models.json",
    codebuddy: "~/.codebuddy/models.json",
  },
};

const demoTargets: TargetStatus[] = [
  {
    kind: "workbuddy",
    displayName: "WorkBuddy",
    path: "~/.workbuddy/models.json",
    installed: true,
    fileExists: true,
    writable: true,
    schema: "array",
    fingerprint: "demo-workbuddy",
    drifted: false,
    error: null,
  },
  {
    kind: "codebuddy",
    displayName: "CodeBuddy",
    path: "~/.codebuddy/models.json",
    installed: true,
    fileExists: true,
    writable: true,
    schema: "wrapped",
    fingerprint: "demo-codebuddy",
    drifted: true,
    error: null,
  },
];

let demoTargetModelStates: TargetModelState[] = [
  {
    target: "workbuddy",
    fingerprint: "demo-workbuddy",
    matchedModelKeys: [
      "demo-gateway::gpt-5.6",
      "demo-gateway::claude-sonnet-4-5",
    ],
    unmatchedCount: 0,
    skippedCount: 0,
  },
  {
    target: "codebuddy",
    fingerprint: "demo-codebuddy",
    matchedModelKeys: ["demo-gateway::gpt-5.6"],
    unmatchedCount: 1,
    skippedCount: 0,
  },
];

const demoImportReport: TargetImportReport = {
  importedGatewayCount: 1,
  importedModelCount: 2,
  issues: [
    {
      target: "codebuddy",
      modelId: "claude-sonnet-4-5",
      code: "targetConflict",
      message: "Parameters differ from the WorkBuddy import baseline",
    },
  ],
};

async function demoCall(
  command: string,
  args?: Record<string, unknown>,
): Promise<unknown> {
  await new Promise((resolve) => window.setTimeout(resolve, 180));
  switch (command) {
    case "bootstrap":
      return {
        gateways: demoGateways,
        models: demoModels,
        targets: demoTargets,
        targetModelStates: demoTargetModelStates,
        importReport: demoImportReport,
        settings: demoSettings,
      } satisfies BootstrapData;
    case "save_gateway": {
      const input = (args as { input: GatewayInput }).input;
      const existing = demoGateways.find((gateway) => gateway.id === input.id);
      const profile: GatewayProfile = {
        id: input.id ?? `demo-gateway-${demoGateways.length + 1}`,
        name: input.name.trim(),
        apiRoot: input.baseUrl.replace(/\/models\/?$/, "").replace(/\/$/, ""),
        tokenRef: input.id ?? `demo-gateway-${demoGateways.length + 1}`,
        createdAt: existing?.createdAt ?? now,
        updatedAt: now,
      };
      demoGateways = existing
        ? demoGateways.map((gateway) =>
            gateway.id === profile.id ? profile : gateway,
          )
        : [...demoGateways, profile];
      demoTokens.set(profile.id, input.token);
      return profile;
    }
    case "get_gateway_token": {
      const id = String((args as { id: string }).id);
      const token = demoTokens.get(id);
      if (!token) {
        throw {
          code: "SECRET_STORE_ERROR",
          message:
            "The gateway token is missing from the system credential store",
        } satisfies AppError;
      }
      return token;
    }
    case "delete_gateway": {
      const id = String((args as { id: string }).id);
      demoGateways = demoGateways.filter((gateway) => gateway.id !== id);
      demoModels = demoModels.filter((model) => model.gatewayId !== id);
      demoTokens.delete(id);
      return undefined;
    }
    case "restore_backup":
      return undefined;
    case "discover_models": {
      const gatewayId = String((args as { gatewayId: string }).gatewayId);
      return demoModels.filter((model) => model.gatewayId === gatewayId);
    }
    case "add_manual_model": {
      const input = (args as { input: ManualModelInput }).input;
      if (
        demoModels.some(
          (model) =>
            model.gatewayId === input.gatewayId && model.id === input.id.trim(),
        )
      ) {
        throw {
          code: "VALIDATION",
          message: "This model ID already exists in the selected API source",
        } satisfies AppError;
      }
      const model = createDemoManualModel(input);
      demoModels = [...demoModels, model];
      return model;
    }
    case "probe_model": {
      const key = String((args as { modelKey: string }).modelKey);
      const model =
        demoModels.find((item) => item.key === key) ?? demoModels[0];
      return { model, requestCount: 3, notes: [] } satisfies ProbeSummary;
    }
    case "update_model": {
      const input = (args as { input: ModelUpdateInput }).input;
      demoModels = demoModels.map((model) =>
        model.key === input.modelKey ? updateDemoModel(model, input) : model,
      );
      return demoModels.find((model) => model.key === input.modelKey);
    }
    case "get_target_statuses":
      return demoTargets;
    case "get_target_model_states":
      return demoTargetModelStates;
    case "prepare_publish": {
      const request = (args as { request: PreparePublishRequest }).request;
      return {
        targets: request.targets.map((target) => ({
          target,
          path: demoSettings.targetPaths[target],
          fingerprint: `demo-${target}`,
          addCount: request.modelIds.length - 1,
          updateCount: 1,
          unchangedCount: 0,
        })),
        conflicts: request.targets.map((target) => ({
          target,
          modelId: request.modelIds[0],
          existingName: request.modelIds[0],
        })),
        warnings: ["Target configuration files contain the API token."],
      } satisfies PublishPreview;
    }
    case "execute_publish": {
      const request = (args as { request: PreparePublishRequest }).request;
      const publishedKeys = request.modelIds.map(
        (id) => `${request.gatewayId}::${id}`,
      );
      demoTargetModelStates = demoTargetModelStates.map((state) =>
        request.targets.includes(state.target)
          ? {
              ...state,
              matchedModelKeys: [
                ...new Set([...state.matchedModelKeys, ...publishedKeys]),
              ],
            }
          : state,
      );
      return {
        success: true,
        results: request.targets.map((target) => ({
          target,
          success: true,
          rollbackAttempted: false,
          rolledBack: false,
          message: `Published to ${target}`,
        })),
      } satisfies PublishResult;
    }
    case "list_backups":
      return [
        {
          id: "demo-backup",
          target: "workbuddy",
          path: "/backups/workbuddy/demo.json",
          sourcePath: "~/.workbuddy/models.json",
          fingerprint: "demo",
          createdAt: now,
        },
      ] satisfies BackupRecord[];
    case "save_settings":
      demoSettings = (args as { input: AppSettings }).input;
      return demoSettings;
    default:
      throw new Error(`Unsupported demo command: ${command}`);
  }
}

function createDemoModel(
  gatewayId: string,
  id: string,
  name: string,
  vendor: string,
  supportsToolCall: boolean,
  supportsImages: boolean,
  supportsReasoning: boolean,
  source: "default" | "metadata" | "openRouter" | "imported" | "probe",
  supportedEfforts: ReasoningEffort[] = [],
  defaultEffort: ReasoningEffort | null = null,
): ManagedModel {
  const capabilities = {
    supportsToolCall,
    supportsImages,
    supportsReasoning,
    reasoningEfforts: supportsReasoning ? supportedEfforts : [],
  };
  const configuration: ModelConfiguration = {
    endpointOverride: null,
    maxInputTokens: null,
    maxOutputTokens: null,
    temperature: null,
    onlyReasoning: false,
    reasoning: {
      effort: null,
      defaultEffort: supportsReasoning ? defaultEffort : null,
      supportedEfforts: supportsReasoning ? supportedEfforts : [],
      summary: null,
      canDisableThinking: true,
    },
    useCustomProtocol: false,
  };
  return {
    key: `${gatewayId}::${id}`,
    gatewayId,
    id,
    name,
    vendor,
    capabilities,
    configuration,
    evidence: [
      {
        capability: "toolCall",
        value: supportsToolCall,
        source,
        detail:
          source === "probe"
            ? "Tool call returned"
            : source === "openRouter"
              ? "OpenRouter model directory"
              : source === "default"
                ? "Conservative default"
                : "Known model metadata",
        checkedAt: now,
      },
      {
        capability: "images",
        value: supportsImages,
        source,
        detail: "Known model metadata",
        checkedAt: now,
      },
      {
        capability: "reasoning",
        value: supportsReasoning,
        source,
        detail: "Known model metadata",
        checkedAt: now,
      },
    ],
    metadata:
      source === "imported"
        ? { id, owned_by: vendor, everybuddySource: "targetImport" }
        : { id, owned_by: vendor },
    updatedAt: now,
  };
}

function updateDemoModel(
  model: ManagedModel,
  input: ModelUpdateInput,
): ManagedModel {
  const reasoningChanged =
    model.configuration.onlyReasoning !== input.configuration.onlyReasoning ||
    JSON.stringify(model.configuration.reasoning) !==
      JSON.stringify(input.configuration.reasoning);
  const evidence = reasoningChanged
    ? [
        ...model.evidence.filter(
          (item) => item.capability !== "reasoningConfiguration",
        ),
        {
          capability: "reasoningConfiguration" as const,
          value: true,
          source: "manual" as const,
          detail: "User override",
          checkedAt: now,
        },
      ]
    : model.evidence;

  return {
    ...model,
    name: input.name,
    vendor: input.vendor,
    capabilities: input.capabilities,
    configuration: input.configuration,
    evidence,
  };
}

function createDemoManualModel(input: ManualModelInput): ManagedModel {
  const id = input.id.trim();
  const vendor = input.vendor.trim().toLocaleLowerCase() || "custom";
  const model = createDemoModel(
    input.gatewayId,
    id,
    input.name.trim() || id,
    vendor,
    false,
    false,
    false,
    "default",
  );
  return {
    ...model,
    metadata: { id, owned_by: vendor, everybuddySource: "manual" },
  };
}
