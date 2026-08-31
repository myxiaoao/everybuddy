import { useId, useState, type FormEvent } from "react";
import {
  AlertTriangle,
  ArchiveRestore,
  ArrowRight,
  CheckCircle2,
  CircleX,
  Eye,
  EyeOff,
  FileJson,
  LoaderCircle,
  Plus,
  RefreshCw,
  RotateCcw,
  Sparkles,
} from "lucide-react";
import type {
  AppSettings,
  BackupRecord,
  GatewayInput,
  GatewayProfile,
  ManualModelInput,
  PublishPreview,
  PublishResult,
  TargetKind,
} from "../types";
import type { createTranslator } from "../lib/i18n";
import { Modal } from "./Modal";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { TargetIcon } from "./TargetIcon";
import type { UpdateCheckStatus } from "../hooks/use-app-updater";
import type { ErrorNoticeContent } from "./ErrorNotice";

interface CommonDialogProps {
  open: boolean;
  busy: boolean;
  t: ReturnType<typeof createTranslator>;
  errorNotice?: ErrorNoticeContent;
  onClose: () => void;
}

export function ConfirmationDialog({
  open,
  busy,
  title,
  description,
  confirmLabel,
  destructive = false,
  t,
  errorNotice,
  onClose,
  onConfirm,
}: CommonDialogProps & {
  title: string;
  description: string;
  confirmLabel: string;
  destructive?: boolean;
  onConfirm: () => void;
}) {
  return (
    <Modal
      open={open}
      title={title}
      description={description}
      closeLabel={t("close")}
      errorNotice={errorNotice}
      onClose={onClose}
      size="small"
      footer={
        <>
          <Button
            variant="secondary"
            type="button"
            onClick={onClose}
            disabled={busy}
            autoFocus
          >
            {t("cancel")}
          </Button>
          <Button
            variant={destructive ? "destructive" : "default"}
            type="button"
            onClick={onConfirm}
            disabled={busy}
          >
            {busy ? (
              <LoaderCircle className="spin" aria-hidden="true" size={17} />
            ) : null}
            {confirmLabel}
          </Button>
        </>
      }
    >
      <div className="confirmation-summary" aria-hidden="true">
        {destructive ? (
          <AlertTriangle size={22} />
        ) : (
          <ArchiveRestore size={22} />
        )}
      </div>
    </Modal>
  );
}

export function ManualModelDialog({
  open,
  busy,
  gateway,
  t,
  errorNotice,
  onClose,
  onSubmit,
}: CommonDialogProps & {
  gateway: GatewayProfile;
  onSubmit: (input: ManualModelInput) => void;
}) {
  const formId = useId();
  const vendorHintId = useId();
  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const [vendor, setVendor] = useState("");

  function submit(event: FormEvent) {
    event.preventDefault();
    onSubmit({ gatewayId: gateway.id, id, name, vendor });
  }

  return (
    <Modal
      open={open}
      title={t("addManualModel")}
      description={t("manualModelDescription", { gateway: gateway.name })}
      closeLabel={t("close")}
      errorNotice={errorNotice}
      onClose={onClose}
      footer={
        <>
          <Button variant="secondary" type="button" onClick={onClose}>
            {t("cancel")}
          </Button>
          <Button type="submit" form={formId} disabled={busy}>
            {busy ? (
              <LoaderCircle className="spin" aria-hidden="true" size={17} />
            ) : (
              <Plus aria-hidden="true" size={17} />
            )}
            {t("addModel")}
          </Button>
        </>
      }
    >
      <form id={formId} className="form-stack" onSubmit={submit}>
        <label>
          <span>{t("manualModelId")}</span>
          <Input
            required
            value={id}
            onChange={(event) => setId(event.currentTarget.value)}
            placeholder="gpt-5.6"
            autoFocus
            autoComplete="off"
            spellCheck="false"
            autoCapitalize="none"
          />
          <small>{t("manualModelIdHint")}</small>
        </label>
        <label>
          <span>{t("manualModelName")}</span>
          <Input
            value={name}
            onChange={(event) => setName(event.currentTarget.value)}
            placeholder={t("manualModelNamePlaceholder")}
            autoComplete="off"
          />
        </label>
        <label>
          <span>{t("manualModelVendor")}</span>
          <Input
            value={vendor}
            onChange={(event) => setVendor(event.currentTarget.value)}
            placeholder={t("manualModelVendorPlaceholder")}
            aria-describedby={vendorHintId}
            autoComplete="off"
            spellCheck="false"
            autoCapitalize="none"
          />
          <small id={vendorHintId}>{t("manualModelVendorHint")}</small>
        </label>
      </form>
    </Modal>
  );
}

