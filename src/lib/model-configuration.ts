import type { ModelConfiguration } from "../types";

export function isValidModelConfiguration(
  configuration: ModelConfiguration,
): boolean {
  return (
    isValidTokenLimit(configuration.maxInputTokens) &&
    isValidTokenLimit(configuration.maxOutputTokens) &&
    (!configuration.useCustomProtocol ||
      Boolean(configuration.endpointOverride?.trim())) &&
    (configuration.temperature === null ||
      (Number.isFinite(configuration.temperature) &&
        configuration.temperature >= 0)) &&
    (configuration.reasoning.summary === null ||
      ["auto", "concise", "detailed"].includes(configuration.reasoning.summary))
  );
}

function isValidTokenLimit(value: number | null): boolean {
  return value === null || (Number.isSafeInteger(value) && value > 0);
}
