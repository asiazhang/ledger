// 前端变异测试配置（spec #1816 / ADR-0137 决策 3 / 接入票 #1832）。
// 每日 mutation-bench.yml workflow 消费，不进 check.sh、不进 PR 门禁（ADR-0137 决策 1）。
// 报告可信前提：vitest-runner@10.0.0 未适配 Vitest 5 会全量误报 Survived，
// 由 pnpm patchedDependencies 套用上游修复 PR stryker-js#6214（见 pnpm-workspace.yaml
// 与 patches/@stryker-mutator__vitest-runner@10.0.0.patch 头注），并以 workflow 内的
// killer 变异金丝雀验收（已知必杀变异报 Survived 即 runner 失效，#6073 残留误报探针）。
// 普通对象导出（defineConfig 从 @stryker-mutator/api/core 导入会依赖传递依赖
// 暴露面，pnpm 严格布局下不可达）。
export default {
  // runner 与 checker（ADR-0137 决策 3）：vitest 消费仓库根 vitest.config.ts
  // （多 project 配置原样生效）；typescript-checker 用根 tsconfig.json 验证
  // 变异体可编译（include src/**、exclude src/__tests__，与 mutate 面同口径）。
  testRunner: "vitest",
  // checker 配置值是插件注册名 "typescript"，包名才是 @stryker-mutator/typescript-checker。
  checkers: ["typescript"],
  // 显式声明插件包（bare 名，非默认 glob）：默认 glob 只扫 core 私有 node_modules，
  // pnpm 隔离布局下看不到兄弟插件包；bare import 依赖 publicHoistPattern 提升
  // （见 pnpm-workspace.yaml），这是 Stryker + pnpm 的官方 workaround。
  plugins: ["@stryker-mutator/vitest-runner", "@stryker-mutator/typescript-checker"],
  // 变异面：根包 src 的 .ts——排除测试（src/__tests__）与声明文件；.vue 不在
  // 变异面（ADR-0137 决策 3，组件层观测面缺口语显式接受）。
  mutate: ["src/**/*.ts", "!src/**/*.test.ts", "!src/**/*.d.ts", "!src/**/__tests__/**"],
  // perTest 覆盖分析：每个变异体只跑覆盖它的测试（vitest-runner 过滤用例链的
  // 分隔符适配即打在这个路径上，见 patch）。
  coverageAnalysis: "perTest",
  // inPlace（#1832 诊断实测）：绕开 sandbox 拷贝——pnpm 布局下 setupFiles 相对
  // 路径与 @ledger/test-support 包名在 sandbox 内会解析到不同模块实例（双实例），
  // 测试替身接缝失效；就地变异无此问题。
  inPlace: true,
  // 增量（ADR-0137 决策 3）：只重测受上次结果影响的变异体；增量文件走 CI
  // actions/cache（mutation-bench.yml 前端 job），本地跑完即落 reports/。
  incremental: true,
  // 观测期不判定（ADR-0137 决策 4）：break 阈值为 null，存活变异体不使命令失败；
  // 报告进 Step Summary 由 mutation-summary 前端脚本格式化，阈值待基线稳定后另立。
  thresholds: { break: null },
  // json：Step Summary 报告脚本与金丝雀断言的数据源；html：artifact 下钻；
  // progress：本地跑时控制台进度（CI 自动降级 append-only）。
  reporters: ["json", "html", "progress"],
};