export function GatewayDialog({
  open,
  busy,
  gateway,
  initialToken,
  t,
  errorNotice,
  onClose,
  onSubmit,
}: CommonDialogProps & {
  gateway: GatewayProfile | null;
  initialToken: string;
  onSubmit: (input: GatewayInput) => void;
}) {
  const formId = useId();
  const tokenInputId = useId();
  const [name, setName] = useState(gateway?.name ?? "");
  const [baseUrl, setBaseUrl] = useState(gateway?.apiRoot ?? "");
  const [token, setToken] = useState(initialToken);
  const [tokenVisible, setTokenVisible] = useState(false);

  function submit(event: FormEvent) {
    event.preventDefault();
    onSubmit({
      id: gateway?.id,
      name,
      baseUrl,
      ...(token.trim() !== initialToken ? { token: token.trim() } : {}),
    });
  }

  return (
    <Modal
      open={open}
      title={gateway ? t("editGateway") : t("addGateway")}
      closeLabel={t("close")}
      errorNotice={errorNotice}
      onClose={onClose}
      footer={
        <>
          <Button variant="secondary" type="button" onClick={onClose}>
            {t("cancel")}
          </Button>
          <Button type="submit" form={formId} disabled={busy}>
            {busy ? (
              <LoaderCircle className="spin" aria-hidden="true" size={17} />
            ) : (
              <ArrowRight aria-hidden="true" size={17} />
            )}
            {t("saveAndDiscover")}
          </Button>
        </>
      }
    >
      <form id={formId} className="form-stack" onSubmit={submit}>
        <label>
          <span>{t("gatewayName")}</span>
          <Input
            required
            value={name}
            onChange={(event) => setName(event.currentTarget.value)}
            autoFocus
            autoComplete="off"
          />
        </label>
        <label>
          <span>{t("gatewayUrl")}</span>
          <Input
            required
            value={baseUrl}
            onChange={(event) => setBaseUrl(event.currentTarget.value)}
            placeholder="https://api.example.com/v1"
            type="url"
            spellCheck="false"
            autoCapitalize="none"
          />
        </label>
        <div className="form-field">
          <label htmlFor={tokenInputId}>{t("gatewayToken")}</label>
          <div className="secret-input">
            <Input
              id={tokenInputId}
              required
              value={token}
              onChange={(event) => setToken(event.currentTarget.value)}
              placeholder="sk-..."
              type={tokenVisible ? "text" : "password"}
              autoComplete="off"
              spellCheck="false"
              aria-describedby={`${tokenInputId}-hint`}
            />
            <Button
              className="secret-input__toggle"
              variant="ghost"
              size="icon-sm"
              type="button"
              onClick={() => setTokenVisible((visible) => !visible)}
              aria-label={t(tokenVisible ? "hideToken" : "showToken")}
              aria-pressed={tokenVisible}
            >
              {tokenVisible ? (
                <EyeOff aria-hidden="true" size={17} />
              ) : (
                <Eye aria-hidden="true" size={17} />
              )}
            </Button>
          </div>
          <small id={`${tokenInputId}-hint`}>
            {t(gateway ? "gatewayTokenEditHint" : "gatewayTokenHint")}
          </small>
        </div>
      </form>
    </Modal>
  );
}

