import {
  ArchiveRestore,
  Circle,
  CircleCheck,
  CircleX,
  ChevronRight,
  LoaderCircle,
  Pencil,
  Plus,
  RefreshCw,
  Settings,
  Trash2,
} from "lucide-react";
import type { GatewayConnectionState, GatewayProfile } from "../types";
import type { createTranslator } from "../lib/i18n";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import everyBuddyIcon from "@/assets/everybuddy-icon-v6.png";

interface GatewaySidebarProps {
  currentVersion: string;
  gateways: GatewayProfile[];
  selectedId: string | null;
  disabled: boolean;
  refreshingIds: ReadonlySet<string>;
  connectionStates: Record<string, GatewayConnectionState>;
  t: ReturnType<typeof createTranslator>;
  onSelect: (id: string) => void;
  onAdd: () => void;
  onEdit: (gateway: GatewayProfile) => void;
  onRefresh: (id: string) => void;
  onDelete: (gateway: GatewayProfile) => void;
  onOpenSettings: () => void;
  onOpenBackups: () => void;
}

export function GatewaySidebar({
  currentVersion,
  gateways,
  selectedId,
  disabled,
  refreshingIds,
  connectionStates,
  t,
  onSelect,
  onAdd,
  onEdit,
  onRefresh,
  onDelete,
  onOpenSettings,
  onOpenBackups,
}: GatewaySidebarProps) {
  return (
    <aside className="gateway-panel" aria-label={t("gateways")}>
      <div className="brand-lockup">
        <div className="brand-mark" aria-hidden="true">
          <img src={everyBuddyIcon} alt="" />
        </div>
        <div>
          <div className="brand-lockup__title">
            <strong>{t("appName")}</strong>
            <small>v{currentVersion}</small>
          </div>
          <span>{t("appTagline")}</span>
        </div>
      </div>

      <div className="panel-heading">
        <div>
          <h2>{t("gateways")}</h2>
          <span>{gateways.length}</span>
        </div>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              className="gateway-add-button"
              variant="secondary"
              size="sm"
              type="button"
              onClick={onAdd}
              aria-label={t("addGateway")}
              disabled={disabled}
            >
              <Plus aria-hidden="true" size={14} />
              <span>{t("addGatewayShort")}</span>
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t("addGateway")}</TooltipContent>
        </Tooltip>
      </div>

      <div className="gateway-list">
        {gateways.map((gateway) => {
          const selected = gateway.id === selectedId;
          const busy = refreshingIds.has(gateway.id);
          const connectionState = busy
            ? "refreshing"
            : (connectionStates[gateway.id] ?? "idle");
          const connectionLabel = gatewayConnectionLabel(connectionState, t);
          return (
            <div
              className={`gateway-item${selected ? " is-selected" : ""}`}
              key={gateway.id}
            >
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    className="gateway-item__main"
                    type="button"
                    onClick={() => onSelect(gateway.id)}
                    disabled={disabled}
                    aria-current={selected ? "true" : undefined}
                    aria-label={`${gateway.name}, ${gateway.apiRoot}, ${connectionLabel}`}
                  >
                    <GatewayStatusIcon
                      state={connectionState}
                      label={connectionLabel}
                    />
                    <span>
                      <strong>{gateway.name}</strong>
                      <small>{gateway.apiRoot}</small>
                    </span>
                    <ChevronRight aria-hidden="true" size={16} />
                  </button>
                </TooltipTrigger>
                <TooltipContent side="right">
                  <span className="gateway-url-tooltip">{gateway.apiRoot}</span>
                </TooltipContent>
              </Tooltip>
              {selected ? (
                <div className="gateway-item__actions">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        type="button"
                        onClick={() => onRefresh(gateway.id)}
                        aria-label={t("refreshModels")}
                        disabled={disabled || busy}
                      >
                        <RefreshCw
                          aria-hidden="true"
                          size={15}
                          className={busy ? "spin" : undefined}
                        />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>{t("refreshModels")}</TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        type="button"
                        onClick={() => onEdit(gateway)}
                        aria-label={t("editGateway")}
                        disabled={disabled || busy}
                      >
                        <Pencil aria-hidden="true" size={15} />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>{t("editGateway")}</TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="destructive"
                        size="icon-sm"
                        type="button"
                        onClick={() => onDelete(gateway)}
                        aria-label={t("remove")}
                        disabled={disabled || busy}
                      >
                        <Trash2 aria-hidden="true" size={15} />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>{t("remove")}</TooltipContent>
                  </Tooltip>
                </div>
              ) : null}
            </div>
          );
        })}
      </div>

      <div className="sidebar-actions">
        <Button
          className="sidebar-action-button"
          variant="secondary"
          type="button"
          onClick={onOpenBackups}
          disabled={disabled}
        >
          <ArchiveRestore aria-hidden="true" size={17} />
          {t("backups")}
        </Button>
        <Button
          className="sidebar-action-button"
          variant="secondary"
          type="button"
          onClick={onOpenSettings}
          disabled={disabled}
        >
          <Settings aria-hidden="true" size={17} />
          {t("settings")}
        </Button>
      </div>
    </aside>
  );
}

function GatewayStatusIcon({
  state,
  label,
}: {
  state: GatewayConnectionState;
  label: string;
}) {
  const icon = {
    idle: <Circle />,
    refreshing: <LoaderCircle className="spin" />,
    connected: <CircleCheck />,
    error: <CircleX />,
  }[state];

  return (
    <span
      className={`gateway-status-icon is-${state}`}
      title={label}
      aria-hidden="true"
    >
      {icon}
    </span>
  );
}

function gatewayConnectionLabel(
  state: GatewayConnectionState,
  t: ReturnType<typeof createTranslator>,
) {
  if (state === "refreshing") return t("gatewayRefreshing");
  if (state === "connected") return t("gatewayConnected");
  if (state === "error") return t("gatewayConnectionFailed");
  return t("gatewayNotChecked");
}
