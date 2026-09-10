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
    setupFiles: ['./src/__tests__/setup.ts'],
    include: ['src/__tests__/**/*.test.ts'],
  },
})
