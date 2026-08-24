import { readFile } from "node:fs/promises";

const root = new URL("../", import.meta.url);
const [rustSource, typescriptSource] = await Promise.all([
  readFile(new URL("src-tauri/src/lib.rs", root), "utf8"),
  readFile(new URL("src/lib/api.ts", root), "utf8"),
]);

const handler = rustSource.match(
  /tauri::generate_handler!\[([\s\S]*?)\]\)/,
)?.[1];
if (!handler) throw new Error("Could not find the Tauri command registry");

const rustCommands = new Set(
  [...handler.matchAll(/commands::([a-z0-9_]+)/g)].map((match) => match[1]),
);
const typescriptCommands = new Set(
  [...typescriptSource.matchAll(/\bcall(?:<[^>]+>)?\(\s*"([a-z0-9_]+)"/g)].map(
    (match) => match[1],
  ),
);

const missingInTypescript = [...rustCommands].filter(
  (command) => !typescriptCommands.has(command),
);
const missingInRust = [...typescriptCommands].filter(
  (command) => !rustCommands.has(command),
);

if (missingInTypescript.length || missingInRust.length) {
  throw new Error(
    [
      missingInTypescript.length
        ? `Missing TypeScript commands: ${missingInTypescript.join(", ")}`
        : null,
      missingInRust.length
        ? `Missing Rust commands: ${missingInRust.join(", ")}`
        : null,
    ]
      .filter(Boolean)
      .join("\n"),
  );
}

console.log(`IPC command contract is aligned (${rustCommands.size} commands).`);
