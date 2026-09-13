import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { initLang } from "./i18n";
import "./styles.css";

// La langue précède le premier rendu : elle vient de Rust, et un panneau qui s'afficherait
// en anglais avant de basculer en français se verrait.
void initLang().then(() => {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
});
