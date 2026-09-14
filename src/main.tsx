import React from "react";
import ReactDOM from "react-dom/client";
import { MainApp } from "./windows/main/MainApp";
import { OverlayApp } from "./windows/overlay/OverlayApp";
import { TooltipProvider } from "./components/ui/tooltip";
import "./styles.css";

const overlay =
  document.documentElement.dataset.voxelyWindow === "overlay" ||
  window.location.pathname.endsWith("overlay.html") ||
  window.location.search.includes("overlay");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {overlay ? (
      <OverlayApp />
    ) : (
      <TooltipProvider>
        <MainApp />
      </TooltipProvider>
    )}
  </React.StrictMode>,
);
