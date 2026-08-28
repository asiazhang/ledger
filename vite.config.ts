import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { execSync } from "node:child_process";
import { fileURLToPath, URL } from "node:url";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// 构建期 Git 版本信息（tauri dev / tauri build 均经此配置生效），
// 消费方见 src/utils/git-info.ts；非 Git 目录（如源码包构建）降级为空值。
function gitSha(): string {
  try {
    return execSync("git rev-parse HEAD", { encoding: "utf-8" }).trim();
  } catch {
    return "";
  }
}

function gitDirty(): boolean {
  try {
    return (
      execSync("git status --porcelain", { encoding: "utf-8" }).trim().length >
      0
    );
  } catch {
    return false;
  }
}

const define = {
  __GIT_SHA__: JSON.stringify(gitSha()),
  __GIT_DIRTY__: JSON.stringify(gitDirty()),
};

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [vue()],

  define,

  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
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
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
