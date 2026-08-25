import type { ModelConfiguration } from "../types";

export function isValidModelConfiguration(
  configuration: ModelConfiguration,
): boolean {
  return (
    isValidTokenLimit(configuration.maxInputTokens) &&
    isValidTokenLimit(configuration.maxOutputTokens) &&
    (configuration.temperature === null ||
      (Number.isFinite(configuration.temperature) &&
        configuration.temperature >= 0))
  );
}

function isValidTokenLimit(value: number | null): boolean {
  return value === null || (Number.isSafeInteger(value) && value > 0);
}
