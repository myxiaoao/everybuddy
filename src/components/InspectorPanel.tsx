import { useEffect, useId, useMemo, useState } from "react";
import {
  BrainCircuit,
  Check,
  ChevronDown,
  CircleAlert,
  CircleCheckBig,
  CircleDashed,
  GitCompareArrows,
  Image,
  Sparkles,
  SlidersHorizontal,
  TriangleAlert,
  Wrench,
} from "lucide-react";
import type {
  CapabilitySet,
  ManagedModel,
  ModelConfiguration,
  ModelUpdateInput,
  ReasoningEffort,
  ReasoningSummary,
  TargetKind,
  TargetStatus,
} from "../types";
import type { createTranslator } from "../lib/i18n";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { ModelIcon } from "./ModelIcon";
import { TargetIcon } from "./TargetIcon";

interface InspectorPanelProps {
  model: ManagedModel | null;
  selectedCount: number;
  targets: TargetStatus[];
  selectedTargets: TargetKind[];
  busy: boolean;
  t: ReturnType<typeof createTranslator>;
  onSaveModel: (input: ModelUpdateInput) => void;
  onProbe: () => void;
  onToggleTarget: (target: TargetKind) => void;
  onDirtyChange: (modelKey: string | null, changed: boolean) => void;
}

