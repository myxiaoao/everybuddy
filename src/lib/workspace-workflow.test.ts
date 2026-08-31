import { describe, expect, it } from "vitest";
import {
  initialWorkspaceWorkflow,
  isWorkspaceBusy,
  workspaceWorkflowReducer,
} from "./workspace-workflow";
import type { PreparePublishRequest, PublishPreview } from "../types";

describe("workspace workflow", () => {
  it("keeps the workspace busy until every concurrent operation finishes", () => {
    let state = workspaceWorkflowReducer(initialWorkspaceWorkflow, {
      type: "operationStarted",
    });
    state = workspaceWorkflowReducer(state, { type: "operationStarted" });
    state = workspaceWorkflowReducer(state, { type: "operationFinished" });

    expect(isWorkspaceBusy(state)).toBe(true);

    state = workspaceWorkflowReducer(state, { type: "operationFinished" });
    expect(isWorkspaceBusy(state)).toBe(false);
  });

  it("owns dirty edit and discard confirmation transitions", () => {
    let state = workspaceWorkflowReducer(initialWorkspaceWorkflow, {
      type: "dirtyChanged",
      modelKey: "gateway::model",
    });
    state = workspaceWorkflowReducer(state, { type: "discardRequested" });

    expect(state).toMatchObject({
      dirtyModelKey: "gateway::model",
      discardOpen: true,
    });

    state = workspaceWorkflowReducer(state, { type: "discardConfirmed" });
    expect(state).toMatchObject({ dirtyModelKey: null, discardOpen: false });
  });

  it("owns the complete publish workflow", () => {
    const request: PreparePublishRequest = {
      gatewayId: "gateway",
      modelIds: ["model"],
      targets: ["workbuddy"],
    };
    const preview: PublishPreview = {
      targets: [],
      conflicts: [],
      warnings: [],
      gatewayRevision: "gateway-revision",
      credentialRevision: "credential-revision",
      modelRevisions: [],
    };
    let state = workspaceWorkflowReducer(initialWorkspaceWorkflow, {
      type: "publishPreviewRequested",
      sessionId: 1,
      request,
    });
    expect(state).toMatchObject({
      publishPhase: "previewing",
      publishRequest: request,
    });

    state = workspaceWorkflowReducer(state, {
      type: "publishPreviewLoaded",
      sessionId: 1,
      preview,
    });
    expect(state).toMatchObject({
      publishPhase: "ready",
      publishPreview: preview,
    });

    state = workspaceWorkflowReducer(state, {
      type: "publishExecutionStarted",
      sessionId: 1,
    });
    expect(state.publishPhase).toBe("publishing");

    state = workspaceWorkflowReducer(state, {
      type: "publishExecutionFinished",
      sessionId: 1,
      result: { success: true, results: [] },
    });
    expect(state).toMatchObject({
      publishPhase: "result",
      publishResult: { success: true },
    });

    state = workspaceWorkflowReducer(state, { type: "publishClosed" });
    expect(state).toMatchObject({
      publishPhase: "closed",
      publishRequest: null,
      publishPreview: null,
      publishResult: null,
    });
  });

  it("ignores a publish preview that arrives after the dialog closes", () => {
    const request: PreparePublishRequest = {
      gatewayId: "gateway",
      modelIds: ["model"],
      targets: ["workbuddy"],
    };
    let state = workspaceWorkflowReducer(initialWorkspaceWorkflow, {
      type: "publishPreviewRequested",
      sessionId: 1,
      request,
    });
    state = workspaceWorkflowReducer(state, { type: "publishClosed" });
    state = workspaceWorkflowReducer(state, {
      type: "publishPreviewLoaded",
      sessionId: 1,
      preview: {
        targets: [],
        conflicts: [],
        warnings: [],
        gatewayRevision: "gateway-revision",
        credentialRevision: "credential-revision",
        modelRevisions: [],
      },
    });

    expect(state.publishPhase).toBe("closed");
    expect(state.publishPreview).toBeNull();
  });

  it("rejects a preview from an older publish session", () => {
    const request: PreparePublishRequest = {
      gatewayId: "gateway",
      modelIds: ["model"],
      targets: ["workbuddy"],
    };
    let state = workspaceWorkflowReducer(initialWorkspaceWorkflow, {
      type: "publishPreviewRequested",
      sessionId: 1,
      request,
    });
    state = workspaceWorkflowReducer(state, {
      type: "publishPreviewRequested",
      sessionId: 2,
      request: { ...request, modelIds: ["new-model"] },
    });
    state = workspaceWorkflowReducer(state, {
      type: "publishPreviewLoaded",
      sessionId: 1,
      preview: {
        targets: [],
        conflicts: [],
        warnings: [],
        gatewayRevision: "stale",
        credentialRevision: "stale",
        modelRevisions: [],
      },
    });

    expect(state).toMatchObject({
      publishPhase: "previewing",
      publishSessionId: 2,
      publishPreview: null,
      publishRequest: { modelIds: ["new-model"] },
    });
  });
});
