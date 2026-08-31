import type {
  PreparePublishRequest,
  PublishPreview,
  PublishResult,
} from "../types";

export type PublishPhase =
  "closed" | "previewing" | "ready" | "publishing" | "result";

export interface WorkspaceWorkflow {
  activeOperationCount: number;
  dirtyModelKey: string | null;
  discardOpen: boolean;
  publishPhase: PublishPhase;
  publishSessionId: number | null;
  publishRequest: PreparePublishRequest | null;
  publishPreview: PublishPreview | null;
  publishResult: PublishResult | null;
}

export type WorkspaceWorkflowEvent =
  | { type: "operationStarted" }
  | { type: "operationFinished" }
  | { type: "dirtyChanged"; modelKey: string | null }
  | { type: "discardRequested" }
  | { type: "discardConfirmed" }
  | { type: "discardCancelled" }
  | {
      type: "publishPreviewRequested";
      sessionId: number;
      request: PreparePublishRequest;
    }
  | { type: "publishPreviewLoaded"; sessionId: number; preview: PublishPreview }
  | { type: "publishPreviewFailed"; sessionId: number }
  | { type: "publishExecutionStarted"; sessionId: number }
  | {
      type: "publishExecutionFinished";
      sessionId: number;
      result: PublishResult;
    }
  | { type: "publishExecutionFailed"; sessionId: number }
  | { type: "publishClosed" };

export const initialWorkspaceWorkflow: WorkspaceWorkflow = {
  activeOperationCount: 0,
  dirtyModelKey: null,
  discardOpen: false,
  publishPhase: "closed",
  publishSessionId: null,
  publishRequest: null,
  publishPreview: null,
  publishResult: null,
};

export function workspaceWorkflowReducer(
  state: WorkspaceWorkflow,
  event: WorkspaceWorkflowEvent,
): WorkspaceWorkflow {
  switch (event.type) {
    case "operationStarted":
      return {
        ...state,
        activeOperationCount: state.activeOperationCount + 1,
      };
    case "operationFinished":
      return {
        ...state,
        activeOperationCount: Math.max(0, state.activeOperationCount - 1),
      };
    case "dirtyChanged":
      return { ...state, dirtyModelKey: event.modelKey };
    case "discardRequested":
      return state.dirtyModelKey ? { ...state, discardOpen: true } : state;
    case "discardConfirmed":
      return { ...state, dirtyModelKey: null, discardOpen: false };
    case "discardCancelled":
      return { ...state, discardOpen: false };
    case "publishPreviewRequested":
      return {
        ...state,
        publishPhase: "previewing",
        publishSessionId: event.sessionId,
        publishRequest: event.request,
        publishPreview: null,
        publishResult: null,
      };
    case "publishPreviewLoaded":
      return publishEventMatches(state, event.sessionId, "previewing")
        ? {
            ...state,
            publishPhase: "ready",
            publishPreview: event.preview,
          }
        : state;
    case "publishPreviewFailed":
      return publishEventMatches(state, event.sessionId, "previewing")
        ? closePublishWorkflow(state)
        : state;
    case "publishExecutionStarted":
      return publishEventMatches(state, event.sessionId, "ready") &&
        state.publishRequest &&
        state.publishPreview
        ? { ...state, publishPhase: "publishing" }
        : state;
    case "publishExecutionFinished":
      return publishEventMatches(state, event.sessionId, "publishing")
        ? { ...state, publishPhase: "result", publishResult: event.result }
        : state;
    case "publishExecutionFailed":
      return publishEventMatches(state, event.sessionId, "publishing")
        ? { ...state, publishPhase: "ready" }
        : state;
    case "publishClosed":
      return closePublishWorkflow(state);
  }
}

function closePublishWorkflow(state: WorkspaceWorkflow): WorkspaceWorkflow {
  return {
    ...state,
    publishPhase: "closed",
    publishSessionId: null,
    publishRequest: null,
    publishPreview: null,
    publishResult: null,
  };
}

function publishEventMatches(
  state: WorkspaceWorkflow,
  sessionId: number,
  phase: PublishPhase,
) {
  return state.publishSessionId === sessionId && state.publishPhase === phase;
}

export function isWorkspaceBusy(state: WorkspaceWorkflow) {
  return state.activeOperationCount > 0;
}
