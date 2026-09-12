import { configDefaults, defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import { fileURLToPath, URL } from 'node:url'

// DOM 依存包内测试的登记处（packages-dom project 消费，node project 排除同源）：
// 逐包登记避免双跑（node include 全量 + exclude 同表）与静默漏跑（新包默认 node）。
const DOM_PACKAGE_TEST_GLOBS = [
  'packages/i18n/**/*.test.ts',
  'packages/storage/**/*.test.ts',
]

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
    // 报告里的长耗时（当时 21s/29s/46s）即真实完成时长。30s ≈ 44x 基线余量，覆盖已
    // 观测的失效样本（21s/29s = 30x/42x）；46s 尾样本为残余风险，再抬预算只会延迟
    // 真挂死用例的报告（retry 会掩盖真实间歇性信号，不采用）。不改变任何断言语义。
    testTimeout: 30_000,
    // 多 project 裁定（issue #1152 / spec #1148）：单根配置内 projects 分域，
    // pnpm test 与 CI --shard=N/2 语义不变（分片在各 project 文件并集上均分）。
    // - app（jsdom）：应用壳测试（src/__tests__，issue #1158）+ 守门脚本包装测试
    //   （scripts/，与所测脚本同目录住）。全局测试接缝 setupFiles 指向共享测试
    //   支持包 @ledger/test-support（包内 setup 消费，src/__tests__/setup.ts 已
    //   下沉迁入），再叠应用壳装配薄壳 app-setup.ts（参考 store 刷新器注册——
    //   接缝包不反向依赖应用壳的包化反转）。
    // - packages（node）：纯类型/纯逻辑包内测试的默认落点（测试跟随被测包，
    //   spec #1148 用户故事 10），不付 jsdom 每文件创建成本；同一 setup 经
    //   环境守卫自适应，接缝定义不随环境分裂。DOM 依存的包内测试移出本 project
    //   （并入 packages-dom），新落位的纯逻辑包自动进 node，无需登记。
    // - packages-dom（jsdom）：DOM 依存包内测试的显式登记处（组件挂载或
    //   Storage.prototype 平台语义需 jsdom；原「组件型包测试落位时再扩 jsdom
    //   project」的预定扩位，issue #1151 起 @ledger/i18n / @ledger/storage
    //   先行落入）。#1157 ui-kit 落位时并入本 project。
    projects: [
      {
        test: {
          name: 'app',
          environment: 'jsdom',
          setupFiles: ['./packages/test-support/src/setup.ts', './src/__tests__/app-setup.ts'],
          include: ['src/__tests__/**/*.test.ts', 'scripts/**/*.test.ts'],
        },
      },
      {
        test: {
          name: 'packages',
          environment: 'node',
          setupFiles: ['./packages/test-support/src/setup.ts'],
          include: ['packages/**/*.test.ts'],
          exclude: [...configDefaults.exclude, ...DOM_PACKAGE_TEST_GLOBS],
        },
      },
      {
        test: {
          name: 'packages-dom',
          environment: 'jsdom',
          setupFiles: ['./packages/test-support/src/setup.ts'],
          include: [...DOM_PACKAGE_TEST_GLOBS],
        },
      },
    ],
  },
})
