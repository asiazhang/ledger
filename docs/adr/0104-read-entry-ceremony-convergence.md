# ADR-0104: 壳层读路径仪式收敛——统一读入口 `read_entry`（锁仪式单点 + 源扫描守门）

- 状态：已接受（spec #1009 grilling 定稿；两批均已落地：①`read_entry` 入口 + 单测（#1028），②51 处机械替换 + 守门上线（#1029）；结构白名单已在 `check-structure` 登记，源扫描接线在 `signals_cross_check` 的 IPC/HTTP 手写锁行反向守门与 `IPC_READ_ENTRY_EXCEPTIONS` 死条目核对）。修订：模块住址自 ADR-0111 起改为壳机制暂住区，#1108 壳层收敛已迁出至根包 `src/shell_support` 正住址（见 ADR-0111 修订注记），组合契约（阻塞线程池投放 + 锁失败归一化 + span 归因）不变；豁免清单自 issue #1284 起 7 条——`get_sync_channel_checkpoint`（#1283）与 `publish_sync_checkpoint`（#1284，配置读取迁入读入口，仅余快照产出短锁）的形状 C 登记相继撤销（见决策 6 修订注记）；消费句柄类型经 ADR-0125 决策 1/4 更替为门面读句柄（issue #1410 落地）——仪式语义与豁免清单原样，组合执行环境由阻塞线程池改为门面读 DB 线程；写槽读形态入口 read_entry_on_write 承载 ADR-0117 甄别结论的四命令（读形态但闭包内含惰性写，必须走写连接）——「只留一种可模仿形态」的判据不变：常规读命令一律走 read_entry，该入口是甄别结论的显式承载而非第二可选形态。修订注记（2026-09-22，spec #1683 grilling 裁决）：架构评审候选 12 重提删除测试，裁决**保留转发层**——转发体自 ADR-0125 句柄更替后实测已空，删除测试通过但残余资产全在非代码维度（对仗命名、不变量文档住所、守门 token 住所），持有成本远低于迁移面；壳层 `db.run`/`run_raw` 直呼系既有合法形态，读侧「只留一种可模仿形态」为软约束（守门 + 评审兜底）；变深三候选诚实性检验见决策 2 修订注记
- 日期：2026-09-11
- 作者：Ledger 项目
- 关联：spec #1009（架构走查 2026-09-11 候选 7，grilling 定稿与本 ADR 同日）；ADR-0073（统一写入口先例，本入口与之对称；其决策 6「读侧不跟进」由本 ADR 翻案，0073 状态行已加修订指针）；ADR-0069（`run_db` 阻塞线程池与 span 归因的唯一拥有者，read_entry 组合之而非替代）；ADR-0056（壳层职责与基础设施白名单，白名单追加一行）；ADR-0047（命令注册扫描面不变）；ADR-0032（置脏豁免单点不动——读路径无置脏维度）

## 背景

ADR-0073 把写路径仪式收敛进 `write_entry` 时，对读侧留下明确搁置：「读命令剩余仪式约两行且无身份/信号维度，待真出现第四件读后横切事项再议」。架构走查（2026-09-11）候选 7 重提读路径：写侧收敛后，读侧成为双壳中仅存的逐处手抄仪式——锁获取与失败映射、span 归因、阻塞线程池投放，每个读命令/handler 各写一遍，锁失败映射的字符串转换是全仓复制量最大的一行。

grilling 实测对账（rg 可机械重放；同时勘误 issue 初稿的量化声称）：

| 口径 | issue 初稿 | 实测 |
|---|---|---|
| commands 下 `run_db(` 调用 | 116 次 | **80 次**（116 ≈ 含 use/注释的出现行数 119） |
| handlers 下读仪式 | 7 次 | 7 次 |
| 壳层手写标准锁行 `.lock().map_err(\|e\| AppError::Db(e.to_string()))` | 62 处 | **60 处**（另有 `db::write` 内部 1 处合法单点与 boot 1 处非锁行映射，宽模式合计 62） |
| 机械可迁（一行标准锁 + 单域调用） | 隐含全部 | **49 处**（IPC 42 + HTTP 7） |

