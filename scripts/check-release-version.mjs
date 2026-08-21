import { readFile } from "node:fs/promises";
import process from "node:process";

const root = new URL("../", import.meta.url);

async function readJson(path) {
  return JSON.parse(await readFile(new URL(path, root), "utf8"));
}

function cargoVersion(source) {
  const match = source.match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) {
    throw new Error("Could not read the package version from src-tauri/Cargo.toml");
  }
  return match[1];
}

const [packageJson, tauriConfig, cargoToml, changelog] = await Promise.all([
  readJson("package.json"),
  readJson("src-tauri/tauri.conf.json"),
  readFile(new URL("src-tauri/Cargo.toml", root), "utf8"),
  readFile(new URL("CHANGELOG.md", root), "utf8"),
]);

const versions = new Map([
  ["package.json", packageJson.version],
  ["src-tauri/tauri.conf.json", tauriConfig.version],
  ["src-tauri/Cargo.toml", cargoVersion(cargoToml)],
]);
const expected = packageJson.version;
const mismatches = [...versions].filter(([, version]) => version !== expected);
if (mismatches.length > 0) {
  throw new Error(
    `Version mismatch: ${mismatches.map(([file, version]) => `${file}=${version}`).join(", ")}`,
  );
}

const tag = process.argv[2] ?? process.env.GITHUB_REF_NAME;
if (tag && tag !== `v${expected}`) {
  throw new Error(`Release tag ${tag} does not match version v${expected}`);
}
if (!changelog.includes(`## [${expected}]`)) {
  throw new Error(`CHANGELOG.md does not contain a ${expected} release entry`);
}

console.log(`Release versions are aligned at ${expected}${tag ? ` (${tag})` : ""}.`);
