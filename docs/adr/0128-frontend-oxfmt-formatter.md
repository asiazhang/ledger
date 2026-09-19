# ADR-0128: 前端格式化器引入 oxfmt——独立形态、--check 入质量门、否决 Vite+ 整体引入

- 状态：已接受
- 日期：2026-09-19
- 作者：Ledger 项目
- 关联：issue #1519

## 背景

仓库自始没有 formatter（oxlint 只做 lint 不管风格），引号 / 分号 / 断行漂移靠评审自觉，实际已出现 vite.config.ts 双引号、packages 大面积单引号的两态并存。评估过以整体引入 Vite+（VoidZero 统一工具链，`vp` CLI）作为补齐 oxfmt 的载体：vp 0.3.x 内置 vitest 4.1.x，引入会把本仓 vitest ^5 压回 4.x（显著回退）；`vp check` 的类型检查不做 .vue SFC（vue-tsc 仍需旁路保留）；而本仓已是 Vite 8 + Vitest 5 + Oxlint，Vite+ 的边际收益只剩 CLI 外壳。oxfmt 作为独立 npm 包形态功能完整（oxc.rs 官方推荐形态；GitHub 纯二进制形态无 Prettier-backed 格式化、不支持 .vue，不采用）。

dependency-upgrades.md 口径「最新 = 最新 stable，不含预发布」，oxfmt 0.x 引入属显式破例，理由与重评条件见决策 6。

## 决策

1. **格式化器选 oxfmt，独立 devDependency 引入；否决整体引入 Vite+**（理由见背景；否决 Prettier / Biome / 维持无 formatter 见「理由」）。
2. **形态 = npm 包**（非 vp 内置、非纯二进制）：.vue 整文件支持——template 与非 script 块走内置 Prettier（Prettier-backed），`<script>` 块走 oxc 原生格式化器。
3. **配置默认值起步**：printWidth 100（oxfmt 默认，锚定现状宽行风格）、Prettier 3.8 兼容风格（双引号、分号）；`sortPackageJson` 显式 false（保 package.json 手工键序）；`sortImports` 不开（导入顺序不属本次风格争议面，不引入额外 churn）。
4. **范围**：src/、packages/、scripts/ 的 .ts/.vue/.css 与根部 *.config.ts、.json、index.html；排除 pnpm-lock.yaml（包管理器自管理）、src-tauri（Rust 侧有 cargo fmt）、.github、全部 markdown（中文 prose 格式化 churn 大收益低）、yaml/toml。
5. **门禁**：format / format:check scripts 供本地使用；check.sh 与 CI build.yml frontend job 双侧同门槛挂 `oxfmt --check`（防双轨漂移）；接线测试承载「删除执行行即变红」负向判据（ADR-0087 断言强度，断言对准执行行而非 echo 文案），并断言 .oxfmtrc.json 可解析且风格锚点未漂移——oxc #25125：配置非法时 oxfmt 静默回退默认值，须 fail-loud。
6. **pre-1.0 破例**：JS/TS 输出与 Prettier 3.8 声明 100% conformance（任何差异视为上游 bug）；CI --check 门兜底格式化器自身 bug；输出风格与 Prettier 兼容，切回 Prettier 的成本 ≈ 换一个 devDependency（退出成本低）。重评条件：oxfmt 1.0 发布；conformance 声明被破坏；或格式门出现误放行实例。
7. **全仓格式化一次性 churn（526 文件）独立 commit**，与依赖 / 配置 / 接线分层，可二分定位。

## 理由

- **为什么 oxfmt 而非 Prettier**：Prettier 3.8 兼容风格下显著更快（Rust 原生）；本仓 lint 已用 oxlint，同属 oxc 生态、联动发版、心智一致；兼容性同时是退出通道。
- **为什么不 Biome**：与 oxlint 职责重叠，引入第二个 lint / 格式生态徒增配置维护面；oxfmt 单生态贴合现状。
- **为什么不维持无 formatter**：风格漂移已实际发生，评审持续为格式分心；一次性 churn 买断，此后格式争议归工具。
- **为什么 printWidth 100**：oxfmt 默认即 100，现状代码即宽行风格（断行 churn 最小的锚点）。

## 代价与边界

1. 0.x 每周发版，格式化器自身 bug 可能改动输出——升级走显式 bump 逐版审，CI --check 兜底。
2. .vue 同文件双引擎（template = Prettier-backed、script = 原生），边界行为以 oxc 官方兼容矩阵为准；已知开放项 #24361（Vue 单行 handler 分号选项）不影响门禁。
3. 源码扫描型守门测试的文本锚点随格式约定同步（check-style-blocks.test.ts 白名单锚点双引号化、ts-comment-mask.test.ts import 正则引号无关化）——后续格式漂移按测试自身声明的路径同步。
4. 编辑器集成（Oxc VSCode 扩展，要求项目内安装 oxfmt）为开发者可选配置，不强制、不进仓库。
5. 纯 devDependency 工具链变更：无运行时行为变化，不改变既有质量门内容，只新增格式门。

## 相关 ADR

- ADR-0046（前端包管理器 pnpm）：oxfmt 经 pnpm 安装与锁文件管理；该 ADR「不写 CHANGELOG」的口径由 dependency-upgrades.md 的 #495 先例取代为极简条目。
- ADR-0083（守门脚本 Bun 运行时）：check.sh 门禁宿主形态不变，oxfmt 以 pnpm exec 消费。
- 无领域关联（工具链决策，不涉领域词汇表）。
