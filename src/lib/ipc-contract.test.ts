import { describe, expect, it } from "vitest";
import fixture from "../../src-tauri/tests/fixtures/bootstrap-contract.json";
import type { BootstrapData } from "../types";

describe("IPC serialization contract", () => {
  it("keeps the bootstrap response keys expected by TypeScript", () => {
    const keys: Array<keyof BootstrapData> = [
      "gateways",
      "models",
      "targets",
      "targetModelStates",
      "importReport",
      "settings",
    ];

    expect(Object.keys(fixture).sort()).toEqual(keys.sort());
    expect(fixture.settings.targetPaths).toEqual({
      workbuddy: "/home/test/.workbuddy/models.json",
      codebuddy: "/home/test/.codebuddy/models.json",
    });
  });
});
