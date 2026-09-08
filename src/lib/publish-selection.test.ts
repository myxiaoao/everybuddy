import { describe, expect, it } from "vitest";
import {
  buildPublishSources,
  findPublishSourceConflicts,
  resolvePublishSources,
} from "./publish-selection";
import type { GatewayProfile, ManagedModel } from "../types";

const gateways = ["a", "b"].map(
  (id) =>
    ({
      id,
      name: id,
      apiRoot: `https://${id}.example/v1`,
      createdAt: "now",
      updatedAt: "now",
    }) satisfies GatewayProfile,
);
const models = ["a", "b"].map(
  (gatewayId) =>
    ({ gatewayId, key: `${gatewayId}::same`, id: "same" }) as ManagedModel,
);

describe("publish source planning", () => {
  it("keeps independent selection and explicit removals across sources", () => {
    expect(
      buildPublishSources(
        models,
        new Set(["b::same"]),
        new Map([["a::same", false]]),
      ),
    ).toEqual([
      { gatewayId: "a", modelIds: [] },
      { gatewayId: "b", modelIds: ["same"] },
    ]);
  });
  it("finds duplicate IDs and preserves the losing source scope after resolution", () => {
    const sources = buildPublishSources(
      models,
      new Set(models.map((model) => model.key)),
      new Map(),
    );
    expect(findPublishSourceConflicts(sources, gateways)).toEqual([
      { modelId: "same", candidates: gateways },
    ]);
    expect(resolvePublishSources(sources, { same: "b" })).toEqual([
      { gatewayId: "a", modelIds: [] },
      { gatewayId: "b", modelIds: ["same"] },
    ]);
  });
});
