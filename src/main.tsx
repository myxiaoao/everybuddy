import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { FrontendErrorBoundary } from "./components/FrontendErrorBoundary";
import { installGlobalErrorHandlers } from "./lib/frontend-logger";

installGlobalErrorHandlers();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <FrontendErrorBoundary>
      <App />
    </FrontendErrorBoundary>
  </React.StrictMode>,
);
