import React from "react";
import ReactDOM from "react-dom/client";
import { MainApp } from "./windows/main/MainApp";
import { OverlayApp } from "./windows/overlay/OverlayApp";
import "./styles.css";

const overlay = window.location.search.includes("overlay") || window.name === "overlay";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{overlay ? <OverlayApp /> : <MainApp />}</React.StrictMode>,
);
