import type { AppSettings, TargetKind, TargetStatus } from "../types";

export const defaultSettings: AppSettings = {
  language: "zh-CN",
  theme: "system",
  selectedTargets: [],
  targetSelectionInitialized: false,
  targetPaths: {
    workbuddy: "~/.workbuddy/models.json",
    codebuddy: "~/.codebuddy/models.json",
  },
};

export function isTargetPublishable(target: TargetStatus) {
  return target.installed && target.writable && target.schema !== "invalid";
}

export function displayTarget(target: TargetKind) {
  return target === "workbuddy" ? "WorkBuddy" : "CodeBuddy";
}
