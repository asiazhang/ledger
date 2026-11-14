# Ledger 视觉规约（DESIGN.md）

> 本仓视觉唯一真源（ADR-0147 决策 2）：叠于 NSH 官方设计指南（nsh.dev/zh-CN/docs/design-guides）之上——指南管「无论长什么样必须为真的事」，本规约管「本仓长什么样」，相左时本规约赢。
>
> 现状：骨架初版（spec #2099 票 #2100）。数值采用官方规范值；代码取值唯一来源是壳根包设计常量模块（`src-app/src/design.rs`，字阶/间距/圆角）与主题令牌表（`src-app/theme.json`，色值）。视图收口票（#2101–#2107）已把存量裸值逐批换到这两处；圆角规格外档（框架 `rounded-md` 6pt）随 review 收编退役到三档令牌类；零裸值守门随 #2108 挂入检查脚本（`scripts/check-bare-font-size.ts`，挂载登记见 `scripts/gate-mounts.ts`）。
>
> 冲突裁决：规约与代码不一致时，按可验证行为修正规约（仓库通则）。

## 1. 原则

- **自有观感**：账本配色（主题令牌表定调），不追随任何参照应用的观感；布局保真口径仍按应用壳域词汇表（布局靠拢 Web 参照前端，观感自立）。
- **信息密集优先**：账本是 information work 密集应用，密度与可扫描性优先于装饰；每屏层级靠字号/字重/留白表达，不靠阴影与描边堆叠。
- **亮暗同为出厂态**：每个自定义表面在亮、暗两态下验看；不得假设前景是黑/背景是白。
- **零可感知迟滞**：视觉落地不得引入等待（AGENTS.md 体验取舍标尺）。

## 2. 色彩

- **一切颜色走主题令牌**（ADR-0147 决策 1）：颜色按语义角色从主题表解析，消费面走 `crate::theme::color(..)`（Rust）与令牌类（`bg-primary`、`text-expense` 等）。应用界面不出现裸 hex / `rgb()` / `hsl()` / 调色板类名（`bg-amber-500`）；缺角色先补主题表再用。
- **令牌闭集**（`src-app/theme.json`，shadcn cssVars 格式，亮暗双态）：
  - shadcn 标准 UI 语义名：`background`/`foreground`、`card`、`popover`、`primary`、`secondary`、`muted`、`accent`、`destructive`（+ 本仓补 `destructive-foreground`）、`border`、`input`、`ring`、sidebar 八件族、chart-1…5（未定制，沿用出厂）；
  - 本仓自有令牌：金额语义色九档 `expense`/`income`/`refund`/`transfer`/`buy`/`sell`/`convert`/`split`/`dividend`；`warning`（+ `warning-foreground`）；`success`（亮 `#18a058` / 暗 `#63e2b7`，达标/成功态）；走势主色 `line`；图表系列 `chart-series-1…12` 与 `chart-uncategorized`；圆角阶四档（见 §4）。
  - **单值档**（亮暗同值的令牌，dark 段不重复写）：两种书写形态——写 `theme` 段（亮暗共用，如圆角阶四档），或只写 `light` 段（引擎按「覆盖式只写差异」解析：`dark` 未写即沿用 `light` 值，如 `line`、`chart-series-*`）。两形态同效，别在 `dark` 段抄同值。
- **分层正交**：主题表答「出厂观感是什么」；设备级亮暗强指（外观设置，#2053 接缝）答「这台设备用亮还是暗」——清单不设 `appearance` 键，两层互不取代。
- **强调克制**：每屏一个主操作（primary）；警示色（destructive/warning/expense）保持稀缺；金额语义色只用于金额与其直接载体。

## 3. 字阶

五档（pt），常量来源 `design::font_size`（官方规范值，不发明第二套数）：

