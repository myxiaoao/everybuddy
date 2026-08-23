import { useMemo, useState } from "react";
import { BrainCircuit, Image, Plus, Search, Wrench, X } from "lucide-react";
import type { ManagedModel, TargetKind } from "../types";
import type { createTranslator } from "../lib/i18n";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { Button } from "@/components/ui/button";

type CapabilityFilter = "all" | "toolCall" | "images" | "reasoning";

interface ModelListProps {
  models: ManagedModel[];
  totalModelCount: number;
  query: string;
  selectedKeys: Set<string>;
  indeterminateKeys: Set<string>;
  presentTargetsByKey: Map<string, TargetKind[]>;
  selectedCount: number;
  activeKey: string | null;
  disabled: boolean;
  t: ReturnType<typeof createTranslator>;
  onQueryChange: (query: string) => void;
  onToggleAll: (models: ManagedModel[]) => void;
  onToggle: (key: string) => void;
  onClearSelection: () => void;
  onAddManual: () => void;
  onActivate: (key: string) => void;
}

export function ModelList({
  models,
  totalModelCount,
  query,
  selectedKeys,
  indeterminateKeys,
  presentTargetsByKey,
  selectedCount,
  activeKey,
  disabled,
  t,
  onQueryChange,
  onToggleAll,
  onToggle,
  onClearSelection,
  onAddManual,
  onActivate,
}: ModelListProps) {
  const [filter, setFilter] = useState<CapabilityFilter>("all");
  const visibleModels = useMemo(
    () =>
      models.filter((model) => {
        if (filter === "all") return true;
        if (filter === "toolCall") return model.capabilities.supportsToolCall;
        if (filter === "images") return model.capabilities.supportsImages;
        return model.capabilities.supportsReasoning;
      }),
    [filter, models],
  );
  const allSelected =
    visibleModels.length > 0 &&
    visibleModels.every((model) => selectedKeys.has(model.key));
  const someSelected = visibleModels.some(
    (model) => selectedKeys.has(model.key) || indeterminateKeys.has(model.key),
  );
  const hasActiveFilters = Boolean(query.trim()) || filter !== "all";

  function clearFilters() {
    setFilter("all");
    onQueryChange("");
  }

  return (
    <section className="model-panel" aria-labelledby="models-heading">
      <div className="model-toolbar">
        <div>
          <h1 id="models-heading">{t("models")}</h1>
          <span>{t("modelCount", { count: visibleModels.length })}</span>
        </div>
        <div className="model-toolbar__actions">
          <label className="search-field">
            <span className="sr-only">{t("searchModels")}</span>
            <Search aria-hidden="true" size={17} />
            <Input
              className="h-full min-h-0 border-0 bg-transparent p-0 shadow-none focus-visible:ring-0"
              value={query}
              onChange={(event) => onQueryChange(event.currentTarget.value)}
              placeholder={t("searchModels")}
              type="search"
            />
          </label>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="secondary"
                className="manual-model-button"
                type="button"
                onClick={onAddManual}
                aria-label={t("addManualModel")}
                disabled={disabled}
              >
                <Plus aria-hidden="true" size={16} />
                <span>{t("addManualModelShort")}</span>
              </Button>
            </TooltipTrigger>
            <TooltipContent>{t("addManualModel")}</TooltipContent>
          </Tooltip>
        </div>
      </div>

      <div className="model-controlbar">
        <div
          className="model-filters"
          role="group"
          aria-label={t("filterCapabilities")}
        >
          <FilterButton
            label={t("allModels")}
            active={filter === "all"}
            onClick={() => setFilter("all")}
          />
          <FilterButton
            label={t("filterToolCall")}
            active={filter === "toolCall"}
            icon={<Wrench />}
            onClick={() => setFilter("toolCall")}
          />
          <FilterButton
            label={t("filterImages")}
            active={filter === "images"}
            icon={<Image />}
            onClick={() => setFilter("images")}
          />
          <FilterButton
            label={t("filterReasoning")}
            active={filter === "reasoning"}
            icon={<BrainCircuit />}
            onClick={() => setFilter("reasoning")}
          />
        </div>
        <div
          className={`selection-slot${selectedCount > 0 ? " is-visible" : ""}`}
          aria-live="polite"
        >
          <span>{t("selectedCount", { count: selectedCount })}</span>
          <Button
            variant="ghost"
            size="icon-sm"
            type="button"
            onClick={onClearSelection}
            aria-label={t("clearSelection")}
            disabled={selectedCount === 0}
          >
            <X aria-hidden="true" size={15} />
          </Button>
        </div>
      </div>

      {visibleModels.length ? (
        <div className="model-table" role="table" aria-label={t("models")}>
          <div className="model-table__header" role="row">
            <div className="model-check" role="columnheader">
              <Checkbox
                checked={
                  allSelected ? true : someSelected ? "indeterminate" : false
                }
                onCheckedChange={() => onToggleAll(visibleModels)}
                aria-label={t("selectAll")}
              />
            </div>
            <span className="model-table__identity-header" role="columnheader">
              {t("modelId")}
            </span>
            <span
              className="model-table__capabilities-header"
              role="columnheader"
            >
              {t("capabilities")}
            </span>
          </div>
          <div className="model-table__body" role="rowgroup">
            {visibleModels.map((model) => (
              <div
                className={`model-row${activeKey === model.key ? " is-active" : ""}`}
                role="row"
                key={model.key}
              >
                <div className="model-check" role="cell">
                  <ModelSelectionCheckbox
                    model={model}
                    checked={selectedKeys.has(model.key)}
                    indeterminate={indeterminateKeys.has(model.key)}
                    presentTargets={presentTargetsByKey.get(model.key) ?? []}
                    t={t}
                    onToggle={onToggle}
                  />
                </div>
                <div className="model-row__identity-cell" role="cell">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        className="model-row__identity"
                        type="button"
                        onClick={() => onActivate(model.key)}
                      >
                        <span className="model-row__name">
                          <strong>{model.name}</strong>
                          {model.metadata.everybuddySource === "manual" ? (
                            <small>{t("manualModelBadge")}</small>
                          ) : null}
                          {model.metadata.everybuddySource ===
                          "targetImport" ? (
                            <small>{t("importedModelBadge")}</small>
                          ) : null}
                        </span>
                        <code className="model-row__id">{model.id}</code>
                      </button>
                    </TooltipTrigger>
                    <TooltipContent>
                      <code className="model-id-tooltip">{model.id}</code>
                    </TooltipContent>
                  </Tooltip>
                </div>
                <div
                  className="capability-icons"
                  role="cell"
                  aria-label={[
                    `${t("toolCall")}: ${model.capabilities.supportsToolCall ? t("supported") : t("unsupported")}`,
                    `${t("images")}: ${model.capabilities.supportsImages ? t("supported") : t("unsupported")}`,
                    `${t("reasoning")}: ${model.capabilities.supportsReasoning ? t("supported") : t("unsupported")}`,
                  ].join(", ")}
                >
                  <CapabilityIcon
                    enabled={model.capabilities.supportsToolCall}
                    label={t("toolCall")}
                    stateLabel={
                      model.capabilities.supportsToolCall
                        ? t("supported")
                        : t("unsupported")
                    }
                  >
                    <Wrench />
                  </CapabilityIcon>
                  <CapabilityIcon
                    enabled={model.capabilities.supportsImages}
                    label={t("images")}
                    stateLabel={
                      model.capabilities.supportsImages
                        ? t("supported")
                        : t("unsupported")
                    }
                  >
                    <Image />
                  </CapabilityIcon>
                  <CapabilityIcon
                    enabled={model.capabilities.supportsReasoning}
                    label={t("reasoning")}
                    stateLabel={
                      model.capabilities.supportsReasoning
                        ? t("supported")
                        : t("unsupported")
                    }
                  >
                    <BrainCircuit />
                  </CapabilityIcon>
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : (
        <div className="empty-state">
          <div className="empty-state__glyph" aria-hidden="true">
            <Search size={24} />
          </div>
          <h2>
            {totalModelCount > 0 && hasActiveFilters
              ? t("noMatchingModelsTitle")
              : t("noModelsTitle")}
          </h2>
          <p>
            {totalModelCount > 0 && hasActiveFilters
              ? t("noMatchingModelsBody", {
                  query: query.trim() || t("activeCapabilityFilter"),
                })
              : t("noModelsBody")}
          </p>
          {totalModelCount > 0 && hasActiveFilters ? (
            <Button variant="secondary" type="button" onClick={clearFilters}>
              <X aria-hidden="true" size={16} />
              {t("clearFilters")}
            </Button>
          ) : null}
        </div>
      )}
    </section>
  );
}

function ModelSelectionCheckbox({
  model,
  checked,
  indeterminate,
  presentTargets,
  t,
  onToggle,
}: {
  model: ManagedModel;
  checked: boolean;
  indeterminate: boolean;
  presentTargets: TargetKind[];
  t: ReturnType<typeof createTranslator>;
  onToggle: (key: string) => void;
}) {
  const targets = presentTargets.map(displayTarget).join(", ");
  const label = indeterminate
    ? t("selectPartialModel", { name: model.name, targets })
    : t("selectModel", { name: model.name });
  const checkbox = (
    <Checkbox
      checked={indeterminate ? "indeterminate" : checked}
      onCheckedChange={() => onToggle(model.key)}
      aria-label={label}
    />
  );

  if (!indeterminate) return checkbox;
  return (
    <Tooltip>
      <TooltipTrigger asChild>{checkbox}</TooltipTrigger>
      <TooltipContent>{t("modelPresentInTargets", { targets })}</TooltipContent>
    </Tooltip>
  );
}

function displayTarget(target: TargetKind) {
  return target === "workbuddy" ? "WorkBuddy" : "CodeBuddy";
}

function FilterButton({
  label,
  active,
  icon,
  onClick,
}: {
  label: string;
  active: boolean;
  icon?: React.ReactElement;
  onClick: () => void;
}) {
  const button = (
    <button
      className={`model-filter${active ? " is-active" : ""}`}
      type="button"
      onClick={onClick}
      aria-pressed={active}
      aria-label={label}
    >
      {icon ?? <span>{label}</span>}
    </button>
  );

  if (!icon) return button;
  return (
    <Tooltip>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function CapabilityIcon({
  enabled,
  label,
  stateLabel,
  children,
}: {
  enabled: boolean;
  label: string;
  stateLabel: string;
  children: React.ReactElement<{ size?: number; "aria-hidden"?: boolean }>;
}) {
  const description = `${label}: ${stateLabel}`;

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className={enabled ? "is-enabled" : ""} aria-hidden="true">
          {children}
        </span>
      </TooltipTrigger>
      <TooltipContent>{description}</TooltipContent>
    </Tooltip>
  );
}