两壳 87 处 `run_db` 调用按闭包形状三分类：**形状 A**（机械可迁）49 处；**形状 B**（ADR-0073 例外白名单写命令的调用点）11 处（9 处手写标准锁、2 处不触 conn；其中 `audit_balance_cache`/`repair_note_pinyin` 闭包体与形状 A 完全同构）；**形状 C**（非机械）27 处（21 处闭包完全不触 conn——账本注册表文件 IO、加密文件转换、钥匙串、备份探测等阻塞工作；2 处 boot 多路径锁编排；4 处 sync_channel 锁内网络/文件/`mut` 守卫，注释明言刻意直连）。issue 初稿「IPC 拿 `State<AppState>`、HTTP 拿 `State<ApiState>`」与代码不符：IPC 状态类型为 `DbState`，HTTP handlers 经 `FromRef` 直取 `Arc<Mutex<Connection>>`——两壳到仪式边界的类型早已同一。

## 决策

1. **翻案 ADR-0073 决策 6 的读侧搁置，重开判据显式更换**：「第四件横切事项」不可数（锁映射、双壳形状、线程池、span 各自算不算一件见仁见智），改用可客观检验的判据——**复制是否规模化**（87 处调用点、60 处手抄锁行）**且失败类有先例**（「新命令抄错仪式、AI 助手模仿样板带回」正是 ADR-0069 立项时的同款失败类）。ADR-0073 正文不重写，状态行加修订指针（ADR-0056 被 ADR-0059 修订的先例）。
2. **统一读入口 `read_entry`：span 归因串、连接句柄、读闭包进，其余内化**。落点为新基础设施模块 `read_entry.rs`，与 `write_entry.rs` 并列（ADR-0056 守门白名单追加一行）：

   ```rust
   pub async fn read_entry<T, F>(
       span: &'static str,
       conn: Arc<Mutex<Connection>>,
       f: F,
   ) -> Result<T>
   where
       T: Send + 'static,
       F: FnOnce(&Connection) -> Result<T> + Send + 'static,
   {
       run_db(span, move || {
           let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
           f(&conn)
       })
       .await
   }
   ```

   内部组合 `run_db`（执行线程与 span 传播照旧，ADR-0069 决策 3），锁行与锁失败映射体内单点。命名与 `write_entry` 对仗，保持「只留一种可模仿形态」（ADR-0069 立项动机之一）。

   > **修订注记（spec #1683 grilling 裁决，2026-09-22）：保留转发层——不删除、不变深。** 架构评审 2026-09-22 候选 12（强度 Speculative、无事故记录）重提删除测试：ADR-0125 句柄更替（#1410）后转发体实测已空——`read_entry` 即 `db.run(span, f)` 一行、`read_entry_on_write` 即 `db.run_raw(span, f)` 一行，外部调用 57+4 处，删掉后仪式复杂度不重现。删除测试因此**通过**（interface 无语义资产），残余资产全在非代码维度：与 `write_entry` 的命名对仗、读协议不变量文档住所（模块头四条语义锚）、反向守门计数 token 住所。裁决**保留**：11 行转发 + 文档的持有成本 ≈ 0，远低于迁移面（61 处改写；守门 token 改扫 `.run(` 有 axum `next.run(` 别名盲区先例；ADR 修订面）。同票查明两点留痕：① `DbReadHandle::run` / `DbWriteHandle::run_raw` 为 pub 且在命令参数表内，壳层直呼系既有合法形态（commands 模块 settings / backup / sync_channel 豁免命令先例），读侧「只留一种可模仿形态」为由来已久的**软约束**（守门只在手写标准锁行出现时触发，其余靠评审兜底），删除或保留都不改变其强度；② 变深路线经诚实性检验出局——超时无真实需求（`job_gate` 限时弃权消费方全在写槽备份调度）、只读纪律已在连接层结构性成立（读连接 `SQLITE_OPEN_READ_ONLY` 打开 + readonly 测试钉死，转发吸收无处着力）、快照一致性在 rollback journal 下有真实多语句窗口（`realized_pnl_summary` 总量≠分量和等），属域层 / SQL 取舍，另案跟踪（#1699）。冗余线程名断言测试（门面已有同款 ×2：`read_job_runs_on_read_thread` 与句柄路由测试）删除随 #1700。
