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
    setupFiles: ['./src/__tests__/setup.ts'],
    include: ['src/__tests__/**/*.test.ts'],
  },
})
