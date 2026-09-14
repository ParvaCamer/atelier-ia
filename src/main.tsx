import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./app/App";
import { installDevMock, isTauri } from "./ipc/devMock";
import "./styles.css";

// Hors de la coquille desktop, on branche le harnais de développement.
// Dans l'application réelle, cette branche n'est jamais prise.
if (import.meta.env.DEV && !isTauri()) installDevMock();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