3. **跨壳类型零机制**：直收 `Arc<Mutex<Connection>>`——IPC 侧 `db.conn.clone()`、HTTP 侧 handler 经既有 `FromRef` 直取，与 `write_entry` 的 58 个调用点同款证据。泛型、trait、薄 adapter 均属为不存在的问题引入抽象。
4. **行为等价由构造保证，验收沿用既有口径**：JoinError→`AppError::Io`、锁中毒→`AppError::Db`、业务 `Result` 原样传播、span 归因串逐字节不变；BDD e2e 与 API 集成测试**断言**零改全绿 + `read_entry` 纯单测钉住仪式（闭包执行、错误传播、panic 归一化）——ADR-0069/0073 的成熟验收纪律照抄。
5. **迁移面 = 形状 A 的 49 处 + 2 处同构白名单命令，纯机械替换**（`audit_balance_cache` / `repair_note_pinyin` 迁入读入口，写侧白名单身份保留）；B/C 组其余 36 处（超时锁、分支锁、`mut` 守卫、锁内网络/钥匙串、多路径编排、不触 conn 的阻塞工作）原地保留、零顺带整理，逐条进独立豁免清单并附动机注释。
6. **读侧反向守门，ADR-0073 决策 5 同形**：测试期源码扫描（与写侧扫描同处安置，不扩 build.rs），掩码注释与字符串后，壳层命令/handler 函数体内出现标准锁行模式而同函数体无 `read_entry(` / `write_entry(` 即红；豁免进**独立新清单**（迁移后预期 9 条：backup 三命令、settings 一条、sync_channel 四条、restore 分支锁等，逐条附动机注释），与写侧 `IPC_WRITE_ENTRY_EXCEPTIONS` 分开——两者核对的知识不同（写身份 vs 锁仪式）。守门不降级为评审：本票要消灭的失败类（手抄仪式回潮）恰是评审最守不住的类。

   > **修订（issue #1283）：撤销 `get_sync_channel_checkpoint` 的形状 C 登记（9 → 8 条）。** 该命令持锁的唯一理由是读通道配置（`app_settings` 单行读），manifest GET 与锁内顺序性无涉——不在 SyncRound 轮次内，无任何「须在锁内调用」契约背书，且网络慢或超时时阻塞全应用读写（ADR-0069 决策 4 正面冲突）。修复形状：锁内只读 `configured_channel`（走读入口短锁）、manifest GET 出锁（阻塞线程池）；返回形态与错误码不变。豁免清单对应条目随之撤销，反向守门改由 `read_entry(` 调用点满足。其余三条 sync_channel 形状 C 登记不受影响（`publish_sync_checkpoint` 的快照产出与位点同刻成对、`bootstrap_sync_from_channel` 的整库换入 `mut` 守卫、`get_sync_status` 的锁内文件探测仍属结构性理由）。

   > **修订（issue #1284）：撤销 `publish_sync_checkpoint` 的形状 C 登记（8 → 7 条）。** 该命令的成对约束只为快照产出段（`create_checkpoint`）而立，封包（KDF）、检查点上传与 manifest 换指针同属网络 / CPU 段、不消费连接（判据同 ADR-0120）——旧形状「持锁触网刻意直连」随 #1284 修复退役。修复形状：通道配置读取走读入口短锁、主连接锁只盖快照产出、封包与三次通道往返出锁；返回形态与错误码不变。豁免清单对应条目随之撤销，反向守门改由命令体内 `read_entry(` 调用点满足；快照产出段保留的手写标准锁行不再需要豁免（扫描只拦「体内无入口调用却有手写锁行」）。`bootstrap_sync_from_channel` 与 `get_sync_status` 的登记不受影响。
