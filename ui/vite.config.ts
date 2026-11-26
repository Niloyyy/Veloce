import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  base: "./",
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "chrome130",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    cssMinify: true,
    modulePreload: {
      polyfill: false,
    },
  },
});