export function InspectorPanel({
  model,
  selectedCount,
  targets,
  selectedTargets,
  busy,
  t,
  onSaveModel,
  onProbe,
  onToggleTarget,
  onDirtyChange,
}: InspectorPanelProps) {
  const [capabilities, setCapabilities] = useState<CapabilitySet | null>(
    model?.capabilities ?? null,
  );
  const [configuration, setConfiguration] = useState<ModelConfiguration | null>(
    model?.configuration ?? null,
  );
  const [name, setName] = useState(model?.name ?? "");
  const [vendor, setVendor] = useState(model?.vendor ?? "");

  const changed = useMemo(
    () =>
      Boolean(
        model &&
        capabilities &&
        configuration &&
        (model.name !== name.trim() ||
          model.vendor !== vendor.trim().toLocaleLowerCase() ||
          JSON.stringify(model.capabilities) !== JSON.stringify(capabilities) ||
          JSON.stringify(model.configuration) !==
            JSON.stringify(configuration)),
      ),
    [capabilities, configuration, model, name, vendor],
  );
  const valid = Boolean(name.trim() && vendor.trim());

  useEffect(() => {
    onDirtyChange(model?.key ?? null, changed);
  }, [changed, model?.key, onDirtyChange]);

  return (
    <aside className="inspector-panel" aria-labelledby="inspector-heading">
      <div className="inspector-heading">
        <div>
          <h2 id="inspector-heading">{t("details")}</h2>
          <span>{t("selectedCount", { count: selectedCount })}</span>
        </div>
      </div>

      {model && capabilities && configuration ? (
        <div className="inspector-content">
          <section className="model-summary">
            <ModelIcon model={model} />
            <div>
              <h3>{model.name}</h3>
              <code title={model.id}>{model.id}</code>
              <span>
                {t("vendor")}: {model.vendor}
              </span>
            </div>
          </section>

          <section className="inspector-section">
            <div className="section-title-row">
              <h3>{t("capabilities")}</h3>
              <Button
                variant="ghost"
                size="sm"
                type="button"
                onClick={onProbe}
                disabled={busy}
              >
                <Sparkles aria-hidden="true" size={16} />
                {t("probe")}
              </Button>
            </div>
            <div className="capability-list">
              <CapabilityToggle
                icon={<Wrench />}
                label={t("toolCall")}
                checked={capabilities.supportsToolCall}
                source={sourceFor(model, "toolCall", t)}
                onChange={(checked) =>
                  setCapabilities({
                    ...capabilities,
                    supportsToolCall: checked,
                  })
                }
              />
              <CapabilityToggle
                icon={<Image />}
                label={t("images")}
                checked={capabilities.supportsImages}
                source={sourceFor(model, "images", t)}
                onChange={(checked) =>
                  setCapabilities({ ...capabilities, supportsImages: checked })
                }
              />
              <CapabilityToggle
                icon={<BrainCircuit />}
                label={t("reasoning")}
                checked={capabilities.supportsReasoning}
                source={sourceFor(model, "reasoning", t)}
                onChange={(checked) => {
                  setCapabilities({
                    ...capabilities,
                    supportsReasoning: checked,
                  });
                  if (!checked) {
                    setConfiguration({
                      ...configuration,
                      onlyReasoning: false,
                      reasoning: emptyReasoningConfiguration(),
                    });
                  }
                }}
              />
            </div>
            <details className="model-config-details">
              <summary>
                <span>
                  <SlidersHorizontal aria-hidden="true" size={16} />
                  {t("advancedModelConfig")}
                </span>
                <ChevronDown
                  className="model-config-details__chevron"
                  aria-hidden="true"
                  size={16}
                />
              </summary>
              <div className="model-config-details__content">
                <ConfigGroup title={t("modelIdentityConfig")}>
                  <div className="model-config-grid">
                    <ConfigField label={t("modelDisplayName")}>
                      <Input
                        value={name}
                        onChange={(event) => setName(event.target.value)}
                      />
                    </ConfigField>
                    <ConfigField label={t("vendor")}>
                      <Input
                        value={vendor}
                        onChange={(event) => setVendor(event.target.value)}
                      />
                    </ConfigField>
                  </div>
                </ConfigGroup>

                <ConfigGroup title={t("invocationConfig")}>
                  <ConfigField
                    label={t("endpointOverride")}
                    hint={t("endpointOverrideHint")}
                  >
                    <Input
                      type="url"
                      placeholder={t("endpointOverridePlaceholder")}
                      value={configuration.endpointOverride ?? ""}
                      onChange={(event) =>
                        setConfiguration({
                          ...configuration,
                          endpointOverride: event.target.value || null,
                        })
                      }
                    />
                  </ConfigField>
                  <div className="model-config-grid model-config-grid--numbers">
                    <NumberField
                      label={t("maxInputTokens")}
                      placeholder={t("providerDefaultPlaceholder")}
                      value={configuration.maxInputTokens}
                      integer
                      onChange={(value) =>
                        setConfiguration({
                          ...configuration,
                          maxInputTokens: value,
                        })
                      }
                    />
                    <NumberField
                      label={t("maxOutputTokens")}
                      placeholder={t("providerDefaultPlaceholder")}
                      value={configuration.maxOutputTokens}
                      integer
                      onChange={(value) =>
                        setConfiguration({
                          ...configuration,
                          maxOutputTokens: value,
                        })
                      }
                    />
                    <NumberField
                      label={t("temperature")}
                      value={configuration.temperature}
                      onChange={(value) =>
                        setConfiguration({
                          ...configuration,
                          temperature: value,
                        })
                      }
                    />
                  </div>
                  <ConfigToggle
                    label={t("customProtocol")}
                    hint={t("customProtocolHint")}
                    checked={configuration.useCustomProtocol}
                    onChange={(checked) =>
                      setConfiguration({
                        ...configuration,
                        useCustomProtocol: checked,
                      })
                    }
                  />
                </ConfigGroup>

                {capabilities.supportsReasoning ? (
                  <ReasoningFields
                    model={model}
                    configuration={configuration}
                    t={t}
                    onChange={setConfiguration}
                  />
                ) : null}
              </div>
            </details>
            {changed ? (
              <Button
                variant="secondary"
                className="full-width"
                type="button"
                onClick={() =>
                  onSaveModel({
                    modelKey: model.key,
                    name: name.trim(),
                    vendor: vendor.trim().toLocaleLowerCase(),
                    capabilities,
                    configuration,
                  })
                }
                disabled={busy || !valid}
              >
                <Check aria-hidden="true" size={16} />
                {t("saveModelConfig")}
              </Button>
            ) : null}
          </section>

          <section className="inspector-section publish-section">
            <h3>{t("targets")}</h3>
            <div className="target-list">
              {targets.map((target) => {
                const invalid =
                  !target.installed ||
                  target.schema === "invalid" ||
                  !target.writable;
                return (
                  <label
                    className={`target-option${selectedTargets.includes(target.kind) ? " is-selected" : ""}${invalid ? " is-disabled" : ""}`}
                    key={target.kind}
                  >
                    <input
                      className="target-option__native-checkbox"
                      type="checkbox"
                      checked={selectedTargets.includes(target.kind)}
                      onChange={() => onToggleTarget(target.kind)}
                      disabled={invalid}
                      aria-label={target.displayName}
                    />
                    <span
                      className="target-option__checkbox"
                      aria-hidden="true"
                    >
                      <Check size={13} />
                    </span>
                    <TargetIcon target={target.kind} />
                    <span className="target-option__copy">
                      <strong>{target.displayName}</strong>
                      <small
                        className={
                          target.drifted || invalid
                            ? "status-warning"
                            : undefined
                        }
                      >
                        {targetStatusLabel(target, t)}
                      </small>
                      <code title={target.path}>{target.path}</code>
                    </span>
                    <TargetStatusIcon target={target} />
                  </label>
                );
              })}
            </div>

            <div
              className={`publish-readiness${selectedCount > 0 && selectedTargets.length > 0 ? " is-ready" : ""}`}
            >
              <GitCompareArrows aria-hidden="true" size={18} />
              <div>
                <strong>
                  {t("publishScope", {
                    models: selectedCount,
                    targets: selectedTargets.length,
                  })}
                </strong>
                <small>{t("publishHint")}</small>
              </div>
            </div>
          </section>
        </div>
      ) : (
        <div className="empty-state inspector-empty">
          <div className="empty-state__glyph" aria-hidden="true">
            <BrainCircuit size={24} />
          </div>
          <h2>{t("selectModelTitle")}</h2>
          <p>{t("selectModelBody")}</p>
        </div>
      )}
    </aside>
  );
}