| 用途 | 档 | 常量 |
|---|---|---|
| 辅助小字下限（时间戳、计数、overline） | 12 | `CAPTION` |
| 次要正文（表格行、表单标签、菜单、按钮） | 14 | `SECONDARY` |
| 正文基准 | 16 | `BODY` |
| 小节标题 | 18 | `LEADING` |
| 页面/卡片标题 | 20 | `TITLE` |

- 阅读文字不小于 12pt；层级靠字阶与字重（每屏 3–4 个字号、两种字重），次要信息走 `muted-foreground`，不靠缩字号。
- 可比较数字（时刻、计数、金额列）用等宽数字（`tabular-nums`）。

## 4. 形状与间距

- **圆角阶**（pt），常量来源 `design::radius`（主题令牌 `radius`/`radius-card`/`radius-panel`/`radius-inset` 同值）：控件/弹层 8 → 卡片 12 → 大面板 16；小控件/标签取约半档；全圆（胶囊/圆形）用框架 `rounded-full`。8pt 档的两种书写形态 `rounded-lg` 与 `rounded-(--radius)` 同指 `--radius` 令牌（框架档由 radius 派生），书写不强制，值同源即合规；框架 `rounded-md` 是 radius−2px 派生的 6pt 非档，不使用。
- **同心规则**：嵌套处内层圆角 ≈ max(0, 外层圆角 − 层间缝)（`design::radius::concentric`）；重复出现的特殊组合应在主题表立具名档（先例：`radius-inset` 8）；嵌套裁剪也可由外层 overflow 承担。
- **间距语义阶**（pt），常量来源 `design::spacing`：2（光学校正）→ 4（控件部件）→ 8（近亲控件）→ 12（同组）→ 16（同 section 不同组）→ 24（不同 section）→ 32（主要区域边界）。
- padding 归组件、gap 归父级，不重复承担；横向优先 `ps-*`/`me-*` 逻辑边。

## 5. 表面与层级

- 基础表面平坦：区域靠背景差异（background/card/muted）与 separator 表达；阴影只给浮层（modal/popover/menu）并逐级增强；不给每张卡片加阴影。
- 弹层家族共用一种主题化表面（popover 令牌 + 边框 + radius + 一种阴影），家族成员不漂移。
- 焦点环画在边框外，圆角 = 控件圆角 + 环宽；祖先裁剪处留出环空间。
- Badge 克制；状态色（destructive/warning/expense/income）保持稀缺。

## 6. 组件规格

- **控件高度**（桌面）：紧凑控件 24–32pt（默认 32pt）；图标 14–16pt；行高 28–36pt；输入框与按钮同档（20/24/32pt）。给组件一个 `size`，字体/图标/padding/命中区一起变，只改外框不算完整。
- **弹层家族单一表面**（`popover` 宿主的家族成员——下拉选项弹层、口径气泡、确认气泡、模态卡片）：`bg-background` + `border-border` + `rounded-(--radius)`（控件/弹层 8pt）单一主题化表面 + 一种阴影（浮层 `shadow-md`、模态 `shadow-lg` 逐级）；弹层内边距按内容密度取语义阶（紧凑列表 `p-1` 4pt、气泡 `p-2`/`p-3` 8/12pt）。家族成员不漂移。
- **壳内 ui 层封装件**（#2107 成文；变体 + 尺寸有限 API 的取值底座 = `design.rs` 常量 + 本节档位，视觉值内聚在封装件内，调用方零散写）：
  - 字阶：弹层/对话框标题 `BODY` 16；控件标签、选项行、气泡说明与对话框正文、按钮 `SECONDARY` 14；辅助字形（▾/✕）、勾选/清除标记、空态 `CAPTION` 12。
  - 圆角：弹层卡与触发件 `rounded-(--radius)` 8pt；卡内嵌套警示块等小面 `rounded-sm` 4pt（同心规则，§4）。
  - 内边距：选项行/菜单项/气泡小钮 `px-2 py-1`（8/4 语义阶）；输入框 `px-2 py-1` + `rounded-(--radius)`；对话框动作钮 `h-8 px-3`（32pt 默认档，§控件高度）；清除钮 `px-1 py-0.5`（4/2）。
  - 颜色：全部经 `crate::theme::color` 单点消费（`foreground`/`muted-foreground`/`destructive`/`warning` 及各自 `-foreground`），实色钮前景走对应 `-foreground` 令牌，封装件零本地色常量。
  - 件清单：`modal`（模态弹窗）、`popover_tip`（口径气泡）、`popconfirm`（轻量确认气泡）、`app_select`（单选下拉）、`multi_select`（多选）、`pinyin_select`（拼音可搜下拉）、`combo_select`（自由文本可搜输入）、`date_select`/`date_option_select`（日期选择）、`danger_confirm`（危险确认，ADR-0078 分级）、`open_context_menu`（原生上下文菜单，平台观感不主题化）。
