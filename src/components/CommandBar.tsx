import {
  ArrowLeft,
  Boxes,
  Cable,
  ChevronRight,
  LoaderCircle,
  RefreshCw,
  SlidersHorizontal,
  Upload,
} from "lucide-react";
import type { GatewayProfile } from "../types";
import type { createTranslator } from "../lib/i18n";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

export type WorkspaceView = "gateways" | "models" | "details";

interface CommandBarProps {
  gateway: GatewayProfile | null;
  modelCount: number;
  selectedModelCount: number;
  selectedTargetCount: number;
  view: WorkspaceView;
  refreshing: boolean;
  busy: boolean;
  t: ReturnType<typeof createTranslator>;
  onNavigate: (view: WorkspaceView) => void;
  onBack: () => void;
  onRefresh: () => void;
  onPublish: () => void;
}

export function CommandBar({
  gateway,
  modelCount,
  selectedModelCount,
  selectedTargetCount,
  view,
  refreshing,
  busy,
  t,
  onNavigate,
  onBack,
  onRefresh,
  onPublish,
}: CommandBarProps) {
  const canPublish = selectedModelCount > 0 && selectedTargetCount > 0;
  const currentStage = {
    gateways: {
      label: t("apiStage"),
      detail: gateway?.name ?? t("addGateway"),
    },
    models: {
      label: t("modelStage"),
      detail: t("discoveredCount", { count: modelCount }),
    },
    details: {
      label: t("targetStage"),
      detail: t("selectedTargetsCount", { count: selectedTargetCount }),
    },
  }[view];

  return (
    <header className="command-bar" aria-label={t("configurationProgress")}>
      <div className="command-context">
        <Button
          variant="ghost"
          size="icon"
          className="compact-back"
          type="button"
          onClick={onBack}
          aria-label={t("back")}
        >
          <ArrowLeft aria-hidden="true" size={18} />
        </Button>

        <div className="command-mobile-context" aria-live="polite">
          <strong>{currentStage.label}</strong>
          <small>{currentStage.detail}</small>
        </div>

        <nav className="command-stages" aria-label={t("configurationProgress")}>
          <CommandStage
            icon={<Cable />}
            label={t("apiStage")}
            detail={gateway?.name ?? t("addGateway")}
            active={view === "gateways"}
            complete={Boolean(gateway)}
            onClick={() => onNavigate("gateways")}
          />
          <ChevronRight
            className="command-separator"
            aria-hidden="true"
            size={15}
          />
          <CommandStage
            icon={<Boxes />}
            label={t("modelStage")}
            detail={t("discoveredCount", { count: modelCount })}
            active={view === "models"}
            complete={modelCount > 0}
            disabled={!gateway}
            onClick={() => onNavigate("models")}
          />
          <ChevronRight
            className="command-separator"
            aria-hidden="true"
            size={15}
          />
          <CommandStage
            icon={<SlidersHorizontal />}
            label={t("targetStage")}
            detail={t("selectedTargetsCount", { count: selectedTargetCount })}
            active={view === "details"}
            complete={canPublish}
            disabled={selectedModelCount === 0}
            onClick={() => onNavigate("details")}
          />
        </nav>
      </div>

      <div className="command-actions">
        {gateway ? (
          <>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="command-refresh"
                  type="button"
                  onClick={onRefresh}
                  disabled={refreshing}
                  aria-label={t("refreshModels")}
                >
                  {refreshing ? (
                    <LoaderCircle
                      className="spin"
                      aria-hidden="true"
                      size={17}
                    />
                  ) : (
                    <RefreshCw aria-hidden="true" size={17} />
                  )}
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t("refreshModels")}</TooltipContent>
            </Tooltip>
            <Button
              className="command-primary"
              type="button"
              onClick={onPublish}
              disabled={busy || !canPublish}
            >
              <Upload aria-hidden="true" size={16} />
              <span className="command-primary__label">{t("publish")}</span>
              <span className="command-primary__label--compact">
                {t("publishShort")}
              </span>
              <span className="command-primary__count" aria-hidden="true">
                {selectedModelCount}
              </span>
            </Button>
          </>
        ) : null}
      </div>
    </header>
  );
}

function CommandStage({
  icon,
  label,
  detail,
  active,
  complete,
  disabled,
  onClick,
}: {
  icon: React.ReactElement;
  label: string;
  detail: string;
  active: boolean;
  complete: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      className={`command-stage${active ? " is-current" : ""}${complete ? " is-complete" : ""}`}
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-current={active ? "step" : undefined}
    >
      <span className="command-stage__icon" aria-hidden="true">
        {icon}
      </span>
      <span className="command-stage__copy">
        <strong>{label}</strong>
        <small>{detail}</small>
      </span>
    </button>
  );
}
