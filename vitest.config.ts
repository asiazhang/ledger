import { configDefaults, defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
import { fileURLToPath, URL } from "node:url";

const DOM_PACKAGE_TEST_GLOBS = [
  "packages/i18n/**/*.test.ts",
  "packages/storage/**/*.test.ts",
  "packages/theme/**/*.test.ts",
  "packages/utils/**/*.test.ts",
  "packages/window-tier/**/*.test.ts",
  "packages/scheduled-plan-list/**/*.test.ts",
  "packages/ui-kit/**/*.test.ts",
];

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  test: {
    globals: true,
    clearMocks: true,
    testTimeout: 30_000,
    projects: [
      {
        test: {
          name: "app",
          environment: "jsdom",
          setupFiles: ["./packages/test-support/src/setup.ts", "./src/__tests__/app-setup.ts"],
          include: ["src/__tests__/**/*.test.ts"],
        },
      },
      {
        test: {
          name: "packages",
          environment: "node",
          setupFiles: ["./packages/test-support/src/setup.ts"],
          include: ["packages/**/*.test.ts"],
          exclude: [...configDefaults.exclude, ...DOM_PACKAGE_TEST_GLOBS],
        },
      },
      {
        test: {
          name: "packages-dom",
          environment: "jsdom",
          setupFiles: ["./packages/test-support/src/setup.ts"],
          include: [...DOM_PACKAGE_TEST_GLOBS],
        },
      },
    ],
  },
});
