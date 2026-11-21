---
name: delete-comment
description: 按 ADR-0145 注释禁令与棘轮基线删除代码注释。Use when 用户要求删除某处注释、清理某目录注释基线、或注释禁令棘轮守门（check-comment-gate）报红需要处置注释时。含四分类判定流程（同义复述/豁口辩护/失准/决策记录）、决策记录类的 ADR 核对纪律、gate 运行与基线更新步骤。
---

# /delete-comment：注释删除与棘轮基线下调

决策权威：ADR-0145（禁令辖域、豁免闭集、棘轮机制、四分类法）。分类术语定义见 `docs/contexts/CONTEXT-testing.md`（同义复述 / 豁口辩护 / 失准 / 决策记录四词条）。豁免闭集（`// SAFETY:` 形态与 shebang 行）之外的注释只许删、不许加；本 skill 不适用于豁免形态。

## 流程

1. **定位**：`bun scripts/check-comment-gate.ts`（真实仓库）列出超线/零容忍目录与注释行；逐文件清理时直接读目标注释所在位置。
2. **逐条判定分类**（判定优先级：决策记录 > 豁口辩护 > 失准 > 同义复述；一条注释命中多类时按优先级归类）：
   - **决策记录（Decision Record）**——注释承载代码查不到的真实决策、理由或口径（依赖钉住理由、unsafe 前提、口径裁定）：走第 3 步 ADR 核对纪律。
   - **豁口辩护（Excuse）**——为 hacky 实现辩护（「这里不能改因为 X」）：登记 Excuse 跟踪 issue #2213（每条 checklist 项 = 注释位置 + 豁口描述 + 修复票链接），然后删注释。豁口修复本身另立票，不在清理中顺手修；登记与删除配对推进，不互相遗忘。
   - **失准（Stale）**——注释与代码行为不一致：以代码为准直接删。若注释描述的「应有行为」实为需求缺口或缺陷，停下报告，不静默修（范围外纪律见根 AGENTS.md）。
   - **同义复述（Tautology）**——复述代码已表达的内容：直接删。
3. **决策记录类的 ADR 核对纪律**：找到注释所属域的对应 ADR（从 `docs/adr/README.md` 索引按域定位）。已含该信息 → 直接删；未含 → 先把信息并入 ADR 再删。并入时遵守 `docs/agents/domain.md`：术语用词汇表用语、坐标收敛到域/crate 级不写文件路径与行号、变更以修订注记追加不改写历史正文。crate 注释里的独有政策并入对应域 ADR；package.json `"//"` 与 Cargo.toml 类配置决策迁 `docs/agents/dependency-upgrades.md` 或守门脚本数据面（登记条目理由字段）。信息只搬家、不蒸发。
4. **更新基线**：删除完成后跑 `bun scripts/check-comment-gate.ts --write-baseline` 重新生成 `scripts/fixtures/comment-baseline.json`。基线只许降不许升：若再生成会使某键计数高于现有基线，说明该键还有未清理的新注释，先处理再生成。基线文件与注释删除放同一提交，PR 描述写明前后计数差。
5. **验证**：`bun scripts/check-comment-gate.ts` 全绿，且 `bun test scripts/gate-mounts.test.ts` 仍绿（确认接线未动）。清理批次提交按仓库纪律跑全量 `./scripts/check.sh`。

## 纪律

- 不在删除注释的同时改代码行为——清理是纯删除 + 基线下调；行为修改独立成票。
- 不删除豁免闭集形态（`// SAFETY:` 首行、shebang）；对 SAFETY 续行（非豁免形态的普通注释行）照常分类处置。
- markdown 文档（docs/、词汇表、ADR、skill）不受禁令辖域，不在本 skill 处置范围。
- 删除后发现自己无法归类的注释：按「决策记录 > 豁口辩护 > 失准 > 同义复述」顺位核对；仍拿不准时在 PR 描述中显式提出，不擅自归为同义复述一删了之。
