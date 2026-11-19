# ADR 0148: 退役 Web 参照前端——NSH 升唯一前端，码化错误翻译对拍资源迁址保留

- 状态：已接受（删除实施随 #2172）
- 日期：2026-11-19
- 作者：Ledger 项目
- 关联：ADR-0145（原生壳——本决策废止其「Web 参照前端保留」条款）、ADR-0050（码化错误本地化契约——门保留，资源迁址）、ADR-0143（浏览器开发模式与替身后端——随本决策整体退役）、ADR-0085 / ADR-0088 票①（前端测试接缝族——随宿主退役）、ADR-0093 / ADR-0118 / ADR-0128 / ADR-0137（Web 工具链面决策——随宿主退役）；spec #2171，实施 #2172
- 来源：grill-with-docs 会话裁决（2026-11-19，七项裁决全数收敛）

## 背景

ADR-0145 修订注记（2026-11-10）把 Web 前端从「整体退役」改为「整体保留、降格为 Web 参照前端」，理由是布局对齐尚未系统盘点、活体对照物有保真价值。此后对照使命按计划完成：逐视图迁移 26/26（#2033–#2056）、功能平价收口（#2126）、布局盘点（#2084）→ 布局对齐剩余项 10/10 收口（#2135）、加密面落地（#2088）。NSH 与 Web 参照前端在官方口径（布局结构对齐；控件形态 GTK 原生、交互语义按保真纪律不逐像素照搬）下已完全对齐，对照基线冻结（`src/` 自 2026-11-12 无演进）。

保留理由随对照收口消失，继续保留的成本三项并存：每次质量门槛承担前端 lint/类型检查/测试的维护税；「哪一套前端是真的」的认知负担；仓库体量冗余。会话裁决：推翻对照物保留条款，整体退役。

## 决策

1. **推翻 ADR-0145 修订注记（2026-11-10）的「Web 参照前端保留」条款**：Web 参照前端整体退役删除——`src/`、全部 `packages/*`（19 包，含 mock-backend、theme、money、test-support、ui-kit、i18n 等）、全部 Vitest 测试面、pnpm 工具链全套（workspace、lock、patches、tsconfig 工程、根 package.json、CI node runtime）。NSH 成为唯一前端。
2. **前端测试面不移植**：Vitest 用例的机制依赖（naive-ui 页签 key 纪律、jsdom 渲染、媒体查询接缝等）不跨栈成立，NSH 无对应故障模式且已有本壳等价物（`keep_mounted.rs` 的 show 页签纪律对应物、headless 组件测试 2871 用例）；移植属形式主义，显式放弃。
3. **ADR-0050 门保留，资源迁址**：`packages/i18n/src/locales/{zh-CN,en-US}/errors.json` 迁住仓库根 `locales/`，`check-i18n-keys.ts` 默认扫描路径同步迁移；Rust 码化错误 ↔ 双语错误模板的对拍继续由 CI 拦截（约 41 处 .rs 注释引用同步改路径/标注退役）。其余命名空间翻译文件（accounts / dashboard 等约 20 份）的唯一消费者是被删的 Web UI，随退役删除。
4. **质量门随宿主退役、不空转**：vue-tsc×2、oxlint、oxfmt、check-frontend-structure、check-style-blocks、check-dialog-forms、check-test-stubs、check-async-guards 八门删除；守门脚本 TS 类型检查**换宿主保留**（vue-tsc 的存在理由是 vue 单文件组件，typescript 7 = Go 端口原生 tsc 可直接检查 TS 工程）——工具链经 `bun install` 装入用户缓存目录（版本钉死在 `scripts/check-scripts-types.sh` 单一事实源，仓库零 manifest；用户裁决方案 C：运行时拉取、不考虑离线环境）。check.sh / CI gates job（原 frontend job 更名）/ gate-mounts 登记三侧同步。
5. **对拍基准转人工对齐**：theme / money 等包曾是 Rust 侧口径的 TS 对拍参照，删除后 Rust 侧改口径不再有 TS 实现可对拍——此代价 ADR-0145 已显式接受过（「对拍基准人工对齐」），本决策延续。
6. **决策留痕**：ADR-0145 加本票指针注记；ADR-0050 加迁址注记；被删工具链面的决策记录（ADR-0046 / 0047 / 0118 / 0128 / 0137 / 0143）在索引加退役注记；AGENTS.md、CONTEXT-MAP、应用壳域 / 测试基础设施域 / 界面状态与交互域词条、README 同步。

## 后果

- 仓库零 JS 工具链 manifest：pnpm、vite、vitest、stryker、oxlint、oxfmt 全部消失；bun 保留为守门脚本运行时（ADR-0083 不变）与类型检查工具链的装配器。
- `src-tauri` 时代 CI 镜像内 Tauri 构建依赖（WebKit 系 apt 包）成为无消费重量，镜像内容收敛属范围外（ci-image.yml 另行收敛候选）。
- 守门脚本工具链的 Python + uv 迁移（替代 bun）另立 spec 承接（2026-11-19 会话裁决 A 方案：本票以 bun 形态收尾，迁移面已因八门退役缩小）。
- CHANGELOG 在 Unreleased 记「Web 参照前端退役」条目（非 BREAKING——不涉 schema、迁移与 AI API 契约）。

## 修订注记（ADR-0145 指针，2026-11-19）

ADR-0145 修订注记 2026-11-10 决策 1「Web 前端整体保留、降格为 Web 参照前端」与本票冲突，以本票为准：对照使命完结后保留条件不再成立，参照前端整体退役（闭集见上文决策 1）。ADR-0145 其余决策（NSH 选型、UI 保真口径、一体切轨）不变；「布局靠拢 web 版」的口径历史所指见应用壳域「Web 参照前端」墓碑词条。

## 修订注记（#2174 指针，2026-11-20）

决策 4「守门脚本 TS 类型检查换宿主保留」与后果节的「bun 保留为守门脚本运行时（ADR-0083 不变）」自本注记起失效：守门脚本运行时已迁移 Python + uv（ADR-0083 修订注记 / issue #2174），类型检查宿主改为 mypy，本票的 bun 形态属过渡态——迁移面已因八门退役缩至最小，逐语义移植不改变本票的退役闭集与对拍裁决。
