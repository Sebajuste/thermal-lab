import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Port fixe : Tauri pointe dessus via `devUrl` dans tauri.conf.json.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    // `127.0.0.1` explicite, et non le defaut `localhost` : celui-ci fait ecouter Vite
    // sur `[::1]` seul, tandis que la WebView resout `localhost` en IPv4 et tombe sur
    // une connexion refusee — fenetre blanche, sans message. Les deux bouts sont donc
    // fixes sur IPv4, sans resolution de nom entre eux. Meme piege que celui documente
    // dans `sensors/lhm.rs` pour le serveur de LibreHardwareMonitor.
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