export function ProbeDialog({
  open,
  busy,
  t,
  errorNotice,
  onClose,
  onConfirm,
}: CommonDialogProps & { onConfirm: () => void }) {
  return (
    <Modal
      open={open}
      title={t("probeTitle")}
      description={t("probeBody")}
      closeLabel={t("close")}
      errorNotice={errorNotice}
      onClose={onClose}
      size="small"
      footer={
        <>
          <Button variant="secondary" type="button" onClick={onClose}>
            {t("cancel")}
          </Button>
          <Button type="button" onClick={onConfirm} disabled={busy}>
            {busy ? (
              <LoaderCircle className="spin" aria-hidden="true" size={17} />
            ) : (
              <Sparkles aria-hidden="true" size={17} />
            )}
            {t("runProbe")}
          </Button>
        </>
      }
    >
      <div className="probe-summary" aria-hidden="true">
        <span>{t("toolCall")}</span>
        <ArrowRight size={15} />
        <span>{t("images")}</span>
        <ArrowRight size={15} />
        <span>{t("reasoning")}</span>
      </div>
    </Modal>
  );
}

export function PublishDialog({
  open,
  busy,
  preview,
  result,
  t,
  errorNotice,
  onClose,
  onConfirm,
}: CommonDialogProps & {
  preview: PublishPreview | null;
  result: PublishResult | null;
  onConfirm: (acceptConflicts: boolean) => void;
}) {
  const [acceptConflicts, setAcceptConflicts] = useState(false);

  const hasConflicts = Boolean(preview?.conflicts.length);
  const canPublish = !hasConflicts || acceptConflicts;

  return (
    <Modal
      open={open}
      title={
        result
          ? result.success
            ? t("published")
            : t("publishFailed")
          : t("publishTitle")
      }
      closeLabel={t("close")}
      errorNotice={errorNotice}
      onClose={onClose}
      size="large"
      footer={
        result ? (
          <Button type="button" onClick={onClose}>
            {t("close")}
          </Button>
        ) : (
          <>
            <Button variant="secondary" type="button" onClick={onClose}>
              {t("cancel")}
            </Button>
            <Button
              type="button"
              onClick={() => onConfirm(acceptConflicts)}
              disabled={busy || !canPublish || !preview}
            >
              {busy ? (
                <LoaderCircle className="spin" aria-hidden="true" size={17} />
              ) : (
                <ArrowRight aria-hidden="true" size={17} />
              )}
              {t("confirmPublish", { count: preview?.targets.length ?? 0 })}
            </Button>
          </>
        )
      }
    >
      {result ? (
        <div className="publish-results">
          {result.results.map((item, index) => (
            <PublishResultRow
              item={item}
              t={t}
              key={`${item.target}-${index}`}
            />
          ))}
        </div>
      ) : preview ? (
        <div className="publish-preview">
          <div className="security-notice">
            <AlertTriangle aria-hidden="true" size={19} />
            <p>{t("publishTokenWarning")}</p>
          </div>
          <div className="preview-targets">
            {preview.targets.map((target) => (
              <section key={target.target}>
                <div className="preview-target__heading">
                  <TargetIcon target={target.target} />
                  <div>
                    <h3>{displayTarget(target.target)}</h3>
                    <code title={target.path}>{target.path}</code>
                  </div>
                </div>
                <div className="change-counts">
                  <span className="is-add">
                    {t("additions", { count: target.addCount })}
                  </span>
                  <span className="is-update">
                    {t("updates", { count: target.updateCount })}
                  </span>
                  {(target.removeCount ?? 0) > 0 ? (
                    <span className="is-update">
                      {t("remove")} {target.removeCount}
                    </span>
                  ) : null}
                  <span>
                    {t("unchanged", { count: target.unchangedCount })}
                  </span>
                </div>
              </section>
            ))}
          </div>
          {hasConflicts ? (
            <div className="conflict-block">
              <h3>
                <AlertTriangle aria-hidden="true" size={17} />
                {t("conflicts")}
              </h3>
              <ul>
                {preview.conflicts.map((conflict) => (
                  <li key={`${conflict.target}-${conflict.modelId}`}>
                    <code>{conflict.modelId}</code>
                    <span>{displayTarget(conflict.target)}</span>
                  </li>
                ))}
              </ul>
              <label className="confirmation-check">
                <Checkbox
                  checked={acceptConflicts}
                  onCheckedChange={(checked) =>
                    setAcceptConflicts(checked === true)
                  }
                />
                <span>{t("acceptConflicts")}</span>
              </label>
            </div>
          ) : null}
        </div>
      ) : (
        <div className="loading-inline">
          <LoaderCircle className="spin" aria-hidden="true" />
          {t("loading")}
        </div>
      )}
    </Modal>
  );
}

