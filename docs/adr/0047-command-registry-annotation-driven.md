# ADR-0047: 命令注册单一来源——注解即注册 + TS 调用面双向全等校验

- 状态：已接受
- 日期：2026-09-01
- 作者：Ledger 项目
- 关联：issue #312（spec，2026-02 grilling 定稿）、#315（实施票）；词汇表不收录本机制（见「代价与边界」）

## 背景

加一个后端命令要手工把「命令存在」同步到多处清单：Rust 侧 `lib.rs` 的
`invoke_handler(generate_handler![…])` 大清单（实施前 78 项），近半年该文件几乎全部提交都是
「追加一行」；TS 侧 `src/api/index.ts` 调用面逐方法追加。任一侧漏改没有校验拦截——漏注册是
运行时 404，漏 TS 方法是编译期才发现的类型缺失，都靠评审人肉。注册知识在两份手工清单间复制，
局部性为零。

## 决策

1. **Rust 侧单一来源 = `#[tauri::command]` 注解本身（注解即注册）**：既有
   `src-tauri/build.rs` 在 `tauri_build::build()` 前文本扫描 `src/commands/**`——裸
   `#[tauri::command]` 行 + 紧随的 `pub fn <name>` / `pub async fn <name>`——生成
   `$(OUT_DIR)/commands_registry.rs`：按（域，命令名）字典序排列的
   `tauri::generate_handler![commands::<name>, …]`，包在具名函数 `tauri_commands_handler`
   里（flat 路径风格与原手工清单逐字同风格，依赖 commands 扁平 pub use 链解析，注册路径
   零变化）。`lib.rs` 以 `include!(concat!(env!("OUT_DIR"), …))` 接入，
   `invoke_handler(logged_invoke_handler(tauri_commands_handler()))` 一行，手工清单删除。
   生成排序确定（BTreeMap 键序、文件遍历按路径排序），diff 稳定；生成物不提交入库，
   正确性由编译期保证，PR 审查对象是注解 diff 与 TS 方法 diff。
2. **扫描器 fail loud（宁可编译/校验失败不可静默漏注册）**：注解行之后不是 fn 定义行（带参注解、
   cfg 条件命令、注解与 fn 之间插属性行、文件以注解结尾）、命令名跨文件重复、扫不到任何命令，
   build.rs 一律 panic 使构建失败；TS 校验脚本同界——任一侧空集、扫描器不认识的形态、
   双向孤儿均非零退出。静默漏注册正是本决策要消灭的失败类，扫描器自身不允许引入同类静默。
3. **扫描器维护边界**：只认裸注解 + 紧随 `pub fn` / `pub async fn`（实施时实测全库 78 处
   全部为该形态，其中 2 处 async：基金按代码添加、持仓价格增量同步——spec 成文时的
   「全库均为 pub fn」计数不精确，扫描器自始支持 async）。带参注解与 cfg 条件命令是未来
   扩展点：遇到即构建失败，届时同步扩展 build.rs 与 TS 校验脚本两处扫描规则（同一规则写两遍
   是接受的对价，见决策 5 的「平凡模式无漂移空间」）。
4. **TS 侧手写调用面保留 + 双向全等校验**：`src/api/index.ts` 调用面本体不动（经删除检验，
   命令名字符串、入参命名规约、返回类型 import 的知识仍在调用面挣饭吃）；新增
   `scripts/check-commands.js`（Node，无依赖，默认 check 模式）比对「Rust 注解命令集 ↔
   invoke('命令名') 集」双向全等，任一方向孤儿即非零退出并列出差异项。例外清单机制不做
   （YAGNI，实施时 78 = 78 零孤儿，严格规则零迁移成本）。校验精度边界 = 命令名存在性；
   参数名 / 返回 serde 形状匹配不做（跨语言类型生成 out of scope），形状层仍靠评审 +
   vue-tsc 的 TS 内部自洽。
