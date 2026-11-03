import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
import { fileURLToPath, URL } from "node:url";

// 前端渲染每日基准的独立配置（issue #2012）：只收 src/__tests__/perf/*.bench.ts
// （默认 vitest.config.ts 的 include 为 *.test.ts，.bench.ts 不进默认套件 / PR CI
// 分片，只在 perf-bench.yml 每日基准 workflow 以本配置显式运行——计时型指标不进
// PR 阻塞路径）。工程装配与 app project 同构（jsdom + 同一全局测试接缝 setup +
// 应用壳装配薄壳 + vue 插件 + @ 别名），保证基准与守门测试跑在同一装配面上。
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
    // 基准单用例含 20 次迭代挂载，预算独立于默认套件的 30s 单测墙钟
    testTimeout: 240_000,
  },
});
