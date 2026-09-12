import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import { fileURLToPath, URL } from 'node:url'

export default defineConfig({
  // 测试不挂 vanilla-extract 插件（issue #888）：*.css.ts 在 vitest 下走纯运行时
  // 求值——主题合同测试经官方 adapter 接缝（@vanilla-extract/css/adapter）捕获
  // createTheme 产出物做同源断言；插件挂载时 CSS 在 transform 期即被抽走，运行时
  // 无块可捕获。jsdom 不消费样式表，测试无需真实 CSS 产物；真实 CSS 由 vite
  // build 产出（见 vite.config.ts）。
  plugins: [vue()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    // 每测清理全部 mock 的调用记录（vitest 5 起为默认值，此处显式写死）：
    // 清记录不碰实现（mockReset 才清 implementation），故 mockResolvedValue
    // 等静态实现不受影响；执行时机在用户 beforeEach 之前（@vitest/runner 的
    // onBeforeTryTask → 再跑 beforeEach），不会抹掉用例内刚记下的调用。
    // 显式化的收益是覆盖 setup.ts 未管的模块级 mock（pushMock / writeText 等），
    // 且不受 vitest 未来默认值变动影响。
    clearMocks: true,
    // 单测墙钟预算（issue #1162）：用例内的 flushPromises（真实 setTimeout）与
    // naive-ui 过渡收尾（jsdom rAF ≈ 16.7ms tick）串行消耗真实墙钟，机器高负载时
    // 随负载线性拉长。默认 5000ms 下最慢一批用例（基线 ~0.7s：「删除当前页最后一条
    // 回退」「URL 下钻往返」等）在 ~7.5x 负载即越过预算偶发红——超时后用例仍会跑完，
    // 报告里的长耗时（当时 21s/29s/46s）即真实完成时长。20s ≈ 30x 基线余量；代价仅是
    // 真挂死的用例报告更慢，不改变任何断言语义。
    testTimeout: 20_000,
    setupFiles: ['./src/__tests__/setup.ts'],
    // 守门脚本包装测试与所测脚本同目录住 scripts/（issue #1158），前端测试住
    // src/__tests__：两处都纳入。setupFiles 仍指 src/__tests__，对两个目录统一生效。
    include: ['src/__tests__/**/*.test.ts', 'scripts/**/*.test.ts'],
  },
})
