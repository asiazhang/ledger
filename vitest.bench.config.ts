import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
import { fileURLToPath, URL } from "node:url";

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./packages/test-support/src/setup.ts", "./src/__tests__/app-setup.ts"],
    include: ["src/__tests__/perf/*.bench.ts"],
    testTimeout: 240_000,
  },
});
