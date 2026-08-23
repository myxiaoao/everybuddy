import { useCallback, useMemo, useState } from "react";
import { deriveModelSelection } from "@/lib/model-selection";
import type { ManagedModel, TargetKind, TargetModelState } from "@/types";

interface Options {
  models: ManagedModel[];
  gatewayModels: ManagedModel[];
  targetModelStates: TargetModelState[];
  selectedTargets: TargetKind[];
}

export function useModelSelection({
  models,
  gatewayModels,
  targetModelStates,
  selectedTargets,
}: Options) {
  const [overrides, setOverrides] = useState<Map<string, boolean>>(
    () => new Map(),
  );
  const selection = useMemo(
    () =>
      deriveModelSelection(
        models,
        targetModelStates,
        selectedTargets,
        overrides,
      ),
    [models, overrides, selectedTargets, targetModelStates],
  );
  const selectedKeys = selection.checkedKeys;
  const selectedModelCount = useMemo(
    () =>
      [...selectedKeys].filter((key) =>
        gatewayModels.some((model) => model.key === key),
      ).length,
    [gatewayModels, selectedKeys],
  );

  const toggleAll = useCallback(
    (visibleModels: ManagedModel[]) => {
      const allSelected = visibleModels.every((model) =>
        selectedKeys.has(model.key),
      );
      setOverrides((current) => {
        const next = new Map(current);
        for (const model of visibleModels) next.set(model.key, !allSelected);
        return next;
      });
    },
    [selectedKeys],
  );

  const clearSelection = useCallback(() => {
    setOverrides((current) => {
      const next = new Map(current);
      for (const model of gatewayModels) next.set(model.key, false);
      return next;
    });
  }, [gatewayModels]);

  const toggleModel = useCallback(
    (key: string) => {
      setOverrides((current) => {
        const next = new Map(current);
        next.set(key, !selectedKeys.has(key));
        return next;
      });
    },
    [selectedKeys],
  );

  const clearOverrides = useCallback((keys: Iterable<string>) => {
    setOverrides((current) => {
      const next = new Map(current);
      for (const key of keys) next.delete(key);
      return next;
    });
  }, []);

  return {
    selection,
    selectedKeys,
    selectedModelCount,
    toggleAll,
    clearSelection,
    toggleModel,
    clearOverrides,
  };
}
