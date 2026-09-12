# AGENTS.md

给 AI 编程助手的仓库级指导：保留稳定原则与文档导航；schema、路由、命令清单和实现细节以代码、脚本及专项文档为准。

## 开始前：按触发条件读文档

“受影响域”包括：修改代码所在域、调用到的域、数据模型所属域，以及用户可见行为所属域。

- **业务规则、领域术语或跨域改动**：先读 `CONTEXT-MAP.md`，再读所有受影响域的 `docs/contexts/CONTEXT-*.md` 与相关 ADR。
- **后端壳、域、基础设施或目录归位（含 triage 判定）**：读 `docs/adr/0056-backend-domain-directory-layering.md`。
- **业务域 crate 内部组织（分区、命名、层序、守门）**：读 `docs/adr/0113-business-domain-crate-internal-organization.md`。
- **后端易 panic 构造（unwrap/expect/panic!/todo!/unimplemented!/unreachable!）或其豁免**：读 `docs/adr/0060-backend-panic-construction-gate.md`。
- **金额或交易写入改动**：先读 `docs/contexts/CONTEXT-core.md` 和相关 ADR，再以当前金额与写入接缝为唯一实现依据。
- **前端状态、界面交互或弹层**：读 `docs/contexts/CONTEXT-reference-settings.md`、`docs/contexts/CONTEXT-ui-interaction.md` 及相关 ADR。
- **用户可见文案或错误**：读相关域词汇表、ADR-0049/0050 和现有 i18n 实现。
- **schema、migration 或数据模型改动**：读 `docs/model/README.md`、相关 migration 和 ADR，并检查发布边界。
- **编写或修改领域词汇表、模型文档或 ADR**：先读 `docs/agents/domain.md`，遵守文档分层、术语唯一和代码坐标规则。
- **AI 导入**：读 `docs/contexts/CONTEXT-ai-import.md`、`src-tauri/prompts/ledger-api.md` 和实际 API 契约。
- **HTTP 端点**：读实际路由、对应 API 契约和 API 集成测试；仅在属于 AI 导入时读取 AI 导入文档。
- **Issue、PR、triage 或依赖关系**：按需读 `docs/agents/issue-tracker.md` 与 `docs/agents/triage-labels.md`。
- **依赖版本升级或工具链 bump**：读 `docs/agents/dependency-upgrades.md`（三层口径、跨 major 实测要求、刻意 hold 的注释纪律、验证与打包盲区）。
- **脚本或质量检查**：先读目标脚本头部注释；脚本当前行为是真源。

代码行为与词汇表不一致时，按可验证行为修正词汇表。代码行为与 ADR 冲突时，先显式报告冲突，确认决策后再修改 ADR 或实现。

## 后端分层

Rust 根（`src-tauri/`）是 workspace：根包仍是 tauri 应用包（壳层、命令注册扫描与集成测试入口），成员 crate 集中在 `src-tauri/crates/`（glob 纳入，新 crate 落入即自动成为成员）。目标依赖方向是 **壳 → 域 → 基础设施 → 协议**，域不依赖壳；拆分轴、正交判据（无环且方向单一）、破环方式与门禁保底见 ADR-0112。业务域按 spec #1086 逐域拆出 crate；拆出前业务语义进入根包内以域命名的顶层域目录，模型随域归位（ADR-0059）；新代码不得扩大壳层业务语义。

业务域 crate 内部按消费面分四区（共享语义 / 跨域接缝 / 写路径 / 读路径），依赖方向单向（写读 → 接缝 → 共享语义），模块清单双向全等与区级反向依赖由结构守门断言，见 ADR-0113；拆出后的域内重排受该 ADR 管辖，不属拆 crate 迁移纪律（ADR-0112 决策 1 的限缩见其修订注记）。

下层不直调上层的路径内副作用：写路径副作用挂载点（置脏触发、余额重算、计划来源解析）按「下层定义注册点、上层注册实现、壳层启动时接线」反转，不为消除合法依赖引入端口/事件反转（ADR-0112 决策 5）；合法依赖（上层依赖下层）直呼。

基础设施（`ledger-infra`，`src-tauri/crates/infra/`）只承诺两条：不依赖任何域 crate、不定义账本数据的口径与规则（账本数据的语义与算术一律归域，基础设施不得对账本数据表执行 DML）；跨层共享机制、引导层不变量、单点收口用的闭集与键名表在此合法（ADR-0111）。crate 内分四区：原语（顶层单文件）、db（库与连接机制）、boot（引导层）、shell_support（只被壳层消费的机制，暂住，随壳层收敛迁出）；events、signals、settings 为共享接缝。内部依赖方向：原语 ← db ← boot ← shell_support。

协议（`ledger-sync-protocol`，`src-tauri/crates/sync-protocol/`）：多端同步的最底层共享协议（设备标识、领域命令契约、op 本地记录与读取、位点）；业务域只依赖协议 crate，不依赖多端同步域。