const reasoningEfforts: ReasoningEffort[] = [
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
];
const reasoningSummaries: ReasoningSummary[] = [
  "auto",
  "concise",
  "detailed",
  "always",
  "never",
];

function emptyReasoningConfiguration(): ModelConfiguration["reasoning"] {
  return {
    effort: null,
    defaultEffort: null,
    supportedEfforts: [],
    summary: null,
    canDisableThinking: true,
  };
}

function ConfigGroup({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <fieldset className="model-config-group">
      <legend>{title}</legend>
      <div>{children}</div>
    </fieldset>
  );
}

function ConfigField({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="model-config-field">
      <span>{label}</span>
      {children}
      {hint ? <small>{hint}</small> : null}
    </label>
  );
}

function NumberField({
  label,
  placeholder,
  value,
  integer = false,
  onChange,
}: {
  label: string;
  placeholder?: string;
  value: number | null;
  integer?: boolean;
  onChange: (value: number | null) => void;
}) {
  return (
    <ConfigField label={label}>
      <Input
        type="number"
        min={integer ? 1 : 0}
        step={integer ? 1 : 0.1}
        inputMode="decimal"
        placeholder={placeholder}
        value={value ?? ""}
        onChange={(event) => {
          const next = event.target.value;
          onChange(next === "" ? null : Number(next));
        }}
      />
    </ConfigField>
  );
}

