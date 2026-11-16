import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(({ mode }) => {
  // `tauri android dev` su un dispositivo reale imposta TAURI_DEV_HOST con
  // l'indirizzo del computer; altrove il server resta solo su 127.0.0.1.
  const devHost = loadEnv(mode, ".", "TAURI_").TAURI_DEV_HOST;
  return {
    plugins: [react()],
    root: "frontend",
    clearScreen: false,
    server: {
      host: devHost || "127.0.0.1",
      port: 1420,
      strictPort: true,
    },
    build: {
      outDir: "../dist",
      emptyOutDir: true,
    },
  };
});
