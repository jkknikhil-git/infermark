import React from "react";
import ReactDOM from "react-dom/client";
// Fonts and icons are bundled with the app so it works fully offline (no Google Fonts requests).
import "@fontsource/roboto/400.css";
import "@fontsource/roboto/500.css";
import "material-symbols/outlined.css";
import "./styles.css";
import App from "./App";
import { applyTheme, loadPreferences } from "./theme";

const prefs = loadPreferences();
applyTheme(prefs.accent, prefs.mode);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
