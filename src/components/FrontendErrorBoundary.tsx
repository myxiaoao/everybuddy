import React from "react";
import { RefreshCw, TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { reportFrontendError } from "@/lib/frontend-logger";

interface State {
  failed: boolean;
}

export class FrontendErrorBoundary extends React.Component<
  React.PropsWithChildren,
  State
> {
  state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    reportFrontendError(
      "react.error-boundary",
      error,
      info.componentStack ?? undefined,
    );
  }

  render() {
    if (!this.state.failed) return this.props.children;

    const english = document.documentElement.lang === "en";
    return (
      <main className="startup-state crash-state" role="alert">
        <TriangleAlert aria-hidden="true" />
        <h1>
          {english ? "The interface stopped responding" : "界面无法继续显示"}
        </h1>
        <p>
          {english
            ? "EveryBuddy recorded redacted diagnostic details. Reload the interface and try again."
            : "EveryBuddy 已记录脱敏后的诊断信息。请重新加载界面后重试。"}
        </p>
        <Button type="button" onClick={() => window.location.reload()}>
          <RefreshCw aria-hidden="true" size={16} />
          {english ? "Reload interface" : "重新加载界面"}
        </Button>
      </main>
    );
  }
}
