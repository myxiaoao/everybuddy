import {
  error as writeError,
  warn as writeWarning,
} from "@tauri-apps/plugin-log";

const REDACTED = "[REDACTED]";
const MAX_LOG_LENGTH = 12_000;
const MAX_DEPTH = 4;
const MAX_ENTRIES = 32;
const SENSITIVE_KEYS = new Set([
  "apikey",
  "accesskey",
  "secretkey",
  "privatekey",
  "clientsecret",
  "token",
  "accesstoken",
  "refreshtoken",
  "idtoken",
  "authorization",
  "auth",
  "password",
  "passwd",
  "secret",
  "credential",
  "cookie",
]);

function isSensitiveKey(key: string) {
  const normalized = key
    .toLowerCase()
    .replace(/[^a-z0-9]/g, "")
    .replace(/s$/, "");
  return SENSITIVE_KEYS.has(normalized);
}

function looksLikeCredential(value: string) {
  const trimmed = value.trim();
  return (
    /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/i.test(trimmed) ||
    /^(?:sk-|github_pat_|gh[pousr]_|xox[baprs]-|ya29\.)[A-Za-z0-9._~+/-]{6,}$/i.test(
      trimmed,
    ) ||
    (trimmed.length >= 32 &&
      /^[A-Za-z0-9._~+/=-]+$/.test(trimmed) &&
      /[A-Za-z]/.test(trimmed) &&
      /\d/.test(trimmed))
  );
}

export function redactLogText(value: string) {
  return value
    .replace(/([?&][A-Za-z0-9_.~-]+)=([^&#\s"'<>]*)/g, `$1=${REDACTED}`)
    .replace(/(https?:\/\/)[^/@\s]+@/gi, `$1${REDACTED}@`)
    .replace(
      /(^|[\r\n])([ \t]*(?:(?:proxy-)?authorization|cookie|set-cookie|x-api-key|api-key)\s*[:=]\s*)[^\r\n]+/gim,
      `$1$2${REDACTED}`,
    )
    .replace(/\b(Bearer|Basic|Token|ApiKey)\s+[^\s"',}\]]+/gi, `$1 ${REDACTED}`)
    .replace(
      /((?:api[_-]?key|access[_-]?token|refresh[_-]?token|token|authorization|password|secret|cookie)\s*["']?\s*[:=]\s*["']?)([^\s"',}\]]+)/gi,
      `$1${REDACTED}`,
    )
    .replace(
      /\b(?:sk-|github_pat_|gh[pousr]_|xox[baprs]-|ya29\.)[A-Za-z0-9._~+/-]{6,}/gi,
      REDACTED,
    );
}

function normalizeValue(
  value: unknown,
  depth: number,
  ancestors: WeakSet<object>,
): unknown {
  if (typeof value === "string") {
    return looksLikeCredential(value) ? REDACTED : value.slice(0, 2_000);
  }
  if (
    value === null ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return value;
  }
  if (typeof value !== "object") {
    return String(value);
  }
  if (value instanceof Error) {
    return {
      name: value.name,
      message: normalizeValue(value.message, depth + 1, ancestors),
      stack: normalizeValue(value.stack ?? "", depth + 1, ancestors),
    };
  }
  if (ancestors.has(value)) return "[Circular]";
  if (depth >= MAX_DEPTH) return "[Maximum depth reached]";

  ancestors.add(value);
  try {
    if (Array.isArray(value)) {
      return value
        .slice(0, MAX_ENTRIES)
        .map((entry) => normalizeValue(entry, depth + 1, ancestors));
    }

    return Object.fromEntries(
      Object.entries(value)
        .slice(0, MAX_ENTRIES)
        .map(([key, entry]) => [
          key,
          isSensitiveKey(key)
            ? REDACTED
            : normalizeValue(entry, depth + 1, ancestors),
        ]),
    );
  } finally {
    ancestors.delete(value);
  }
}

export function formatFrontendLog(
  context: string,
  value: unknown,
  details?: string,
) {
  let serialized: string;
  try {
    serialized =
      JSON.stringify(normalizeValue(value, 0, new WeakSet())) ?? String(value);
  } catch {
    serialized = "[Unserializable value]";
  }
  const message = [`[frontend] ${context}`, serialized, details]
    .filter(Boolean)
    .join("\n");
  return redactLogText(message).slice(0, MAX_LOG_LENGTH);
}

function persistLog(
  writer: (message: string, options?: { file?: string }) => Promise<void>,
  context: string,
  value: unknown,
  details?: string,
) {
  try {
    void writer(formatFrontendLog(context, value, details), {
      file: "frontend",
    }).catch(() => undefined);
  } catch {
    // Logging must never create a second application error.
  }
}

export function reportFrontendError(
  context: string,
  value: unknown,
  details?: string,
) {
  persistLog(writeError, context, value, details);
}

export function reportFrontendWarning(
  context: string,
  value: unknown,
  details?: string,
) {
  persistLog(writeWarning, context, value, details);
}

export function installGlobalErrorHandlers(target: Window = window) {
  const handleError = (event: ErrorEvent) => {
    const location = event.filename
      ? `${event.filename}:${event.lineno}:${event.colno}`
      : undefined;
    reportFrontendError("window.error", event.error ?? event.message, location);
  };
  const handleRejection = (event: PromiseRejectionEvent) => {
    reportFrontendError("window.unhandledrejection", event.reason);
  };

  target.addEventListener("error", handleError);
  target.addEventListener("unhandledrejection", handleRejection);
  return () => {
    target.removeEventListener("error", handleError);
    target.removeEventListener("unhandledrejection", handleRejection);
  };
}
