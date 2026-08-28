import type { ManagedModel, TargetKind, TargetModelState } from "../types";

export interface ModelSelection {
  checkedKeys: Set<string>;
  indeterminateKeys: Set<string>;
  presentTargetsByKey: Map<string, TargetKind[]>;
  configuredKeys: Set<string>;
}

export function deriveModelSelection(
  models: ManagedModel[],
  targetStates: TargetModelState[],
  selectedTargets: TargetKind[],
  overrides: Map<string, boolean>,
): ModelSelection {
  const stateByTarget = new Map(
    targetStates.map((state) => [
      state.target,
      new Set(state.matchedModelKeys),
    ]),
  );
  const checkedKeys = new Set<string>();
  const indeterminateKeys = new Set<string>();
  const presentTargetsByKey = new Map<string, TargetKind[]>();
  const configuredKeys = new Set(
    targetStates.flatMap((state) => state.matchedModelKeys),
  );

  for (const model of models) {
    const presentTargets = selectedTargets.filter((target) =>
      stateByTarget.get(target)?.has(model.key),
    );
    presentTargetsByKey.set(model.key, presentTargets);
    const override = overrides.get(model.key);
    if (override === true) {
      checkedKeys.add(model.key);
      continue;
    }
    if (override === false || selectedTargets.length === 0) continue;
    if (presentTargets.length === selectedTargets.length) {
      checkedKeys.add(model.key);
    } else if (presentTargets.length > 0) {
      indeterminateKeys.add(model.key);
    }
  }

  return {
    checkedKeys,
    indeterminateKeys,
    presentTargetsByKey,
    configuredKeys,
  };
}