5. **门槛挂载**：`node scripts/check-commands.js` 挂入 `scripts/check.sh`（与
   check-docs.sh 相邻）；CI `build.yml` frontend job 补跑该校验与 check-docs.sh（CI 此前
   不跑 check.sh，文档校验的 CI 缺口顺手一并补）。测试策略按「只测外部可观察结果」：校验
   脚本以进程退出码与输出为接缝做 Vitest 测试（真实仓库活体全等 + 夹具正反例），
   build.rs 生成器不单测——编译通过即注册表可达性的证明，命令集行为零变化由生成物与
   原清单逐命令 diff 验证（实施时执行，78 = 78）。
6. **机制术语在 ADR 内定义，不进 docs/contexts/**：命令注册接线是跨域基础设施，不属任何
   自然域，不强行造域；「commands_registry / 注解即注册 / 双向全等」等机制名词以本 ADR
   为唯一解释处。

## 理由

- **为什么 build.rs 生成而非脚本 + 提交生成物**：脚本方案需要「生成物新鲜度」门禁兜底
  「忘重新生成」失败类，派生物 diff 审查零增量信息；build.rs 让生成物永远新鲜（编译期保证），
  零门禁、零提交物，且注册时点与编译时点天然同一。
- **被排除路线（已验证机制，防重提）**：
  ① inventory 类宏自动收集——运行时注册，违反「编译期展开、无运行时开销」；
  ② 域清单宏嵌套喂 `generate_handler!`——它是 proc_macro（syn 解析路径列表），收到的
  token 不展开嵌套宏，直接解析失败；
  ③ 域 handler 链式分派——未匹配分支 `return false` 且 `Invoke` 按值单次消费，链式在
  move 检查下不可行，「名字守卫 + handler」每域两处清单比现状更糟；
  ④ 脚本生成 + 生成物提交 + 新鲜度校验——被决策 1 的 build.rs 方案翻案（见上条）。
- **为什么扫描器 fail loud 而非尽力匹配**：扫描器的价值在「注解集即全集」的完备性承诺；
  对不认识形态保持沉默会把承诺变成假象（新形态命令静默缺席注册表，回到运行时 404）。
  构建失败把维护边界变成显式的一次性决策点。
- **为什么两条扫描规则（Rust 侧 build.rs 与 Node 侧校验脚本）写两遍是可接受的**：两者匹配
  的都是同一文本形态（裸注解 + 紧随 fn 行）——平凡模式没有漂移空间；真出现分歧形态时
  fail loud 机制会先拦住（任一侧对未知形态拒绝），不会静默分叉。

## 代价与边界

1. 扫描规则文本存在于 build.rs 与 scripts/check-commands.js 两处，扩展时须同步修改
   （失败模式是构建/校验显式报错，不是静默漏配）。
2. 注解与 fn 定义之间不允许任何其他行（含 doc comment 与内层属性）；这是当前全库事实形态，
   收紧带来的表达力损失在需要时由扩展扫描规则赎回。
3. `cargo:rerun-if-changed=src/commands` 后，build.rs 只在命令目录变化时重跑（build.rs
   自身变更仍必然重跑）；生成物进 lib.rs 的依赖由 rustc dep-info 自动追踪，无需额外机制。
4. 命令注册热点的另一半——BDD 测试脚手架的注册与快照字段膨胀——由 #312 的兄弟票承接，
   本决策不触及 e2e 组织结构。
5. 校验精度边界 = 命令名存在性：TS 方法名与 Rust 命令名的映射质量、参数/返回形状仍靠
   评审与 vue-tsc。

## 实施 recorded（验收走查）

- 新命令三步走通：域内注解函数 + `cargo build` + TS 加方法，lib.rs 零改动（scratch 命令
  验证后删除，命令集回到 78）。
- 删 TS 方法 → 校验失败列出孤儿；TS 调不存在命令 → 失败列出孤儿；均非零退出。
- 生成注册表与原手工清单逐命令 diff 一致（78 = 78，含 2 个 async 命令）；clippy/fmt/
  vue-tsc/cargo test 全绿。

## 相关 ADR

- ADR-0032（连接层统一写入口）与本决策同属「接线收口」系列：写路径收口到接缝，注册收口到注解。
- ADR-0046（包管理器切 pnpm）：CI frontend job 的执行器即 pnpm；本决策新增的 CI 步骤
  与包管理器无耦合（纯 node 脚本）。
