import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  X,
} from "lucide-react";
import type { TargetImportReport, TargetKind } from "../types";
import type { createTranslator } from "../lib/i18n";
import { Button } from "@/components/ui/button";

export function ImportNotice({
  report,
  expanded,
  t,
  onToggle,
  onClose,
}: {
  report: TargetImportReport;
  expanded: boolean;
  t: ReturnType<typeof createTranslator>;
  onToggle: () => void;
  onClose: () => void;
}) {
  const hasIssues = report.issues.length > 0;
  return (
    <aside
      className={`import-notice${hasIssues ? "" : " is-success"}`}
      aria-labelledby="import-notice-title"
    >
      {hasIssues ? (
        <AlertTriangle aria-hidden="true" size={17} />
      ) : (
        <CheckCircle2 aria-hidden="true" size={17} />
      )}
      <div className="import-notice__content">
        <strong id="import-notice-title">
          {hasIssues ? t("importNoticeTitle") : t("importNoticeSuccessTitle")}
        </strong>
        {report.importedGatewayCount > 0 || report.importedModelCount > 0 ? (
          <span>
            {t("importSucceeded", {
              gateways: report.importedGatewayCount,
              models: report.importedModelCount,
            })}
          </span>
        ) : null}
        {hasIssues ? (
          <span>
            {t("importNoticeSummary", { count: report.issues.length })}
          </span>
        ) : null}
        {expanded ? (
          <ul>
            {report.issues.map((item, index) => (
              <li
                key={`${item.target}-${item.modelId ?? "target"}-${item.code}-${index}`}
              >
                <strong>
                  {item.modelId
                    ? `${displayTarget(item.target)} · ${item.modelId}`
                    : displayTarget(item.target)}
                </strong>
                <span>{importIssueLabel(item.code, t)}</span>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
      {hasIssues ? (
        <Button
          variant="ghost"
          size="sm"
          type="button"
          onClick={onToggle}
          aria-expanded={expanded}
        >
          {expanded ? (
            <ChevronUp aria-hidden="true" size={15} />
          ) : (
            <ChevronDown aria-hidden="true" size={15} />
          )}
          {expanded ? t("hideImportDetails") : t("viewImportDetails")}
        </Button>
      ) : null}
      <Button
        variant="ghost"
        size="icon-sm"
        type="button"
        onClick={onClose}
        aria-label={t("dismissImportNotice")}
      >
        <X aria-hidden="true" size={15} />
      </Button>
    </aside>
  );
}

function importIssueLabel(
  code: string,
  t: ReturnType<typeof createTranslator>,
) {
  const labels = {
    targetReadFailed: "importIssueTargetReadFailed",
    missingModelId: "importIssueMissingModelId",
    missingUrl: "importIssueMissingUrl",
    invalidUrl: "importIssueInvalidUrl",
    missingToken: "importIssueMissingToken",
    invalidParameters: "importIssueInvalidParameters",
    customProtocol: "importIssueCustomProtocol",
    ambiguousModel: "importIssueAmbiguousModel",
    ambiguousGateway: "importIssueAmbiguousGateway",
    targetConflict: "importIssueTargetConflict",
    modelConflict: "importIssueModelConflict",
  } as const;
  return t(labels[code as keyof typeof labels] ?? "importIssueUnknown");
}

function displayTarget(target: TargetKind) {
  return target === "workbuddy" ? "WorkBuddy" : "CodeBuddy";
}
