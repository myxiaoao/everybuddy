export type TargetKind = "workbuddy" | "codebuddy";
export type Theme = "light" | "dark" | "system";
export type Language = "zh-CN" | "en";
export type EvidenceSource =
  "default" | "metadata" | "openRouter" | "imported" | "probe" | "manual";
export type TargetSchema = "missing" | "array" | "wrapped" | "invalid";
export type GatewayConnectionState =
  "idle" | "refreshing" | "connected" | "error";

export interface GatewayProfile {
  id: string;
  name: string;
  apiRoot: string;
  createdAt: string;
  updatedAt: string;
}

export interface GatewayInput {
  id?: string;
  name: string;
  baseUrl: string;
  token?: string;
}

export interface SaveGatewayResult {
  profile: GatewayProfile;
  modelsInvalidated: boolean;
}

export interface ManualModelInput {
  gatewayId: string;
  id: string;
  name: string;
  vendor: string;
}

export interface CapabilitySet {
  supportsToolCall: boolean;
  supportsImages: boolean;
  supportsReasoning: boolean;
  reasoningEfforts: string[];
}

export type ReasoningEffort =
  "minimal" | "low" | "medium" | "high" | "xhigh" | "max";
export type ReasoningSummary =
  "auto" | "always" | "never" | "concise" | "detailed";

export interface ReasoningConfiguration {
  effort: ReasoningEffort | null;
  defaultEffort: ReasoningEffort | null;
  supportedEfforts: ReasoningEffort[];
  summary: ReasoningSummary | null;
  canDisableThinking: boolean;
}

export interface ModelConfiguration {
  endpointOverride: string | null;
  maxInputTokens: number | null;
  maxOutputTokens: number | null;
  temperature: number | null;
  onlyReasoning: boolean;
  reasoning: ReasoningConfiguration;
  useCustomProtocol: boolean;
}

export interface ModelUpdateInput {
  modelKey: string;
  name: string;
  vendor: string;
  capabilities: CapabilitySet;
  configuration: ModelConfiguration;
}

export interface CapabilityEvidence {
  capability:
    | "toolCall"
    | "images"
    | "reasoning"
    | "configuration"
    | "reasoningConfiguration";
  value: boolean;
  source: EvidenceSource;
  detail: string;
  checkedAt: string;
}

export interface ManagedModel {
  key: string;
  gatewayId: string;
  id: string;
  name: string;
  vendor: string;
  capabilities: CapabilitySet;
  configuration: ModelConfiguration;
  evidence: CapabilityEvidence[];
  metadata: Record<string, unknown>;
  updatedAt: string;
}

export interface TargetStatus {
  kind: TargetKind;
  displayName: string;
  path: string;
  installed: boolean;
  fileExists: boolean;
  writable: boolean;
  schema: TargetSchema;
  fingerprint: string | null;
  drifted: boolean;
  error: string | null;
}

export interface TargetModelState {
  target: TargetKind;
  fingerprint: string | null;
  matchedModelKeys: string[];
  unmatchedCount: number;
  skippedCount: number;
}

export interface TargetSnapshot {
  targets: TargetStatus[];
  targetModelStates: TargetModelState[];
}

export interface TargetImportIssue {
  target: TargetKind;
  modelId: string | null;
  code: string;
  message: string;
}

export interface TargetImportReport {
  importedGatewayCount: number;
  importedModelCount: number;
  issues: TargetImportIssue[];
}

export interface AppSettings {
  language: Language;
  theme: Theme;
  selectedTargets: TargetKind[];
  targetSelectionInitialized: boolean;
  targetPaths: Record<TargetKind, string>;
}

export interface BootstrapData {
  gateways: GatewayProfile[];
  models: ManagedModel[];
  targets: TargetStatus[];
  targetModelStates: TargetModelState[];
  importReport: TargetImportReport;
  settings: AppSettings;
}

export interface PreparePublishRequest {
  gatewayId: string;
  modelIds: string[];
  targets: TargetKind[];
}

export interface TargetPreview {
  target: TargetKind;
  path: string;
  writePath: string;
  fingerprint: string | null;
  addCount: number;
  updateCount: number;
  unchangedCount: number;
  removeCount?: number;
}

export interface ModelConflict {
  target: TargetKind;
  modelId: string;
  existingName: string;
}

export interface PublishPreview {
  targets: TargetPreview[];
  conflicts: ModelConflict[];
  warnings: string[];
  gatewayRevision: string;
  credentialRevision: string;
  modelRevisions: Array<{ key: string; updatedAt: string }>;
}

export interface PublishResult {
  success: boolean;
  results: Array<{
    target: TargetKind;
    success: boolean;
    rollbackAttempted: boolean;
    rolledBack: boolean;
    message: string;
  }>;
}

export interface ProbeSummary {
  model: ManagedModel;
  requestCount: number;
  notes: string[];
}

export interface BackupRecord {
  id: string;
  target: TargetKind;
  path: string;
  sourcePath: string;
  fingerprint: string;
  createdAt: string;
}

export interface AppError {
  code: string;
  message: string;
}
