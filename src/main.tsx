import React from "react";
import ReactDOM from "react-dom/client";
import { MainApp } from "./windows/main/MainApp";
import { OverlayApp } from "./windows/overlay/OverlayApp";
import { TooltipProvider } from "./components/ui/tooltip";
import { Toaster } from "./components/ui/sonner";
import "./styles.css";

const overlay = window.location.search.includes("overlay") || window.name === "overlay";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {overlay ? (
      <OverlayApp />
    ) : (
      <TooltipProvider>
        <MainApp />
        <Toaster />
      </TooltipProvider>
    )}
  </React.StrictMode>,
);
