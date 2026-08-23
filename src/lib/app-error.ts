import type { createTranslator } from "./i18n";
import type { AppError } from "../types";

type Translator = ReturnType<typeof createTranslator>;

export function asAppError(error: unknown): AppError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error
  ) {
    return { code: String(error.code), message: String(error.message) };
  }
  return {
    code: "UNEXPECTED_ERROR",
    message: error instanceof Error ? error.message : String(error),
  };
}

export function localizedError(error: AppError, t: Translator) {
  switch (error.code) {
    case "AUTHENTICATION_ERROR":
      return {
        title: t("errorAuthenticationTitle"),
        message: t("errorAuthenticationMessage"),
        recovery: t("errorAuthenticationRecovery"),
      };
    case "NETWORK_ERROR":
      return {
        title: t("errorNetworkTitle"),
        message: t("errorNetworkMessage"),
        recovery: t("errorNetworkRecovery"),
      };
    case "PROTOCOL_ERROR":
      return {
        title: t("errorProtocolTitle"),
        message: t("errorProtocolMessage"),
        recovery: t("errorProtocolRecovery"),
      };
    case "TARGET_ERROR":
      return {
        title: t("errorTargetTitle"),
        message: t("errorTargetMessage"),
        recovery: t("errorTargetRecovery"),
      };
    case "DRIFT_ERROR":
      return {
        title: t("errorDriftTitle"),
        message: t("errorDriftMessage"),
        recovery: t("errorDriftRecovery"),
      };
    case "CONFLICT_ERROR":
      return {
        title: t("errorConflictTitle"),
        message: t("errorConflictMessage"),
        recovery: t("errorConflictRecovery"),
      };
    case "SECRET_STORE_ERROR":
      return {
        title: t("errorSecretTitle"),
        message: t("errorSecretMessage"),
        recovery: t("errorSecretRecovery"),
      };
    case "STORAGE_ERROR":
      return {
        title: t("errorStorageTitle"),
        message: t("errorStorageMessage"),
        recovery: t("errorStorageRecovery"),
      };
    case "VALIDATION_ERROR":
    case "VALIDATION":
      return {
        title: t("errorValidationTitle"),
        message: t("errorValidationMessage"),
        recovery: t("errorValidationRecovery"),
      };
    default:
      return {
        title: t("connectionError"),
        message: t("errorUnexpectedMessage"),
        recovery: t("errorUnexpectedRecovery"),
      };
  }
}
