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
    case "CREDENTIAL_ERROR":
      return {
        title: t("errorCredentialTitle"),
        message: t("errorCredentialMessage"),
        recovery: t("errorCredentialRecovery"),
      };
    case "STORAGE_ERROR":
      return {
        title: t("errorStorageTitle"),
        message: t("errorStorageMessage"),
        recovery: t("errorStorageRecovery"),
      };
    case "VALIDATION_ERROR":
    case "VALIDATION":
      return localizedValidationError(error.message, t);
    default:
      return {
        title: t("connectionError"),
        message: t("errorUnexpectedMessage"),
        recovery: t("errorUnexpectedRecovery"),
      };
  }
}

function localizedValidationError(message: string, t: Translator) {
  const categories: Array<{
    messages: string[];
    title: Parameters<Translator>[0];
    body: Parameters<Translator>[0];
    recovery: Parameters<Translator>[0];
  }> = [
    {
      messages: ["This model ID already exists in the selected API source"],
      title: "errorDuplicateModelTitle",
      body: "errorDuplicateModelMessage",
      recovery: "errorDuplicateModelRecovery",
    },
    {
      messages: ["This model is not available in the OpenRouter model catalog"],
      title: "errorOpenRouterModelTitle",
      body: "errorOpenRouterModelMessage",
      recovery: "errorOpenRouterModelRecovery",
    },
    {
      messages: [
        "Enter a valid HTTP or HTTPS API URL",
        "Only HTTP and HTTPS gateway URLs are supported",
        "Remote gateway URLs must use HTTPS",
        "Credentials are not allowed inside the gateway URL",
        "Gateway URLs cannot contain a query or fragment",
      ],
      title: "errorApiUrlTitle",
      body: "errorApiUrlMessage",
      recovery: "errorApiUrlRecovery",
    },
    {
      messages: [
        "Select at least one model to publish",
        "Select WorkBuddy, CodeBuddy, or both",
        "A configuration target can only be selected once",
      ],
      title: "errorPublishSelectionTitle",
      body: "errorPublishSelectionMessage",
      recovery: "errorPublishSelectionRecovery",
    },
    {
      messages: [
        "Temperature must be a finite number",
        "Reasoning effort and default effort must be included in supported efforts",
        "Model name and vendor are required",
      ],
      title: "errorModelConfigTitle",
      body: "errorModelConfigMessage",
      recovery: "errorModelConfigRecovery",
    },
    {
      messages: [
        "Gateway profile not found",
        "Model not found",
        "One or more selected models no longer exist in this gateway",
        "Backup not found",
      ],
      title: "errorStaleDataTitle",
      body: "errorStaleDataMessage",
      recovery: "errorStaleDataRecovery",
    },
    {
      messages: [
        "Unsupported interface language",
        "Unsupported theme",
        "Configuration target paths cannot be empty",
      ],
      title: "errorSettingsValidationTitle",
      body: "errorSettingsValidationMessage",
      recovery: "errorSettingsValidationRecovery",
    },
    {
      messages: [
        "Gateway name is required",
        "API token is required",
        "Model ID is required",
      ],
      title: "errorRequiredFieldTitle",
      body: "errorRequiredFieldMessage",
      recovery: "errorRequiredFieldRecovery",
    },
  ];
  const category = categories.find((item) => item.messages.includes(message));

  return category
    ? {
        title: t(category.title),
        message: t(category.body),
        recovery: t(category.recovery),
      }
    : {
        title: t("errorValidationTitle"),
        message: t("errorValidationMessage"),
        recovery: t("errorValidationRecovery"),
      };
}