7. **两批迁移**：①`read_entry.rs` 落地 + 单测（纯增量零迁移）；②51 处机械替换 + 守门上线，一票完成——无双轨过渡（对比 ADR-0073 的三批：彼时分批动因是声明表双轨过渡，本票不存在）。
8. **术语归属**：「壳层入口 / 读入口 / read_entry」等机制术语以本 ADR 为唯一解释处，不进 docs/contexts/（ADR-0047 决策 6 先例：跨域基础设施机制不属任何自然域）。

## 否决（防重复提案）

- **泛型 / trait / 薄 adapter 统一两壳状态类型**：issue 初稿的三选一建立在不成立的前提上（`AppState` 不存在、HTTP 不经 `State<ApiState>` 摸连接）；两壳类型已同一，任何统一机制都是负收益。
- **`db::read` 镜像 `db::write`（锁行住 db/）**：`db::read` 今日仅一个消费者（read_entry），为对称多一层；若未来出现第二个同步读消费者，把一行搬进 db/ 即可升格，非难逆转决策。
- **read_entry 并入 write_entry.rs（「壳层入口」同模块）**：代码层零共享（共同依赖 run_db 已在 db/），模块名 `write_entry` 装 `read_entry` 名实不符；改名牵动 import 面、白名单与 ADR-0073 档案文本。
- **更宽守门模式（任何 `.lock()` 即红）**：把超时锁、mut 守卫、boot 内部编排全卷进豁免清单，噪音化到无信息量。
- **域层纳入**：域函数经接缝收 `&Connection`，仪式已被接缝设计吃掉（ADR-0073 决策 6 同款边界）；测试世界按 ADR-0056 测试豁免约定不受影响。
- **顺带整理 B/C 组**（如锁内文件探测外移）：超出本票范围的语义判断；已知的「run_db 被非 DB 阻塞工作使用与『DB 闭包』注释的张力」属既有事实，不在本票处理。

## 代价

1. 迁移后壳层仍余 9 处手写标准锁行（9 个命令），由守门豁免清单 + 动机注释看守——「读命令必经入口」不是全称命题，是「除显式豁免外」的命题。（issue #1283 起 8 处：预检的 manifest GET 出锁，见决策 6 修订注记；issue #1284 起 7 处：发布命令的配置读取走读入口，仅余快照产出短锁，同见决策 6 修订注记。）
2. 写侧白名单与读侧豁免两张清单并存，认知成本 +1；合并为一会混淆两种核对知识，维持分开。
3. 守门白名单追加一行（read_entry.rs），基础设施清单 +1。
4. 「run_db 名义是 DB 闭包、实被文件 IO/钥匙串复用」的注释与用法张力保留（既有事实，本票不动）。

## 后果

- 新增读命令只剩：域函数 + 壳层一行 `read_entry` + TS 调用面；手抄锁仪式的失败类由扫描守门在测试期拦住。
- 锁失败映射从 60 份副本收敛为 read_entry 1 处 + 9 处显式豁免（issue #1283 起 8 处，issue #1284 起 7 处）；双壳读路径形状一致，与写路径对称，ADR-0056「壳层无业务语义」的肉眼可验程度再进一步。
- `run_db` 保持阻塞线程池与 span 归因的唯一拥有者，组合拓扑与写侧同构：`write_entry = run_db ∘ db::write ∘ emit`，`read_entry = run_db ∘ lock`。
