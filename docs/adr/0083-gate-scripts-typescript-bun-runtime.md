# ADR 0083: 守门脚本工具链形态——Python + uv 零工具链依赖、强类型入门槛

- 状态：已接受（形态历经 JS → TS+Bun → Python+uv 两轮换代，现行态见决策节，沿革见演进脉络）
- 日期：2026-09-06（现行态收束：2026-11-20）
- 作者：Ledger 项目
- 关联：issue #734（spec，grilling 定稿）、#737（实施收窄票）、#1991/#1995（守门测试运行口径）、#2174（Python + uv 迁移）；关联 ADR-0047、ADR-0049、ADR-0056、ADR-0071、ADR-0148

## 背景

`scripts/` 下的守门脚本维护着全仓最关键的文本扫描规则（白名单、正则、掩码），类型错误只能靠运行时暴露；同时脚本自身的测试长期游离在类型检查与门槛之外。本 ADR 是守门脚本工具链形态的唯一解释处（先例：曾改判 ADR-0047 的运行时表述，后被各 ADR 反向引用）。

## 决策（现行，#2174 迁移后）

1. **运行时 = uv + Python，零工具链 manifest**：门宿主行 = `uv run scripts/<name>.py`；`check.sh` 前置 `command -v uv` 检测，缺失即显式中文报错退出，不静默降级——降级会把「门槛在什么运行时上跑」变成机器相关的偶然事实。仓库无 pyproject/requirements：门脚本全部 stdlib-only，唯一第三方依赖（mypy、pytest）以 PEP 723 内联声明钉死版本、各自单一事实源（`check_scripts_types.py` / `run_tests.py`），升级即改对应脚本。
2. **强类型入质量门槛 = mypy --strict**（首门：check.sh 首位 + CI gates job 首步）：移植代码全量显式标注，门槛水位与原 TS strict 套件同精神；pyright 排除（依赖 node 运行时，与「仓库零 JS 工具链」目标相悖）。
3. **守门脚本测试 = pytest，唯一口径 `uv run scripts/run_tests.py`**：测试文件 `test_<name>.py` 与所测脚本同目录住；包装测试 spawn `uv run <script>` 与门槛调用同款——「测的就是门槛路径」（外部可观察接缝：退出码与输出即断言面）。挂载登记与「删除即变红」核对住 `gate_mounts.py`（issue #1682）。
4. **判定语义跨载体逐字保留**：历次载体换代（JS → TS → Python）均以「判定语义、白名单/豁免清单、正则、fail-loud 空集哨兵、退出码、zh-CN 输出文案逐字保留」为移植验收基线；跨语言双源登记（如词法掩码 Rust ↔ Python，共享语料夹具双侧消费）随之换载体、语义不变。
5. **术语**：「守门脚本 / 门槛脚本」沿用各 ADR 既有定义，不进词汇表。

## 理由、否决方案与已知边界

- **为什么零依赖自持性优先**：守门脚本属质量地基，运行时升级应是一次显式评审（运行时兼容层仍在快速演进），CI 与本地同钉一版消除「本地绿 CI 红」的运行时漂移面。历史先例：TS 时代为保零 npm 依赖否决 tsx devDep、否决 node strip-types（erasable-only 子集受限）；Python 时代延续为「零工具链依赖」——stdlib-only + 两处 PEP 723 内联声明同构于原「零 npm 依赖、类型库只服务类型检查」。
- **为什么 spawn 测试而非 import 被测函数**：守门测试只测外部可观察结果（ADR-0047 先例），以退出码与输出为接缝；同款调用覆盖真实门槛路径。
- **被排除路线（防重提）**：pyright（node 依赖）；js 系运行时整体（仓库零 JS 工具链目标，#2174 定案）；tsx / node strip-types（TS 时代否决，理由留档）。
- **已知边界**：跨运行时对 `node:`/stdlib 行为差异的可能——脚本只依赖 fs/path/子进程稳定子集，换代时以输出逐字一致性实跑核验；运行时升级须重跑一致性核对。守门测试经 spawn 起真进程，比内存测试慢（外部可观察接缝的收益优先于这层耗时）。

## 演进脉络

| 日期 | 变更 | 关键裁决 |
|---|---|---|
| 2026-09-06（本 ADR 初版，#734） | 四守门脚本 JS → TypeScript，运行时选 Bun（三选一：strip-types/tsx/Bun）；类型检查（tsconfig.scripts.json + vue-tsc）入门槛；门槛范围收窄先绿后扩 | 零 npm 依赖性质保留，运行时由 node 改判 Bun；tsx 与 strip-types 否决留档 |
| 2026-10-31（#1991） | 守门测试唯一运行口径 = `bun test scripts/`，vitest 收集范围不再含 scripts/ | 「测试在哪个运行时跑」与「被测脚本在哪个运行时跑」对齐；frontend-test 分片不再依赖 bun |
| 2026-11-01（#1995） | 测试导入显式 `from "bun:test"`，消除 vitest 隐式别名依赖；types 面新增 bun-types | 导入与唯一运行口径同名同源 |
| 2026-11-20（#2174） | **全量换代：TS 载体退役，迁移 Python + uv**——11 个脚本逐语义移植（bun test 用例为验收基线），tsc 改 mypy --strict，bun test 改 pytest，setup-bun 改 setup-uv | 动因：Web 参照前端退役（ADR-0148）后 bun 仅剩守门脚本角色；拷问会话裁决 A 方案；零 npm 依赖性质延续为零工具链依赖 |
