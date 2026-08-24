import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

const output = process.argv[2] ?? "src-tauri/tauri.release.conf.json";
const publicKey = process.env.TAURI_UPDATER_PUBLIC_KEY?.trim();
if (!publicKey) {
  throw new Error("TAURI_UPDATER_PUBLIC_KEY is required");
}

const config = {
  bundle: {
    createUpdaterArtifacts: true,
  },
  plugins: {
    updater: {
      endpoints: [
        "https://github.com/myxiaoao/everybuddy/releases/latest/download/latest.json",
      ],
      pubkey: publicKey,
    },
  },
};

await mkdir(path.dirname(output), { recursive: true });
await writeFile(output, `${JSON.stringify(config, null, 2)}\n`, {
  mode: 0o600,
});
console.log(`Prepared ${output}.`);
