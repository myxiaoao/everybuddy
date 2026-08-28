import { describe, expect, it } from "vitest";
import type { ManagedModel, TargetModelState } from "../types";
import { deriveModelSelection } from "./model-selection";

const models = [
  { key: "gateway::all", gatewayId: "gateway", id: "all" },
  { key: "gateway::partial", gatewayId: "gateway", id: "partial" },
  { key: "gateway::none", gatewayId: "gateway", id: "none" },
] as ManagedModel[];

const states: TargetModelState[] = [
  {
    target: "workbuddy",
    fingerprint: "work",
    matchedModelKeys: ["gateway::all", "gateway::partial"],
    unmatchedCount: 0,
    skippedCount: 0,
  },
  {
    target: "codebuddy",
    fingerprint: "code",
    matchedModelKeys: ["gateway::all"],
    unmatchedCount: 0,
    skippedCount: 0,
  },
];

describe("deriveModelSelection", () => {
  it("derives checked, indeterminate, and unchecked states from selected targets", () => {
    const selection = deriveModelSelection(
      models,
      states,
      ["workbuddy", "codebuddy"],
      new Map(),
    );

    expect([...selection.checkedKeys]).toEqual(["gateway::all"]);
    expect([...selection.indeterminateKeys]).toEqual(["gateway::partial"]);
    expect(selection.checkedKeys.has("gateway::none")).toBe(false);
    expect(selection.presentTargetsByKey.get("gateway::partial")).toEqual([
      "workbuddy",
    ]);
    expect([...selection.configuredKeys]).toEqual([
      "gateway::all",
      "gateway::partial",
    ]);
  });

  it("derives configured models from every target regardless of publish selection", () => {
    const selection = deriveModelSelection(models, states, [], new Map());

    expect([...selection.checkedKeys]).toEqual([]);
    expect([...selection.configuredKeys]).toEqual([
      "gateway::all",
      "gateway::partial",
    ]);
  });

  it("keeps explicit overrides when target scope changes", () => {
    const overrides = new Map([
      ["gateway::partial", true],
      ["gateway::all", false],
    ]);

    const bothTargets = deriveModelSelection(
      models,
      states,
      ["workbuddy", "codebuddy"],
      overrides,
    );
    const workbuddyOnly = deriveModelSelection(
      models,
      states,
      ["workbuddy"],
      overrides,
    );

    expect([...bothTargets.checkedKeys]).toEqual(["gateway::partial"]);
    expect([...bothTargets.indeterminateKeys]).toEqual([]);
    expect([...workbuddyOnly.checkedKeys]).toEqual(["gateway::partial"]);
    expect(workbuddyOnly.checkedKeys.has("gateway::all")).toBe(false);
  });
});
