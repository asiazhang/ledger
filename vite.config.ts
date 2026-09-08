import { defineConfig, type Plugin } from "vite";
import vue from "@vitejs/plugin-vue";
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, URL } from "node:url";

// 窗口分级断点唯一收口于 src/composables/useWindowTier.ts 的 WINDOW_TIER_BREAKPOINT_PX
// （ADR-0088 决策 2）。本配置以源码为唯一事实源运行时提取：不静态 import src 文件
// （避免把产品源码拉进 tsconfig.node.json 的类型工程边界），且收口漂移在构建期
// fail-loud——收口点改名或移位而不修此提取，tauri dev/build 立即报错。
export const WINDOW_TIER_BREAKPOINT_PX = readWindowTierBreakpointPx();

function readWindowTierBreakpointPx(): number {
  // vitest 转换后 import.meta.url 非 file: scheme（check-test-support.test.ts 同款
  // 前提）：vite 与 vitest 的进程 cwd 都是仓库根，以 cwd 定位收口点。
  const source = readFileSync(join(process.cwd(), "src/composables/useWindowTier.ts"), "utf-8");
  const m = /export const WINDOW_TIER_BREAKPOINT_PX = (\d+)/.exec(source);
  if (!m) {
    throw new Error(
      "窗口分级断点不在唯一收口点 src/composables/useWindowTier.ts —— 收口漂移，请恢复 WINDOW_TIER_BREAKPOINT_PX 或同步修正本提取",
    );
  }
  return Number(m[1]);
}

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

// 窗口分级断点的构建期共享（issue #841 / ADR-0088 决策 2）：断点数值唯一收口于
// useWindowTier 的 WINDOW_TIER_BREAKPOINT_PX 常量（上方从源码提取）；CSS 源码不
// 书写魔法数字，以占位符消费同值，构建期在此替换。用法：
//   @media (max-width: __WINDOW_TIER_BREAKPOINT_PX__px) { … }
export const WINDOW_TIER_CSS_TOKEN = "__WINDOW_TIER_BREAKPOINT_PX__";

/** 把 CSS 源码中的断点占位符替换为唯一断点常量值（纯函数，测试直打）。 */
export function substituteWindowTierBreakpoint(code: string): string {
  return code.split(WINDOW_TIER_CSS_TOKEN).join(String(WINDOW_TIER_BREAKPOINT_PX));
}

/** 断点占位符替换插件：占位符本身即唯一定位串，凡含它的模块（CSS / SFC 样式块 /
 * 其他文本模块）构建期一律替换为唯一断点常量值。 */
function windowTierBreakpointCss(): Plugin {
  return {
    name: "window-tier-breakpoint-css",
    transform(code) {
      if (!code.includes(WINDOW_TIER_CSS_TOKEN)) return null;
      return substituteWindowTierBreakpoint(code);
    },
  };
}

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [vue(), windowTierBreakpointCss()],

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