export function SettingsDialog({
  open,
  busy,
  settings,
  currentVersion,
  availableVersion,
  updateCheckStatus,
  installingUpdate,
  t,
  errorNotice,
  onClose,
  onSubmit,
  onCheckForUpdates,
  onInstallUpdate,
}: CommonDialogProps & {
  settings: AppSettings;
  currentVersion: string;
  availableVersion: string | null;
  updateCheckStatus: UpdateCheckStatus;
  installingUpdate: boolean;
  onSubmit: (settings: AppSettings) => void;
  onCheckForUpdates: () => void;
  onInstallUpdate: () => void;
}) {
  const formId = useId();
  const [draft, setDraft] = useState(settings);

  return (
    <Modal
      open={open}
      title={t("settings")}
      closeLabel={t("close")}
      errorNotice={errorNotice}
      onClose={onClose}
      footer={
        <>
          <Button variant="secondary" type="button" onClick={onClose}>
            {t("cancel")}
          </Button>
          <Button type="submit" form={formId} disabled={busy}>
            {t("save")}
          </Button>
        </>
      }
    >
      <form
        id={formId}
        className="settings-form"
        onSubmit={(event) => {
          event.preventDefault();
          onSubmit(draft);
        }}
      >
        <fieldset>
          <legend>{t("language")}</legend>
          <div className="segmented-control">
            <Segment
              label="简体中文"
              checked={draft.language === "zh-CN"}
              name="language"
              onChange={() => setDraft({ ...draft, language: "zh-CN" })}
            />
            <Segment
              label="English"
              checked={draft.language === "en"}
              name="language"
              onChange={() => setDraft({ ...draft, language: "en" })}
            />
          </div>
        </fieldset>
        <fieldset>
          <legend>{t("theme")}</legend>
          <div className="segmented-control segmented-control--three">
            <Segment
              label={t("themeSystem")}
              checked={draft.theme === "system"}
              name="theme"
              onChange={() => setDraft({ ...draft, theme: "system" })}
            />
            <Segment
              label={t("themeLight")}
              checked={draft.theme === "light"}
              name="theme"
              onChange={() => setDraft({ ...draft, theme: "light" })}
            />
            <Segment
              label={t("themeDark")}
              checked={draft.theme === "dark"}
              name="theme"
              onChange={() => setDraft({ ...draft, theme: "dark" })}
            />
          </div>
        </fieldset>
        <fieldset className="path-fields">
          <legend>{t("targetPaths")}</legend>
          {(["workbuddy", "codebuddy"] as const).map((target) => (
            <label key={target}>
              <span>{displayTarget(target)}</span>
              <Input
                required
                value={draft.targetPaths[target]}
                onChange={(event) =>
                  setDraft({
                    ...draft,
                    targetPaths: {
                      ...draft.targetPaths,
                      [target]: event.currentTarget.value,
                    },
                  })
                }
                spellCheck="false"
              />
            </label>
          ))}
        </fieldset>
        <fieldset className="app-information">
          <legend>{t("appInformation")}</legend>
          <div className="app-information__row">
            <span className="app-version">
              <span>{t("currentVersion")}</span>
              <strong>v{currentVersion}</strong>
            </span>
            <div className="app-information__actions">
              <Button
                variant="secondary"
                size="sm"
                type="button"
                onClick={onCheckForUpdates}
                disabled={updateCheckStatus === "checking" || installingUpdate}
              >
                <RefreshCw
                  className={updateCheckStatus === "checking" ? "spin" : ""}
                  aria-hidden="true"
                  size={16}
                />
                {t(
                  updateCheckStatus === "checking"
                    ? "checkingForUpdates"
                    : "checkForUpdates",
                )}
              </Button>
              {availableVersion ? (
                <Button
                  size="sm"
                  type="button"
                  onClick={onInstallUpdate}
                  disabled={installingUpdate}
                >
                  {installingUpdate ? (
                    <LoaderCircle
                      className="spin"
                      aria-hidden="true"
                      size={16}
                    />
                  ) : null}
                  {t("updateAndRestart")}
                </Button>
              ) : null}
            </div>
          </div>
          <div
            className="update-check-status"
            role="status"
            aria-live="polite"
            aria-atomic="true"
          >
            {updateStatusMessage(
              updateCheckStatus,
              currentVersion,
              availableVersion,
              t,
            )}
          </div>
        </fieldset>
      </form>
    </Modal>
  );
}