门禁保底：clippy 六件套唯一声明处在 workspace 级 `[workspace.lints.clippy]`，成员必须显式 `[lints] workspace = true` 继承；静态检查与测试命令显式 `--workspace` 覆盖全部成员。结构边界由 `bun scripts/check-structure.ts` 守门（守门脚本运行时 = Bun，ADR-0083）：crate 边界唯一事实源是脚本内 `CRATES` 清单（成员登记、分层、允许依赖方向），模块级白名单与认许边继续辖编译器看不见的规则；白名单和归位状态以脚本及 ADR-0056、ADR-0111、ADR-0112 为准。

## 数据与交易

- 金额以整数分表达；用户展示统一调用 `formatAmount`，金额换算和展示口径集中维护。
- 交易写入改动须保持 Writer、行为编排、定时执行、批量导入和投资路径的既有接缝契约一致；新增路径前先核对各入口。
- 账户余额与净资产读持久化缓存（V017，ADR-0067）：写路径在既有事务内对受影响账户整体重算，禁止增量加减；实时计算保留为审计与测试的权威比对基准，缓存行缺失报码化错误引导审计修复，不静默回退。

## 前端与入口

- 参考数据、设备偏好、后端设置和运行时状态遵循各自领域的单一来源与归口规则。
- 会影响快捷键抑制的交互层通过现有 `App*` 封装或 `useAppDialog` 接入 Overlay Suppression；新增形态先补对应封装和注册表接线。
- 新增 IPC 命令：放入已声明并扁平再导出的命令模块，由构建扫描器生成注册表，再同步前端 API/类型并运行一致性检查。新增 HTTP-only 端点走 API 契约与 API 集成测试，不增加无关 IPC 调用面。
- 用户可见文案经 i18n；后端用户可见错误使用码化错误构造器，并同步错误模板。

## 测试、工作流与发布

- Rust 侧测试三层各有权威：域行为归域单测（写入编排、金额折算、余额规则、删除清理副作用、查询语义）；壳行为归 API 集成测试（参数解包、状态码、错误码、壳层接线），域语义至多以接线证明出现；跨模块用户旅程归 e2e BDD，不为域规则凑数据。前端逻辑补 Vitest；BDD world 只保存跨步骤读写的状态。
- 改动引入「所有入口都必须遵循」的约束（成对调用、必须接线、必须带门、必须清理）时，用 `rg` 枚举全部调用点并逐一核对，交付报告附核对清单；修复缺陷后，用 `rg` 检索同一根因的其他实例并核对每处命中。
- 拆票或补验收判据时，接线 / 时机 / 入口型 ticket（核心价值是某调用必须在某时机出现在某位置）的验收判据必须含「删除即变红」负向条目：删除 `<接线调用点>` → 至少一条测试变红，断言对准用户可观察结果，不对准线程或函数调用形状（ADR-0087 断言强度）；领域逻辑型不加，避免形式主义；接线在本机 CI 不构建的分支内时以源码扫描守门替代（先例 #959、#961）。
- 编译加速归口 kache：热回路（编辑后重跑 `cargo test`）由仓库根 `.kache.toml` 的 `preserve_incremental` 保住 rustc 增量，冷构建（新 worktree 首次构建）由 kache 共享 store 承担；需要提速构建时直接依赖这套设置，不要另行配置 `RUSTC_WRAPPER`、`KACHE_DISABLED` 等环境变量方案。
- 调用 `/implement` 实施代码改动时：对应 GitHub issue 先认领（见 `docs/agents/issue-tracker.md` 开发认领），再使用独立 git worktree，并在工作树内完成验证和提交；提交后推送分支并主动在 GitHub 上创建 PR，PR 是交付终点，不自行合并。worktree 缺少前端依赖时先运行 `pnpm install`。
- 只读审查不修改、不提交；研究任务是否写入文档，以用户要求和对应 skill 为准。
- 修改迁移、AI API 契约、数据模型或准备发布时，先判断当前提交相对最新 tag 的发布边界。无可用 tag 时，先报告无法判断发布边界，不擅自把 schema/AI API 契约当作已发布或未发布。已发布 AI API 契约和数据模型只增不改；已发布迁移的就地修改须在 migration 文件头部注明对应 CHANGELOG 条目，并在 `CHANGELOG.md` 的对应版本或 `Unreleased` 下增加 BREAKING 条目。

## 范围外发现

实施中遇到 ticket 范围外的问题，按性质分流：

- **用户可见缺陷**（数据错误、功能失效、安全风险等真实缺陷）：直接修掉；连续发现多个时继续逐个处理、一次性报告。
- **设计或范围问题**（决定该不该改、要不要顺带重构）：停下报告，等确认后再动。
- **既有技术债**（重构、命名、结构归位等整理类工作）：单独立 GitHub issue，交付报告列出编号，不顺手修。

范围外修复统一要求：每个修复单独提交、提交信息写明根因，交付报告显式标注「范围外修复」并列出证据。判据：修掉后行为是否变得更正确——是则修，只是更整齐则立单；归属拿不准时按设计或范围问题停下报告。

## 完成标准

1. 已识别受影响域、入口和分层，并读取触发条件所指向的词汇表、ADR 和专项指引。
2. 改动已放在正确接缝，适用测试、文案/错误翻译和文档同步已完成。
3. 代码或脚本改动已运行 `./scripts/check.sh`；仅文档改动已运行 `./scripts/check-docs.sh`。
4. 未解决的 ADR 冲突、无法运行的检查或未提交的改动已明确报告。