function ConfigToggle({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  const id = useId();
  return (
    <div className="model-config-toggle">
      <label htmlFor={id}>
        <strong>{label}</strong>
        <small>{hint}</small>
      </label>
      <Switch
        id={id}
        checked={checked}
        onCheckedChange={onChange}
        aria-label={label}
      />
    </div>
  );
}

function ReasoningFields({
  model,
  configuration,
  t,
  onChange,
}: {
  model: ManagedModel;
  configuration: ModelConfiguration;
  t: ReturnType<typeof createTranslator>;
  onChange: (configuration: ModelConfiguration) => void;
}) {
  const effortGroupId = useId();
  const effortHintId = `${effortGroupId}-hint`;
  const updateReasoning = (reasoning: ModelConfiguration["reasoning"]) => {
    onChange({ ...configuration, reasoning });
  };
  const toggleEffort = (effort: ReasoningEffort, checked: boolean) => {
    const current = configuration.reasoning.supportedEfforts;
    const supportedEfforts = checked
      ? reasoningEfforts.filter(
          (item) => item === effort || current.includes(item),
        )
      : current.filter((item) => item !== effort);
    updateReasoning({
      ...configuration.reasoning,
      supportedEfforts,
      effort:
        configuration.reasoning.effort === effort && !checked
          ? null
          : configuration.reasoning.effort,
      defaultEffort:
        configuration.reasoning.defaultEffort === effort && !checked
          ? null
          : configuration.reasoning.defaultEffort,
    });
  };

  return (
    <ConfigGroup title={t("reasoningConfig")}>
      <ConfigToggle
        label={t("onlyReasoning")}
        hint={t("onlyReasoningHint")}
        checked={configuration.onlyReasoning}
        onChange={(checked) =>
          onChange({
            ...configuration,
            onlyReasoning: checked,
            reasoning: checked
              ? { ...configuration.reasoning, canDisableThinking: false }
              : configuration.reasoning,
          })
        }
      />
      <ConfigToggle
        label={t("canDisableThinking")}
        hint={t("canDisableThinkingHint")}
        checked={configuration.reasoning.canDisableThinking}
        onChange={(checked) =>
          onChange({
            ...configuration,
            onlyReasoning: checked ? false : configuration.onlyReasoning,
            reasoning: {
              ...configuration.reasoning,
              canDisableThinking: checked,
            },
          })
        }
      />
      <fieldset
        className="effort-fieldset"
        aria-labelledby={effortGroupId}
        aria-describedby={effortHintId}
      >
        <legend id={effortGroupId}>{t("supportedEfforts")}</legend>
        <p id={effortHintId} className="effort-fieldset__hint">
          {reasoningEffortHint(model, configuration, t)}
        </p>
        <div className="effort-grid">
          {reasoningEfforts.map((effort) => {
            const id = `${effortGroupId}-${effort}`;
            return (
              <label htmlFor={id} key={effort}>
                <Checkbox
                  id={id}
                  checked={configuration.reasoning.supportedEfforts.includes(
                    effort,
                  )}
                  onCheckedChange={(checked) =>
                    toggleEffort(effort, checked === true)
                  }
                />
                <span>{t(`effort_${effort}`)}</span>
              </label>
            );
          })}
        </div>
      </fieldset>
      <div className="model-config-grid">
        <EffortSelect
          label={t("defaultEffort")}
          value={configuration.reasoning.defaultEffort}
          supported={configuration.reasoning.supportedEfforts}
          emptyLabel={t("automatic")}
          t={t}
          onChange={(value) =>
            updateReasoning({
              ...configuration.reasoning,
              defaultEffort: value,
            })
          }
        />
        <EffortSelect
          label={t("legacyEffort")}
          value={configuration.reasoning.effort}
          supported={configuration.reasoning.supportedEfforts}
          emptyLabel={t("notSet")}
          t={t}
          onChange={(value) =>
            updateReasoning({ ...configuration.reasoning, effort: value })
          }
        />
      </div>
      <ConfigField
        label={t("reasoningSummary")}
        hint={t("reasoningSummaryHint")}
      >
        <select
          className="model-config-select"
          value={configuration.reasoning.summary ?? ""}
          onChange={(event) =>
            updateReasoning({
              ...configuration.reasoning,
              summary: event.target.value
                ? (event.target.value as ReasoningSummary)
                : null,
            })
          }
        >
          <option value="">{t("notSet")}</option>
          {reasoningSummaries.map((summary) => (
            <option key={summary} value={summary}>
              {t(`summary_${summary}`)}
            </option>
          ))}
        </select>
      </ConfigField>
    </ConfigGroup>
  );
}

function reasoningEffortHint(
  model: ManagedModel,
  configuration: ModelConfiguration,
  t: ReturnType<typeof createTranslator>,
) {
  const hasSavedOverride = model.evidence.some(
    (item) =>
      item.capability === "reasoningConfiguration" && item.source === "manual",
  );
  const hasDraftOverride =
    model.configuration.onlyReasoning !== configuration.onlyReasoning ||
    JSON.stringify(model.configuration.reasoning) !==
      JSON.stringify(configuration.reasoning);

  if (hasSavedOverride || hasDraftOverride) {
    return t("reasoningEffortsOverridden");
  }
  if (configuration.reasoning.supportedEfforts.length === 0) {
    return t("reasoningEffortsUnknown");
  }
  return t("reasoningEffortsResolved");
}

function EffortSelect({
  label,
  value,
  supported,
  emptyLabel,
  t,
  onChange,
}: {
  label: string;
  value: ReasoningEffort | null;
  supported: ReasoningEffort[];
  emptyLabel: string;
  t: ReturnType<typeof createTranslator>;
  onChange: (value: ReasoningEffort | null) => void;
}) {
  return (
    <ConfigField label={label}>
      <select
        className="model-config-select"
        value={value ?? ""}
        disabled={supported.length === 0}
        onChange={(event) =>
          onChange(
            event.target.value ? (event.target.value as ReasoningEffort) : null,
          )
        }
      >
        <option value="">{emptyLabel}</option>
        {supported.map((effort) => (
          <option key={effort} value={effort}>
            {t(`effort_${effort}`)}
          </option>
        ))}
      </select>
    </ConfigField>
  );
}

function TargetStatusIcon({ target }: { target: TargetStatus }) {
  if (target.schema === "invalid" || !target.writable)
    return (
      <CircleAlert className="status-warning" aria-hidden="true" size={17} />
    );
  if (target.drifted)
    return (
      <TriangleAlert className="status-warning" aria-hidden="true" size={17} />
    );
  if (!target.installed) return <CircleDashed aria-hidden="true" size={17} />;
  return <CircleCheckBig aria-hidden="true" size={17} />;
}

function CapabilityToggle({
  icon,
  label,
  checked,
  source,
  onChange,
}: {
  icon: React.ReactElement;
  label: string;
  checked: boolean;
  source: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="capability-toggle">
      <input
        className="capability-toggle__native-input"
        type="checkbox"
        role="switch"
        checked={checked}
        onChange={(event) => onChange(event.currentTarget.checked)}
        aria-label={label}
      />
      <span className="capability-toggle__icon" aria-hidden="true">
        {icon}
      </span>
      <span className="capability-toggle__copy">
        <strong>{label}</strong>
        <small>{source}</small>
      </span>
      <span className="capability-toggle__visual-switch" aria-hidden="true">
        <span />
      </span>
    </label>
  );
}

const sourceOrder = [
  "manual",
  "probe",
  "imported",
  "openRouter",
  "metadata",
  "default",
] as const;

function sourceFor(
  model: ManagedModel,
  capability: string,
  t: ReturnType<typeof createTranslator>,
) {
  const evidence = sourceOrder
    .map((source) =>
      model.evidence.find(
        (item) => item.capability === capability && item.source === source,
      ),
    )
    .find(Boolean);
  const labels = {
    default: t("evidenceDefault"),
    metadata: t("evidenceMetadata"),
    openRouter: t("evidenceOpenRouter"),
    imported: t("evidenceImported"),
    probe: t("evidenceProbe"),
    manual: t("evidenceManual"),
  };
  return t("capabilitySource", {
    source: evidence ? labels[evidence.source] : t("unknown"),
  });
}

function targetStatusLabel(
  target: TargetStatus,
  t: ReturnType<typeof createTranslator>,
) {
  if (target.schema === "invalid" || !target.writable)
    return t("targetInvalid");
  if (target.drifted) return t("targetDrifted");
  if (!target.installed) return t("targetMissing");
  return t("targetReady");
}
