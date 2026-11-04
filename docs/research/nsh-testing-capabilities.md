# NSH 测试钩子能力调查——UI 自动化接缝裁决材料（issue #2016 评估报告）

- 评估日期：2026-11-05（Asia/Shanghai）；**研究型评估票：只产能力清单与裁决建议、回填 #2015，不改代码、不改 ADR 正文**（裁决建议型票，无「删除即变红」负向条目）
- 触发 issue：[#2016](https://github.com/asiazhang/ledger/issues/2016)（父 spec [#2015](https://github.com/asiazhang/ledger/issues/2015) 立桩拆票①的前置项；ADR-0145 决策 6「NSH 测试钩子能力另查」的执行）
- 评估对象：NSH（Native Shell，[longbridge/nsh](https://github.com/longbridge/nsh)）自动化测试能力。裁决问题＝迁移系列接线型断言（快捷键抑制 [#2021](https://github.com/asiazhang/ledger/issues/2021)、删除刷新链路 [#2022](https://github.com/asiazhang/ledger/issues/2022)）采用 **UI 自动化**还是**源码扫描守门**（先例 [#959](https://github.com/asiazhang/ledger/issues/959) / [#961](https://github.com/asiazhang/ledger/issues/961)），及 [#2024](https://github.com/asiazhang/ledger/issues/2024) 的「UI 冒烟自动化首件」条件项是否成立
- 关联裁决：ADR-0145（决策 6 测试策略、决策 2 版本锁定）、ADR-0087（断言强度）、ADR-0084（test_support）、#2012（计数型性能门禁先例，与本报告 1.5 节 trace 门禁同构）
- 证据标注：
  - **【一手】** nsh.dev 官方文档（Testing / CLI / Linux）、longbridge/nsh 仓库源码与 PR / issue——裁决承重结论均已回原站逐条核验
  - **【社区】** 上游 issue 报告者实测（含未获官方回应项，标注 issue 号）
  - **【推断】** 本报告基于一手事实的裁决推断，均显式标注
- 版本基线：`nsh-core` / `nsh-testing` 0.4.0（2026-11-04 发布；`nsh-core` 0.1.0（10-17）→0.4.0 三周四版）【一手：[crates.io](https://crates.io/api/v1/crates/nsh-testing)】；文档与 issue 状态截至本报告日期
- 回填记录：裁决结论已评论回填 [#2015](https://github.com/asiazhang/ledger/issues/2015#issuecomment-6635787987)

---

## 0. 结论摘要（裁决建议）

**裁决：立 UI 自动化接缝，形态＝NSH headless 组件测试——Rust 侧 `cargo test` + `nsh-engine` / `nsh-testing` dev-dependency，进程内、无窗口、无平台后端。迁移系列接线型断言默认落此接缝；源码扫描守门不用于此两类接线。`--target linux` 真实 GTK 窗口测试不进常规 CI，留作 #2024 立桩验收的本地复跑工具。IME 不在可自动化面，#2023 维持人工验收。**

四条决定性理由【推断，基于下文一手事实】：

1. **接线测试可达性——先例条件不成立。** #959 / #961 走源码扫描，是因为接线住在测试不可达处（Tauri 壳 `lib.rs` 启动路径不进任何测试进程，「缺失一个调用不会让断言变红」）。NSH headless 组件测试把**整个视图树拉进 cargo test 进程**（`Headless::start(EngineConfig)` 起真实根组件），快捷键派发、弹层注册表、失效信号订阅全部在被测进程内——接线断言直接对准行为结果（页没翻、行消失），强度高于文本级扫描（ADR-0087：断言对准用户可观察结果）。
2. **零 CI 基建成本。** 官方明确 headless「runs anywhere Cargo or the CLI runs, **including Linux CI machines**」「Headless tests need no device or display, so they run on Linux runners」【一手】——纯 `cargo test`，直接进入既有 `--workspace` 测试泳道，无需 Xvfb、显示器或容器改造。
3. **原语覆盖两族接线断言所需全部动作面。** 快捷键派发（`keyboard.press`，走 `keymap.bind` 同一路径）、右键（`right_click`）、上下文菜单（`menu()` / `choose_menu_item`）、对话框应答（`on_dialog`——危险确认）、用户可见断言（`texts()` / `count()` / `shows()` / `wait_for` / `wait_for_gone`）；选择器按「用户所见」模型寻址（顶层 modal 优先、隐藏/裁剪排除、动作型步骤要求恰好一个匹配）【一手】。
4. **平台保真缺口已有归属，不重复设防。** headless 不经 GTK4 真实输入管线、渲染器与 IME——这些恰是 #2024 硬伤清单人工对表与 #2023 IME 人工验收的领地。接线断言的职责是拦住**我们的接线错误**（headless 全覆盖），不是拦住**平台的输入/渲染缺陷**（人工清单的职责）。

---

## 1. 能力清单

### 1.1 测试命令：`nsh test` 存在，headless / 真实平台双形态【一手：[nsh.dev/docs/testing-and-trace](https://nsh.dev/docs/testing-and-trace)】

- CLI：`nsh test [PATH] [--filter] [--trace] [--junit <FILE>] [--target <TARGET>] [--display <N>]`【一手：[nsh.dev/docs/cli](https://nsh.dev/docs/cli.md)】。
- **headless（默认）**：进程内运行真实引擎（同一反应式图、渲染器、组件、布局、列表虚拟化；Rust 模式无 QuickJS），60 Hz 显示帧，无模拟器、无窗口。Rust 应用的测试就是它 crate 的集成测试，`cargo test` 直接跑（dev-deps `nsh-engine` + `nsh-testing` 默认 features 不含 QuickJS）。原文：「It runs anywhere Cargo or the CLI runs, including Linux CI machines」。
- **`nsh test` 对 Rust 应用的加成**：给定含 Rust crates 的应用目录时，先跑各 crate 的 `cargo test` 再跑 TS 测试文件，合并计数与 `--junit` 报告——「`nsh test` is the one command for a Rust app」。
- **真实平台 `--target linux|macos|windows|gpui|web|ios|android`**：同一套测试文件，每个 `open` 起真实窗口、平台真实输入（「as Playwright does in a browser」）；Linux 写法原文：「`nsh test --target linux  # in GTK windows (xvfb-run -a … on a machine without a display)`」。
- 官方 CI 配方（文档 In CI 节）：Linux runner 纯 `cargo test` + `nsh test --junit`，无任何显示依赖。

### 1.2 组件级测试形态：Rust 与 TS 双 API，步骤/断言原语同集【一手：同上】

Rust 形态（对目标仓库是正身）：

```rust
use nsh_engine::{EngineConfig, Root};
use nsh_testing::{Headless, MemoryStorage};

let mut config = EngineConfig::new(Root::of::<my_app::App>());
config.storage = Some(MemoryStorage::new([("todos".to_owned(), "[]".to_owned())]));
let mut app = Headless::start(config).unwrap();
app.type_text("id:new-todo", "Buy milk").unwrap();
app.press("text:Add").unwrap();
app.wait_for("text:Buy milk").unwrap();
assert!(app.screen().texts().contains(&"1 item left"));
```

- **交互步骤**：`press` / `long_press` / `clear` / `type_text` / `submit` / `scroll` / `back` / `right_click`（桌面惯用语）/ `hover` / `keyboard.press("mod+k")`（快捷键，走应用 `keymap.bind`）/ `keyboard.down/up` / `drag` / `paste` / `choose_menu_item`（应答上下文菜单）/ `on_dialog`（应答 Alert 对话框）/ `wait_for` / `wait_for_gone` / `wait_until` / `run_for` / `fast_forward` / `set_system_time` / `set_viewport`。
- **断言原语**：`shows(sel)`、`screen()` → `texts()` / `count(sel)` / `enabled(sel)` / `dialogs()`，另有 `value(sel)`（输入值）、`spanStyle(sel)`（字符级样式：颜色/字号/字重/斜体/下划线/删除线——**语义色断言可用**）、`storage()`、`clipboard()`、`requests()`、`menu()`。无像素级快照比对（`screenshot()` 仅真实平台返回 PNG）。
- **mock 能力**：`MemoryStorage`（存储）、`StubNetwork` / `StubResponse`（按路由 stub，未匹配路由一律失败防外泄）、`Clock::manual`（手动时钟：`run_for` 逐帧推进、`fast_forward`；官方 gallery 12 个动画等待测试 28s→6s）、`on_pick`（文件选择）、host 应答。
- **步骤完成语义**（对断言时机承重）：步骤在应用渲染静默 100 ms 后返回；**后台线程的结果（文件读、Rust 任务）晚于步骤到达，须 `wait_for` 其呈现**——原文：「What another thread answers … arrives after the step resolves: `waitFor` what it shows」。域栈的 SQLite 读写发生在后台线程，此语义直接决定测试写法（见 3.5）。
- **隔离**：每个测试文件独立引擎，每次 `open` 全新引擎，测试间不共享状态。
- `examples/gallery-rs` 为官方 Rust 根组件测试示例（「one module per demo」）。

### 1.3 控件寻址与断言：Playwright 风格选择器，「用户所见」模型【一手：同上】

| 选择器 | 匹配 |
|---|---|
| `id:<testID>` | 按测试 id（Rust `.test_id(…)`） |
| `label:<无障碍标签>` | 按 `accessibilityLabel`（支持正则 `text:/^Buy/i`） |
| `text:<文本>` | 按可见文本（含输入框 placeholder） |
| `role:<role>[name]` | 按无障碍角色＋名称 |
| `view:<id>` | 按 `--trace` 输出的 view id |
| `A >> B`、`nth=<n>` | 嵌套与索引 |

- **「用户所见」寻址语义**：顶层 modal 优先，导航栈只看顶层屏；`visibility:hidden` / 裁剪排除；滚动区含可滚入内容；作用于视图的步骤要求选择器**恰好匹配一个**，多匹配即错并列出屏上文本。
- **headless 与 `--target` 共享同一视图模型**：原文「selectors, waits and queries see the same model of the app as headless runs, built from what the host draws, so they find the same views」——headless 写的测试换 `--target linux` 复跑不改写。
- **testID 的外部工具出口只有移动端**：官方映射表仅 iOS（`accessibilityIdentifier` / XCUITest）与 Android（`By.res` / UI Automator）；Linux 落 GTK widget name（源码注释：「its `testID` as the widget name (what GTK Inspector shows)」【一手：[access.rs](https://github.com/longbridge/nsh/blob/main/crates/nsh-platform-linux/src/access.rs)】），**AT-SPI / Appium 等外部工具经此寻址无官方承诺**（未找到任何文档或讨论）。
- **平台惯用语**：headless 默认以手机（`platform: "ios"`）惯用语运行；桌面能力（快捷键 `keymap`、`rightClick`）须以桌面 platform 运行——TS 侧 `open` 选项 `platform: "linux"`，Rust 侧经 `EngineConfig.environment`（`Environment.os: Os` / `idiom: Idiom`）置桌面形态【一手：[docs.rs/nsh-engine](https://docs.rs/nsh-engine/latest/nsh_engine/struct.Environment.html)】。**headless 默认值是手机惯用语——不显式置桌面形态则快捷键与右键派发不出**，此为 #2021 / #2022 断言的硬前提（精确拼法在 #2017 实现时按当时版 docs.rs 核对）。

### 1.4 已知限制

**headless 通用**：

- 文本测量用近似引擎（`ApproxTextEngine`）：「text sizes differ a little from a device … Layout logic, view counts and operation counts are the same」【一手】——布局逻辑与计数可靠，像素级尺寸不可靠。
- **open issue [#917](https://github.com/longbridge/nsh/issues/917)**（2026-11-04 提出，无官方回应）：跨测试用例节点泄漏，索引型断言可能命中先前用例的残留节点；报告者 workaround＝断言限定 `nth=0` 或精确 scope【社区】。
- **open PR [#881](https://github.com/longbridge/nsh/pull/881)** 揭示的语义缺口：headless `press` 按选择器逻辑解析目标而非物理命中——被 overlay 完全遮住的按钮「今天写的测试按了也通过」；`hit/bounds/is_interactive` 查询在补【一手：PR 描述】。→ 不要写「物理遮挡导致不可点」类断言（见 3.5）。
- `--target` 真实平台不支持 `host.handlers`（`ERR_NSH_TEST_UNSUPPORTED`）——host mock 断言只写 headless 侧。
- 无像素快照断言。

**Linux / GTK4 特有**：

- **GTK4 不能接收合成事件**：「GTK 4 cannot be handed a made-up event, so the agent works through what real input reaches」——press / 滚轮走 `input::simulate`（真实输入到达的 handler），打字用焦点字段自身 signal，back 是 Escape，截图为下次 paint 后离屏渲染【一手：[PR #232](https://github.com/longbridge/nsh/pull/232)（2026-10-21 合并）】。
- **`--target linux` 的 CI 代价**：官方 CI 需 `xvfb-run -a` 包裹，且须 `dbus-run-session`——「without one, the first GTK host on each display waited about 20 s for D-Bus autolaunch」；修后**每测试 2–6 s**【一手：PR #232 验证记录】。对比 headless：零显示依赖、毫秒级。
- **IME composition step 已随 PR #232 移除且无替代**（「the IME-composition `mark` step」列入 lost capabilities）【一手】——IME 自动化在新测试体系里**当前不存在**。
- Linux 无障碍缺口（官方 Linux 文档 Not supported 明列）：`accessible` grouping、custom `accessibilityActions` 不支持【一手：[nsh.dev/docs/linux](https://nsh.dev/docs/linux)】。
- Wayland/X11 边角：GPUI 后端截图须去 `WAYLAND_DISPLAY` 走 X11；GTK4 后端近期仍密集修 Wayland/X11 bug（如 #904）【一手：PR #232 / 上游 issue】。

**0.x 期不稳定性**：

- 三周四版（0.1.0→0.4.0）；公共 API 删除先例：PR #232 整体移除 `--script` / `NSH_SCRIPT`；PR #274 在 0.1.2 前重命名 environment / clipboard / back API；issue #161 文档与 CLI 版本漂移【一手：上游 PR / issue】。→ 测试 API 随框架共享版本锁定税（ADR-0145 决策 2 已接受），升级票须预算测试修复。

### 1.5 官方态度、路线图与性能门禁

- **Testing 是一等公民**：文档站正式章节；Introduction 页把「`nsh test` runs the app's tests headless, and the same tests in … windows … with the platform's own input」列为框架特性【一手】。
- **官方 CI 自证**：core.yml 在 ubuntu-24.04 纯 `cargo test`（含 nsh-testing 全 feature，注释明言这些 crate「none … needs a platform backend」）；linux.yml 在 Xvfb+dbus 上跑 `--target linux` 全家（counter、gallery 13 个测试文件、gallery-rs Rust 应用）【一手：[longbridge/nsh CI workflows](https://github.com/longbridge/nsh/tree/main/.github/workflows)】。
- **AI-friendly 定位**：文档明言 agent 闭环设计（headless 无设备运行、报告错误与产出视图）【一手】。
- **性能门禁思路**（与 #2012 计数型门禁同构）：官方建议「record the item counts of a few interactions … and fail when they grow. **Counts are deterministic; timings on shared CI machines are not**」；`--trace` / `EngineConfig.trace` 输出每 mount batch 的 item/ops 计数【一手】——迁移系列的翻页性能守门（#2012 的 NSH 后继）有现成接缝，本票不展开、留待相关票引用。
- 未找到独立 ROADMAP；GitHub Discussions 为空；Linux 桌面 AT-SPI 自动化对接无任何官方讨论【一手：检索结果】。

---

## 2. 裁决

### 2.1 立：headless 组件测试＝迁移系列 UI 自动化接缝

- 接缝定义：NSH 应用 crate 的 Rust 集成测试（`tests/`），dev-deps `nsh-engine` + `nsh-testing`，`Headless::start` 起真实根组件，断言只写用户可见结果（文本 / 计数 / enabled / 值）。
- 它同时吃下三类既有测试缺口：接线型断言（2.2）、冒烟（2.4）、未来的计数型性能门禁（1.5）。
- 版本税可控：测试 API 与框架同版本锁定（#2017 的 NSH 锁版本纪律覆盖 `nsh-testing`），breaking 升级时测试修复是升级票预算内工作，不是接缝存废问题。

### 2.2 接线型断言默认走 UI 自动化，不走源码扫描

- #959 / #961 的源码扫描是「接线在测试不可达处」的替代品（Tauri 壳启动路径）。NSH headless 下接线就在被测进程内、行为可断言——文本扫描降级为更弱形态，不采用【推断】。
- 源码扫描守门家族保留其既有职责不变（启动成组、后台服务、分平台门等壳层机制），本裁决不迁移它们。
- 边界诚实声明：headless 快捷键派发走引擎 keymap 路径，**不覆盖 GTK4 硬件键→NSH 转发层**；该层缺陷属平台保真面，归 #2024 人工清单（真实窗口本地复跑可抽查，见 2.3）。

### 2.3 `--target linux` 真实窗口测试不进常规 CI

- 代价不成比例：容器须加 Xvfb + `dbus-run-session`，每测试 2–6 s【一手】；上游 Linux 侧仍在密集修复期。
- 保留用法：#2024 立桩验收时在本地跑同套测试 `nsh test <dir> --target linux`（xvfb-run -a 包裹），作为「headless 语义在真实 GTK 窗口成立」的抽查证据记录进 #2015 对表——同一模型不改写测试（1.3 节）。
- 注意：Rust 侧 `cargo test` 只跑 headless；真实窗口形态由 TS 测试文件驱动（`nsh test` 起 Rust 根的 development host）。TSX 测试文件属工具脚本，不违 ADR-0145「TSX 仅容许当立桩期速写板」（非产品形态）。

### 2.4 IME 与渲染体感维持人工验收

- IME composition step 已被上游移除且无替代（1.4 节）【一手】；IME 是平台输入法栈与真实窗口的事，headless 天然不经过。#2023 的人工验收记录安排不变。
- 渲染迟滞、闪烁、fractional scaling 等「体感」断言同理——#2024 硬伤清单人工对表是唯一验收面；`--trace` 计数门禁可作补充证据（1.5 节）。

### 2.5 headless 断言不了什么（汇总，防误用）

| 断言不了 | 原因 | 归属 |
|---|---|---|
| GTK4 硬件输入→keymap 转发层缺陷 | headless 走引擎输入路径 | #2024 人工清单 + 本地 `--target linux` 抽查 |
| IME 候选框 / composing 行为 | composition step 已移除 | #2023 人工验收 |
| 渲染迟滞 / 闪烁 / 字体 / 分数缩放 | 无像素与体感断言 | #2024 硬伤清单对表 |
| 像素级样式（间距、图标） | 仅字符级样式 `spanStyle` | 人工；语义色可用 `spanStyle` 断言 |
| 物理遮挡下的不可点性 | #881：press 逻辑寻址 | 不写此类断言（3.5） |

---

## 3. 断言落点指引（后续票按此直接执行）

### 3.1 结构前置（对 #2017，接缝成立的前提）

- NSH 应用 crate 取 **lib + bin 双靶形态**：lib 导出根组件与装配入口（账本库路径等启动参数可注入），bin 只做薄启动。headless 测试经 `Root::of::<nsh_app::App>` + 注入测试库路径起真实视图树。
- `nsh-engine` / `nsh-testing` 进 dev-dependencies 并随 workspace NSH 版本**精确锁定**（`=0.4.0` 式；官方文档示例 `"0.1"` 是版滞后写法，不效仿）。
- 测试库用统一测试数据库工厂（`test_support`，ADR-0084）造数——组件测试与域单测同源造数，不另立。
- **开工首验项**：headless 默认手机惯用语、桌面形态是全部快捷键/右键断言的硬前提（1.3 节）——#2017 落第一份 headless 测试时**第一步**先验证 `Environment` 桌面形态设置可编译且快捷键可派发；拼法对不上 docs.rs 即停下回报，本报告 3.2 / 3.3 的落点指引随此复核。

### 3.2 #2021 快捷键抑制接线断言

- **落点**：NSH 应用 crate 集成测试（headless 组件测试）。
- **形态**：`Environment` 置桌面形态（1.3 节硬前提）→ 测试库造 ≥2 页数据 → 打开任一注册抑制的弹层（如危险确认）→ `keyboard.press("[")`（键位＝ADR-0140 裸键翻页；周期键 `,` / `.` 同理，ADR-0141）→ **断言用户可见的页码指示 / 首行文本不变** → 关闭弹层（ESC / `back()`）→ 同键再按 → 断言翻页生效。两族快捷键共用闸门，翻页键为代表用例、周期键同形各一条。
- **「删除即红」负向条目**：删掉 keymap 处理器中的抑制检查 → 上述测试红（弹层下按键把页翻了）。断言对准用户可观察结果（页码文本），不对准函数调用形状。
- 边界幂等 / 期间钳制的**决策半边**仍是纯 Rust 单测（步进逻辑），headless 只钉接线——与 ADR-0087 分层一致。

### 3.3 #2022 失效信号→列表重读接线断言

- **落点**：同 crate 集成测试。
- **形态**：列表呈现 N 行 → `right_click(行选择器)` → `choose_menu_item(删除)` → `on_dialog` 应答确认 → `wait_for_gone(该行文本选择器)` 或断言行 `count` 减一；同测补取消路径——应答取消 → 断言行仍在（UI 可见性）且库中仍在（测试与被测应用同进程持有测试库路径，**直接调域读 API 核对行存在**——headless 断言原语无查库能力，库态核对走 Rust 测试本体，不经 UI）。
- **「删除即红」负向条目**：删掉失效信号订阅接线 → 库中行已删而列表仍旧 → `wait_for_gone` 超时红。断言对准用户可见的刷新结果。
- **纪律**：域写与重读在后台线程，呈现晚于步骤返回（1.2 节步骤语义）——异步结果一律 `wait_for`，不断言步骤后同步可见。

### 3.4 #2024 UI 冒烟条件项判定：**成立**

- 首件＝headless 冒烟：起应用根 → `wait_for` 首屏关键文本（如「共 N 条」计数）。归入 NSH 应用 crate 集成测试，随既有 `cargo test --workspace` 泳道运行。
- 真实窗口复跑（本地、不进 CI，理由与包裹细节见 2.3）：`nsh test <dir> --target linux`，结果记入 #2015 对表。

### 3.5 通用纪律（迁移系列全部 UI 自动化测试）

1. **选择器**：优先 `id:`（`.test_id()` 稳定锚点）；`text:` 承受文案改动会脆，仅作补充。动作型步骤选择器必须唯一（多匹配即错，引擎自带）。
2. **#917 缓解**：断言限定精确 scope（`id:xxx >> nth=0`），防跨用例残留节点误匹配【社区 workaround】。
3. **#881 缓解**：不写「被遮挡即不可点」类断言；抑制断言用**状态结果**（页码没变）而非可点性。
4. **异步**：后台线程结果一律 `wait_for` / `wait_for_gone`；禁用「步骤后立刻断言」。
5. **时钟**：涉等待 / 动画用 `Clock::manual` + `run_for` / `fast_forward`，消除真实时间依赖。
6. **断言面**：`texts()` / `count()` / `shows()` / `enabled()` / `value()` / `spanStyle()`（语义色）；不用像素断言（无此能力）。
7. **性能门禁**（供后续性能票引用）：`--trace` / `EngineConfig.trace` 的 item/ops 计数做门禁（1.5 节），与 #2012 先例同哲学。

---

## 4. 风险与复核点

- **#917 / #881 修复与否不推翻裁决**：两者都有测试侧纪律兜底（3.5），且属上游活跃修复区（testing 是一等公民，官方 CI 全量自证）。
- **0.x breaking**：版本精确锁定 + 升级显式成票（#2017 纪律）已覆盖；测试 API 变更的修复成本是升级票预算内工作。
- **上游若让 headless 依赖平台后端**：与官方 core CI 自证（无平台后端、ubuntu 纯跑）矛盾，出现即值得单独评估——记为 revisit 触发条件。
- **#2024 立桩验收时复核**：若真实窗口本地复跑暴露 headless 语义盲区（如 GTK4 转发层缺陷漏网），按 0 节理由 4 的既有归属处理，不回卷接线断言形态。