function updateStatusMessage(
  status: UpdateCheckStatus,
  currentVersion: string,
  availableVersion: string | null,
  t: ReturnType<typeof createTranslator>,
) {
  if (status === "checking") return t("checkingForUpdates");
  if (status === "latest") {
    return t(
      currentVersion.includes("-")
        ? "latestPrereleaseVersion"
        : "latestVersion",
    );
  }
  if (status === "available" && availableVersion) {
    return t("updateAvailable", { version: availableVersion });
  }
  if (status === "error") return t("updateCheckFailed");
  if (status === "desktop-required") return t("updateDesktopRequired");
  return "";
}

export function BackupsDialog({
  open,
  busy,
  backups,
  locale,
  t,
  errorNotice,
  onClose,
  onRestore,
}: CommonDialogProps & {
  backups: BackupRecord[];
  locale: string;
  onRestore: (backup: BackupRecord) => void;
}) {
  return (
    <Modal
      open={open}
      title={t("backups")}
      closeLabel={t("close")}
      errorNotice={errorNotice}
      onClose={onClose}
      size="large"
      footer={
        <Button type="button" onClick={onClose}>
          {t("close")}
        </Button>
      }
    >
      {backups.length ? (
        <div className="backup-list">
          {backups.map((backup) => (
            <div className="backup-row" key={backup.id}>
              <FileJson aria-hidden="true" size={19} />
              <span>
                <strong>{displayTarget(backup.target)}</strong>
                <small>
                  {new Date(backup.createdAt).toLocaleString(locale)}
                </small>
                <code title={backup.sourcePath}>{backup.sourcePath}</code>
              </span>
              <Button
                variant="secondary"
                type="button"
                onClick={() => onRestore(backup)}
                disabled={busy}
              >
                <ArchiveRestore aria-hidden="true" size={16} />
                {t("restore")}
              </Button>
            </div>
          ))}
        </div>
      ) : (
        <div className="empty-state">
          <div className="empty-state__glyph" aria-hidden="true">
            <ArchiveRestore size={24} />
          </div>
          <h2>{t("noBackups")}</h2>
        </div>
      )}
    </Modal>
  );
}

type PublishTargetResult = PublishResult["results"][number];
type PublishVisualState =
  "success" | "failure" | "rolled-back" | "rollback-failed";

function PublishResultRow({
  item,
  t,
}: {
  item: PublishTargetResult;
  t: ReturnType<typeof createTranslator>;
}) {
  const state: PublishVisualState = item.success
    ? "success"
    : item.rolledBack
      ? "rolled-back"
      : item.rollbackAttempted
        ? "rollback-failed"
        : "failure";
  const icon =
    state === "success" ? (
      <CheckCircle2 aria-hidden="true" size={20} />
    ) : state === "rolled-back" ? (
      <RotateCcw aria-hidden="true" size={20} />
    ) : state === "rollback-failed" ? (
      <CircleX aria-hidden="true" size={20} />
    ) : (
      <AlertTriangle aria-hidden="true" size={20} />
    );
  const status = {
    success: t("publishResultSuccess"),
    failure: t("publishResultFailure"),
    "rolled-back": t("publishResultRolledBack"),
    "rollback-failed": t("publishResultRollbackFailed"),
  }[state];

  return (
    <div className={`is-${state}`}>
      {icon}
      <span>
        <strong>{displayTarget(item.target)}</strong>
        <small>{status}</small>
      </span>
    </div>
  );
}

function Segment({
  label,
  checked,
  name,
  onChange,
}: {
  label: string;
  checked: boolean;
  name: string;
  onChange: () => void;
}) {
  return (
    <label>
      <input type="radio" name={name} checked={checked} onChange={onChange} />
      <span>{label}</span>
    </label>
  );
}

function displayTarget(target: TargetKind) {
  return target === "workbuddy" ? "WorkBuddy" : "CodeBuddy";
}