- 组件规格随壳内 ui 层逐件成文落本节；未成文的组件以现状为基线，触碰时对齐本规约并补规格。

## 7. 视图零裸值纪律与例外挂账处

**纪律**：视图层零裸值——颜色走主题令牌、字号走字阶常量、圆角与间距走尺度常量；新增代码不得出现裸 `text_size(px(…))` 直调、裸 hex/`rgb()`、调色板类名与散写圆角/间距值。守门已挂入（#2108）：`scripts/check-bare-font-size.ts` 扫描壳根包（`src-app/src`）`text_size(px(<数字字面量>))` 直调即红——字号取值唯一来源 `design::font_size` 字阶。扫描面内的 `#[cfg(test)]` 单元测试块与设计常量模块不豁免（夹具写裸字号同样制造第二套数）；`src-app/tests/` 目录不在扫描面（headless 集成测试不辖本门，其探针断言值另受「唯一来源派生」纪律约束——字号由 `design::font_size` 常量派生，色值由 theme.json 真源解析派生（#2113），见 §8）；注释/字符串引证经词法掩码豁免；命中质量不达预期的合法在场经脚本内 `REGISTERED_EXCEPTIONS` 严格相等校验登记并挂账本表。

**例外挂账处**（裸值/特殊值必须在此成文，未挂账即违规）：

| 例外 | 范围 | 状态 |
|---|---|---|
| 图表数据色映射（分类 id 散列 → 系列槽位） | `chart-series-*` 槽位序 = 装配层口径常量；数据即颜色的映射逻辑豁免（色值本体已收编令牌表） | 已收编（#2100） |
| 走势底面积 15% 透明度、网格线降透明 | 图表视觉微调值，随 `line`/`border` 令牌消费 | 已挂账（#2100） |
| 弹层遮罩压暗（`bg-black/50` 模态、`bg-black/30` 非模态抽屉） | 遮罩半透明黑是压暗 scrim 惯例值；主题表暂无 scrim 档，立档后收编令牌 | 待收编（待主题表立 scrim 档） |
| 存量裸值（`text_size(px(..))` 643 处、调色板类 431 处、实色按钮白字等） | expand–contract 序列的 contract 半边：#2101–#2107 逐视图收口 | 已收口（#2101–#2107；守门随 #2108 挂入） |
| 密集表格行高/密度微调 | 表格行高若需偏离 §6 档位，落地时在本表挂账并说明密度依据 | 未发生 |
| 主题表与兜底常量 | 裸色值的唯一合法住所：`src-app/theme.json` 与 `crate::theme` 缺失兜底（`theme::color` 单点内） | 已收编（#2100；视图层散落兜底随 #2104 与 review 收编清零） |

## 8. 验收与对表

- 亮暗双态 headless 组件测试断言语义色令牌生效与外观强指呈现（`cargo test -p nsh-app`）。
- 出厂观感对照截图属人工对表子集（亮/暗两态各一张，对照「非默认 neutral」）。
