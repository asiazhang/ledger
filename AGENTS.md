# AGENTS.md

给 AI 编程助手的仓库级指导：保留稳定原则与文档导航；schema、路由、命令清单和实现细节以代码、脚本及专项文档为准。

## 开始前：按触发条件读文档

“受影响域”包括：修改代码所在域、调用到的域、数据模型所属域，以及用户可见行为所属域。

- **业务规则、领域术语或跨域改动**：先读 `CONTEXT-MAP.md`，再读所有受影响域的 `docs/contexts/CONTEXT-*.md` 与相关 ADR。
- **后端壳、域、基础设施或目录归位**：读 `docs/adr/0056-backend-domain-directory-layering.md`；分层规则、归位域与 triage 判定以该 ADR 为准。
- **后端易 panic 构造（unwrap/expect/panic!/todo!/unimplemented!/unreachable!）或其豁免**：读 `docs/adr/0060-backend-panic-construction-gate.md`；六件套 deny 门禁与逐点豁免纪律以该 ADR 为准。
- **金额或交易写入改动**：先读 `docs/contexts/CONTEXT-core.md` 和相关 ADR，再以当前金额与写入接缝为唯一实现依据。
- **前端状态、界面交互或弹层**：读 `docs/contexts/CONTEXT-reference-settings.md`、`docs/contexts/CONTEXT-ui-interaction.md` 及相关 ADR。
- **用户可见文案或错误**：读相关域词汇表、ADR-0049/0050 和现有 i18n 实现。
- **schema、migration 或数据模型改动**：读 `docs/model/README.md`、相关 migration 和 ADR，并检查发布边界。
- **编写或修改领域词汇表、模型文档或 ADR**：先读 `docs/agents/domain.md`，遵守文档分层、术语唯一和代码坐标规则。
- **AI 导入**：读 `docs/contexts/CONTEXT-ai-import.md`、`src-tauri/prompts/ledger-api.md` 和实际 API 契约。
- **HTTP 端点**：读实际路由、对应 API 契约和 API 集成测试；仅在属于 AI 导入时读取 AI 导入文档。
- **Issue、PR、triage 或依赖关系**：按需读 `docs/agents/issue-tracker.md` 与 `docs/agents/triage-labels.md`。
- **脚本或质量检查**：先读目标脚本头部注释；脚本当前行为是真源。

代码行为与词汇表不一致时，按可验证行为修正词汇表。代码行为与 ADR 冲突时，先显式报告冲突，确认决策后再修改 ADR 或实现。

## 后端分层

目标依赖方向是 **壳 → 域 → 基础设施**，域不依赖壳。IPC 壳（`src-tauri/src/commands/`）与 HTTP 壳（`src-tauri/src/api_server/`）负责参数解包、事务边界和信号发射，不含业务语义；业务语义进入以域命名的顶层域目录；无域语义的数据库、信号、错误和设置能力进入基础设施（模型已随域归位，ADR-0059）。新代码不得扩大壳层业务语义。

结构边界由 `node scripts/check-structure.js` 守门；白名单和归位状态以脚本及 ADR-0056 为准。

## 数据与交易

- 金额以整数分表达；用户展示统一调用 `formatAmount`，金额换算和展示口径集中维护。
- 交易写入改动须保持 Writer、行为编排、定时执行、批量导入和投资路径的既有接缝契约一致；新增路径前先核对各入口。
- 账户余额实时计算，不新增持久化余额口径。

## 前端与入口

- 参考数据、设备偏好、后端设置和运行时状态遵循各自领域的单一来源与归口规则。
- 会影响快捷键抑制的交互层通过现有 `App*` 封装或 `useAppDialog` 接入 Overlay Suppression；新增形态先补对应封装和注册表接线。
- 新增 IPC 命令：放入已声明并扁平再导出的命令模块，由构建扫描器生成注册表，再同步前端 API/类型并运行一致性检查。新增 HTTP-only 端点走 API 契约与 API 集成测试，不增加无关 IPC 调用面。
- 用户可见文案经 i18n；后端用户可见错误使用码化错误构造器，并同步错误模板。

## 测试、工作流与发布

- Rust 业务行为按项目约定补 BDD；HTTP-only 行为补 API 集成测试；纯内部逻辑可补单测；前端逻辑补 Vitest；BDD world 只保存跨步骤读写的状态。
- 调用 `/implement` 实施代码改动时使用独立 git worktree，并在工作树内完成验证和提交；提交后推送分支并主动在 GitHub 上创建 PR，PR 是交付终点，不自行合并；只读审查不修改、不提交；研究任务是否写入文档，以用户要求和对应 skill 为准。worktree 缺少前端依赖时先运行 `pnpm install`。
- 修改迁移、AI API 契约、数据模型或准备发布时，先判断当前提交相对最新 tag 的发布边界。无可用 tag 时，先报告无法判断发布边界，不擅自把 schema/AI API 契约当作已发布或未发布。已发布 AI API 契约和数据模型只增不改；已发布迁移的就地修改须在 migration 文件头部注明对应 CHANGELOG 条目，并在 `CHANGELOG.md` 的对应版本或 `Unreleased` 下增加 BREAKING 条目。

## 完成标准

1. 已识别受影响域、入口和分层，并读取触发条件所指向的词汇表、ADR 和专项指引。
2. 改动已放在正确接缝，适用测试、文案/错误翻译和文档同步已完成。
3. 代码或脚本改动已运行 `./scripts/check.sh`；仅文档改动已运行 `./scripts/check-docs.sh`。
4. 未解决的 ADR 冲突、无法运行的检查或未提交的改动已明确报告。
