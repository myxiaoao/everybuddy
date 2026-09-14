import type {
  GatewayProfile,
  ManagedModel,
  PublishSourceSelection,
} from "../types";

export interface PublishSourceConflict {
  modelId: string;
  candidates: GatewayProfile[];
}

export function buildPublishSources(
  models: ManagedModel[],
  checkedKeys: Set<string>,
  overrides: Map<string, boolean>,
): PublishSourceSelection[] {
  const sources = new Map<string, PublishSourceSelection>();
  for (const model of models) {
    if (!checkedKeys.has(model.key) && !overrides.has(model.key)) continue;
    const source = sources.get(model.gatewayId) ?? {
      gatewayId: model.gatewayId,
      modelIds: [],
    };
    if (checkedKeys.has(model.key)) source.modelIds.push(model.id);
    sources.set(model.gatewayId, source);
  }
  return [...sources.values()].sort((a, b) =>
    a.gatewayId.localeCompare(b.gatewayId),
  );
}

export function findPublishSourceConflicts(
  sources: PublishSourceSelection[],
  gateways: GatewayProfile[],
): PublishSourceConflict[] {
  const candidates = new Map<string, GatewayProfile[]>();
  const gatewayById = new Map(gateways.map((gateway) => [gateway.id, gateway]));
  for (const source of sources) {
    const gateway = gatewayById.get(source.gatewayId);
    if (!gateway) continue;
    for (const id of source.modelIds)
      candidates.set(id, [...(candidates.get(id) ?? []), gateway]);
  }
  return [...candidates]
    .filter(([, options]) => options.length > 1)
    .map(([modelId, options]) => ({ modelId, candidates: options }));
}

export function resolvePublishSources(
  sources: PublishSourceSelection[],
  choices: Record<string, string>,
): PublishSourceSelection[] {
  return sources.map((source) => ({
    ...source,
    modelIds: source.modelIds.filter(
      (id) =>
        !Object.prototype.hasOwnProperty.call(choices, id) ||
        choices[id] === source.gatewayId,
    ),
  }));
}
