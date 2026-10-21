import { defineConfig, type Plugin } from "vite";
import vue from "@vitejs/plugin-vue";
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, URL } from "node:url";

// 窗口分级断点唯一收口于 packages/window-tier/src/useWindowTier.ts 的
// WINDOW_TIER_BREAKPOINT_PX（ADR-0088 决策 2；#1315 起包化为 @ledger/window-tier，
// ADR-0118 决策 5：构建期契约保留，只换收口点坐标）。本配置以源码为唯一事实源运行时
// 提取：不静态 import 包源码（避免把产品源码拉进 tsconfig.node.json 的类型工程边界），
// 且收口漂移在构建期 fail-loud——收口点改名或移位而不修此提取，tauri dev/build 立即报错。
export const WINDOW_TIER_BREAKPOINT_PX = readWindowTierBreakpointPx();

function readWindowTierBreakpointPx(): number {
  // vitest 转换后 import.meta.url 非 file: scheme（check-test-support.test.ts 同款
  // 前提）：vite 与 vitest 的进程 cwd 都是仓库根，以 cwd 定位收口点。
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

// 构建期 Git 版本信息（tauri dev / tauri build 均经此配置生效），
// 消费方见 packages/utils/src/git-info.ts（#1314 起随 @ledger/utils 成包）；非 Git 目录（如源码包构建）降级为空值。
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

/**
 * 浏览器开发模式的 tauri API 面模块替身别名（issue #1962 / ADR-0143 决策 5）：
 * 仅 mock mode 下把 tauri API 面整体替换为 @ledger/mock-backend 的进程内替身，
 * 全部散用直连点（事件 listen、文件对话框、外链打开、系统返回）一字不动；
 * 其余 mode 返回空——生产构建引用真模块，替身模块图不可达（静态消除的第二
 * 道构造条件，第一道是 main.ts 仅存的 mode 分支动态 import；接线由
 * src/__tests__/mock-mode-wiring.test.ts 守门）。
 */
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

// https://vite.dev/config/
export default defineConfig(async ({ mode }) => ({
  // vanilla-extract 插件（issue #888 / ADR-0093）：构建期把 *.css.ts 编译为
  // 静态 CSS，无 codegen 产物目录；置于 vue 插件之前（官方推荐顺序）。
  // 惰性动态导入：本模块被 src/__tests__/window-tier-build-contract.test.ts 直接
  // 导入（断点占位符替换函数的测试消费方，#1315 起壳侧契约测试与包内 composable
  // 测试分置——包内禁止相对路径穿越，守门规则③），静态引入 esbuild
  // （vanilla-extract 依赖链）会在该测试的 jsdom 环境下触发 TextEncoder 不变式崩溃；
  // 动态导入使导入本模块不拉起 esbuild，仅真实 vite 构建时加载插件。
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
      // 浏览器开发模式（ADR-0143）：mock mode 下 tauri API 面整体模块替身；其余
      // mode 空表——真模块引用原样解析，替身不进生产包
      ...mockModeAliases(mode),
    ],
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
