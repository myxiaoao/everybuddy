import {
  ArchiveRestore,
  ChevronRight,
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
  gateways: GatewayProfile[];
  selectedId: string | null;
  busyId: string | null;
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
  gateways,
  selectedId,
  busyId,
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
  const refreshing = busyId !== null;
  return (
    <aside className="gateway-panel" aria-label={t("gateways")}>
      <div className="brand-lockup">
        <div className="brand-mark" aria-hidden="true">
          <img src={everyBuddyIcon} alt="" />
        </div>
        <div>
          <strong>{t("appName")}</strong>
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
              variant="ghost"
              size="icon"
              type="button"
              onClick={onAdd}
              aria-label={t("addGateway")}
              disabled={refreshing}
            >
              <Plus aria-hidden="true" size={18} />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t("addGateway")}</TooltipContent>
        </Tooltip>
      </div>

      <div className="gateway-list">
        {gateways.map((gateway) => {
          const selected = gateway.id === selectedId;
          const busy = gateway.id === busyId;
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
                    aria-current={selected ? "true" : undefined}
                    aria-label={`${gateway.name}, ${gateway.apiRoot}, ${connectionLabel}`}
                  >
                    <span
                      className={`status-dot is-${connectionState}${busy ? " is-pulsing" : ""}`}
                      title={connectionLabel}
                      aria-hidden="true"
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
                        disabled={refreshing}
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
                        disabled={refreshing}
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
                        disabled={refreshing}
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
        >
          <ArchiveRestore aria-hidden="true" size={17} />
          {t("backups")}
        </Button>
        <Button
          className="sidebar-action-button"
          variant="secondary"
          type="button"
          onClick={onOpenSettings}
        >
          <Settings aria-hidden="true" size={17} />
          {t("settings")}
        </Button>
      </div>
    </aside>
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
