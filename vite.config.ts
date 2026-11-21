import { defineConfig, type Plugin } from "vite";
import vue from "@vitejs/plugin-vue";
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, URL } from "node:url";

export const WINDOW_TIER_BREAKPOINT_PX = readWindowTierBreakpointPx();

function readWindowTierBreakpointPx(): number {
  const source = readFileSync(
    join(process.cwd(), "packages/window-tier/src/useWindowTier.ts"),
    "utf-8",
  );
  const m = /export const WINDOW_TIER_BREAKPOINT_PX = (\d+)/.exec(source);
  if (!m) {
    throw new Error(
      "窗口分级断点不在唯一收口点 packages/window-tier/src/useWindowTier.ts —— 收口漂移，请恢复 WINDOW_TIER_BREAKPOINT_PX 或同步修正本提取",
    );
  }
  return Number(m[1]);
}

const host = process.env.TAURI_DEV_HOST;

function gitSha(): string {
  try {
    return execSync("git rev-parse HEAD", { encoding: "utf-8" }).trim();
  } catch {
    return "";
  }
}

function gitDirty(): boolean {
  try {
    return execSync("git status --porcelain", { encoding: "utf-8" }).trim().length > 0;
  } catch {
    return false;
  }
}

const define = {
  __GIT_SHA__: JSON.stringify(gitSha()),
  __GIT_DIRTY__: JSON.stringify(gitDirty()),
};

export const WINDOW_TIER_CSS_TOKEN = "__WINDOW_TIER_BREAKPOINT_PX__";

export function substituteWindowTierBreakpoint(code: string): string {
  return code.split(WINDOW_TIER_CSS_TOKEN).join(String(WINDOW_TIER_BREAKPOINT_PX));
}

function windowTierBreakpointCss(): Plugin {
  return {
    name: "window-tier-breakpoint-css",
    transform(code) {
      if (!code.includes(WINDOW_TIER_CSS_TOKEN)) return null;
      return substituteWindowTierBreakpoint(code);
    },
  };
}

export function mockModeAliases(mode: string): Array<{ find: string; replacement: string }> {
  if (mode !== "mock") return [];
  const mockTauri = (file: string): string =>
    fileURLToPath(new URL(`./packages/mock-backend/src/tauri/${file}`, import.meta.url));
  return [
    { find: "@tauri-apps/api/core", replacement: mockTauri("core.ts") },
    { find: "@tauri-apps/api/event", replacement: mockTauri("event.ts") },
    { find: "@tauri-apps/api/window", replacement: mockTauri("window.ts") },
    { find: "@tauri-apps/api/app", replacement: mockTauri("app.ts") },
    { find: "@tauri-apps/plugin-dialog", replacement: mockTauri("plugin-dialog.ts") },
    { find: "@tauri-apps/plugin-opener", replacement: mockTauri("plugin-opener.ts") },
  ];
}

export default defineConfig(async ({ mode }) => ({
  plugins: [
    (await import("@vanilla-extract/vite-plugin")).vanillaExtractPlugin(),
    vue(),
    windowTierBreakpointCss(),
  ],

  define,

  resolve: {
    alias: [
      {
        find: "@",
        replacement: fileURLToPath(new URL("./src", import.meta.url)),
      },
      ...mockModeAliases(mode),
    ],
  },

  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
