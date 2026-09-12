//! 写操作身份闭集与单一来源宏（ADR-0044 决策 2 / ADR-0102）：`WriteOp` 的变体
//! 清单即 enum 本体，enum / `ALL` / `from_ident` 三份表示同体展开。
//!
/// 写操作闭集单一来源宏（ADR-0102）：清单即 enum 本体，同体从同一 token 流展开
/// [`WriteOp`] enum、[`WriteOp::ALL`]（切片）与 [`WriteOp::from_ident`]——「enum 新增
/// 变体漏登 `ALL` / 漏 parse」的漂移**不可表达**（构造性保证，非「可检测」）。
///
/// 变体 `///` 文档经 `$meta` 原位透传（rustdoc 不变）；域分组注释保留在调用内作
/// 纯注释。宏定义与调用就地本模块（局部性即卖点，ADR-0102 决策 1 / 决策 3）。
macro_rules! write_op_set {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident
            ),* $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )*
        }

        impl $name {
            /// 全部写操作身份（闭集清单）：信号守门测试（`signals_cross_check`，
            /// ADR-0044 决策 3 / ADR-0073 决策 5）按此遍历做「映射未声明」反向核对——
            /// 除特例条目 [`WriteOp::AutoBackupDeepPath`]（登记生产者清单、刻意不做
            /// 命令键）外，每个身份须被至少一壳声明（`write_entry` 调用点或例外白名单），
            /// 否则测试期即红。
            ///
            /// 本清单由 `write_op_set!` 宏（本文件私有，ADR-0102 决策 1）从宏调用清单同体展开：
            /// 与 enum 本体共享同一 token 流，不存在第二份事实，漏登失败类不可表达。
            pub const ALL: &[$name] = &[
                $($name::$variant,)*
            ];

            /// 变体标识符 → 身份（ADR-0102 决策 2）：供 `signals_cross_check` 把源码
            /// 扫描提取的 `WriteOp::<Variant>` 文本映射回变体，取代手写
            /// `parse_write_op` 穷尽臂。臂集与本清单同源展开，完备性由构造保证；
            /// 非变体文本返回 [`None`]，由调用方以断言失败报「扫描提取漂移」。
            /// 消费方仅守门测试（ADR-0073 决策 5）；`signals_cross_check` 属根包的
            /// `#[cfg(test)]` 模块，跨 crate 消费时不随本 crate 的 cfg(test) 编译
            /// （issue #1088 归位后实测），故本函数恒生成、以 `#[doc(hidden)]` 退场。
            #[doc(hidden)]
            pub fn from_ident(ident: &str) -> Option<$name> {
                match ident {
                    $(stringify!($variant) => Some($name::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

write_op_set! {
/// 写操作身份（ADR-0044 决策 2）：跨 IPC 壳与 HTTP 壳共享的强类型键，闭集。
///
/// 变体按域分组；每个变体注释标明对应的 IPC 命令与/或 HTTP 端点，以及
/// 预期携带的结果证据（条件信号操作）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WriteOp {
    // ── 参考数据四表（ADR-0012）：写入成功发 `ledger:changed` ──
    /// 创建账户（IPC `create_account`；HTTP `POST /api/v1/accounts`）。
    CreateAccount,
    /// 编辑账户（IPC `update_account`；HTTP `PUT /api/v1/accounts/{id}`）。
    UpdateAccount,
    /// 删除账户（软删除；IPC `delete_account`；HTTP `DELETE /api/v1/accounts/{id}`）。
    DeleteAccount,
    /// 创建分类（IPC `create_category`；HTTP `POST /api/v1/categories`）。
    CreateCategory,
    /// 编辑分类（IPC `update_category`）。
    UpdateCategory,
    /// 分类重排（IPC `reorder_categories`）。
    ReorderCategories,
    /// 删除分类（软删除；IPC `delete_category`；HTTP `DELETE /api/v1/categories/{id}`）。
    DeleteCategory,
    /// 创建商户（第四张参考表，ADR-0028；IPC `create_merchant`）。
    CreateMerchant,
    /// 商户改名（IPC `update_merchant`）。
    UpdateMerchant,
    /// 删除商户（软删除；IPC `delete_merchant`）。
    DeleteMerchant,
    /// 创建保司（保险域自有字典，ADR-0082；IPC `create_insurer`）。
    CreateInsurer,
    /// 保司改名（IPC `update_insurer`）。
    UpdateInsurer,
    /// 删除保司（软删除；IPC `delete_insurer`）。
    DeleteInsurer,

    // ── 物品域（ADR-0014：独立领域，复用 `ledger:changed` 同名事件）──
    /// 创建物品（IPC `create_item`）。
    CreateItem,
    /// 编辑物品（IPC `update_item`）。
    UpdateItem,
    /// 处置物品（IPC `dispose_item`）。
    DisposeItem,
    /// 删除物品（IPC `delete_item`）。
    DeleteItem,

    // ── 保单域（ADR-0051：独立领域，复用 `ledger:changed` 同名事件）──
    /// 创建保单（IPC `create_policy`）。
    CreatePolicy,
    /// 编辑保单（IPC `update_policy`）。
    UpdatePolicy,
    /// 删除保单（软删除；IPC `delete_policy`）。
    DeletePolicy,

    // ── 实物资产域（ADR-0064：独立领域，复用 `ledger:changed` 同名事件）──
    /// 建档实物资产（IPC `create_physical_asset`；资产行 + 首条估值行同事务）。
    CreatePhysicalAsset,
    /// 编辑实物资产档案（IPC `update_physical_asset`，issue #467 T2）：
    /// 仅名称 / 购买信息，估值不经本入口变更。
    UpdatePhysicalAsset,
    /// 更新实物资产估值（IPC `update_physical_asset_valuation`，issue #467 T2）：
    /// 追加一条估值历史行（只追加不改写），当前估值变为最新一条。
    UpdatePhysicalAssetValuation,
    /// 处置实物资产（IPC `dispose_physical_asset`，issue #468 T3）：
    /// 状态标记转已处置 + 处置信息落库，退出默认列表与在持合计。
    DisposePhysicalAsset,
    /// 软删除实物资产（IPC `delete_physical_asset`，issue #468 T3）：
    /// `is_deleted=1`，数据与估值历史保留，退出列表与合计。
    DeletePhysicalAsset,

    // ── 账户域 ──
    /// 余额调整（IPC `adjust_account_balance`，ADR-0026）：预期证据
    /// [`crate::signals::WriteEvidence::BlackHoleCreated`]——仅按需新建黑洞账户时参考表变更。
    AdjustAccountBalance,

    // ── 价格域：条件信号 `ledger:prices-changed`（ADR-0031），证据
    //    [`WriteEvidence::PriceWritten`]，映射内共享一份「实际写入」判定 ──
    /// 标的信息同步（IPC `sync_instrument_info`，issue #827 前名
    /// `sync_holding_prices`）：证据 = 价格或名称实际写入（`any_written`）。
    SyncInstrumentInfo,
    /// 按代码即拉场外基金（IPC `add_fund_by_code`，ADR-0038）：证据 = 落现价缓存。
    AddFundByCode,
    /// 按代码添加投资标的·场内通道（IPC `add_instrument_by_code`，issue #697 /
    /// ADR-0081）：证据 = 落现价缓存（与基金即拉同款，零变化不广播）。
    AddInstrumentByCode,
    /// 手动报价（IPC `record_manual_price`，ADR-0036）：证据 = 实际写入任一落点。
    RecordManualPrice,
    /// 标的创建 / 幂等复用（IPC `create_instrument` 手动创建；HTTP
    /// `POST /api/v1/instruments` 含基金增强分支，ADR-0037/0039）：标的字典写入本身
    /// 不发参考信号；仅基金增强分支落现价时携 [`crate::signals::WriteEvidence::PriceWritten`] 发价格信号。
    CreateInstrument,
    /// 删除标的（IPC `delete_instrument`）：刻意零信号——无流水引用的标的无
    /// 持仓 / 走势消费方，前端标的列表本地重拉（issue #292 验收项）。
    DeleteInstrument,
    /// 半成品写价通道（IPC `create_market_price`）：刻意零信号（ADR-0044 决策 6）。
    /// 写 `market_prices` 现价缓存、被 `ledger:prices-changed` 定义覆盖，但属
    /// 「手动报价落地（#291）前」的半成品通道、前端零调用点——补广播只会制造
    /// 「发了但没人听」的假一致性。淘汰 / 合并路径：`record_manual_price` 已承载该信号。
    CreateMarketPrice,
    /// 当期汇率写入（IPC `create_exchange_rate`）：刻意零信号——写 `fx_rates`
    /// 当期表，不在 `ledger:prices-changed` 定义（MarketPrice / PriceHistory /
    /// FxRateHistory，ADR-0031）覆盖范围内。
    CreateExchangeRate,
    /// 余额缓存手动审计（IPC `audit_balance_cache`，issue #491 / ADR-0067）：
    /// 刻意零信号——修复的是派生缓存行，不置脏（ADR-0032 豁免形态）、
    /// 前端按返回的差异报告就地刷新，无需失效广播。
    AuditBalanceCache,
    /// 备注拼音一键修复（IPC `repair_note_pinyin`，issue #513）：刻意零信号——
    /// 回填的是搜索派生列（V018 `note_pinyin`），不置脏（ADR-0032 豁免形态）、
    /// 前端按返回的修复报告就地展示，无需失效广播。
    RepairNotePinyin,

    // ── 备份域：`ledger:backups-changed`（issue #129）──
    /// 手动备份（IPC `create_backup`）：刻意零信号——前端备份组合在命令成功后
    /// 自行刷新列表（受管路径随后触发的滚动清理经 [`WriteOp::PruneBackups`] 发信号），
    /// 后端再广播属重复通知；收编不改现状（spec：信号语义与触发条件零变化）。
    CreateBackup,
    /// 受管备份修剪（IPC `prune_backups`）：清理成功改变备份列表。
    PruneBackups,
    /// 从备份恢复（IPC `restore_backup`）：刻意零信号——恢复成功后前端随即调
    /// `restart_app`（原位重引导 + WebView 重载，ADR-0080），全部状态重新加载，
    /// 失效信号无消费窗口。
    RestoreBackup,
    /// **特例条目，不做命令键**（ADR-0044 决策 5）：自动备份深路径执行点
    /// （连接层写入口提交点的写时顺带检查等，无命令身份）拿不到 `AppHandle`，
    /// 经 `events::EVENT_APP` 镜像句柄发射（`events::emit_backups_changed_current`）。
    /// 登记于此只为「备份信号生产者清单单点可查」，壳层不得以本变体调用
    /// [`crate::signals::signals_for`] 发射。
    AutoBackupDeepPath,

    // ── 交易域：基线零信号；唯一例外是「即建商户」证据（ADR-0028 / ADR-0044 决策 4，
    //    修复 HTTP 导入即建商户后的参考数据陈旧漏发，#331 接线）──
    /// 创建单笔交易（IPC `create_transaction`）：预期证据
    /// [`crate::signals::WriteEvidence::MerchantCreated`]（入参带 `merchant_name` 且未命中即建）。
    CreateTransaction,
    /// 批量创建交易（IPC `create_transactions`；HTTP `POST /api/v1/transactions/batch`）：
    /// 证据 = 批内聚合「任一行即建商户」。
    BatchCreateTransactions,
    /// 全字段替换交易（IPC `update_transaction`；HTTP `PUT /api/v1/transactions/{id}`）：
    /// 证据同 [`WriteOp::CreateTransaction`]。
    UpdateTransaction,
    /// 删除交易（软删除；IPC `delete_transaction`；HTTP `DELETE /api/v1/transactions/{id}`）：
    /// 零信号——交易类写入不触发参考失效。
    DeleteTransaction,
    /// 执行定时期次（IPC `execute_scheduled_occurrence`）：写入交易行（商户为计划
    /// 既有引用、不即建），零信号。
    ExecuteScheduledOccurrence,
    /// 回填定时期次（IPC `expand_scheduled_occurrences`）：写入期次计划行，零信号。
    ExpandScheduledOccurrences,

    // ── 预算域：刻意零信号（预算写不属参考 / 价格 / 备份任何一信号语义）──
    /// 创建预算（IPC `create_budget`）。
    CreateBudget,
    /// 修改预算额度（IPC `update_budget`）。
    UpdateBudget,
    /// 删除预算（IPC `delete_budget`）。
    DeleteBudget,

    // ── 定时计划域：刻意零信号（计划写入不触发参考失效；期次执行见交易域）──
    /// 创建定时计划（IPC `create_scheduled_transaction`）。
    CreateScheduledTransaction,
    /// 启停定时计划（IPC `update_scheduled_transaction_status`）。
    UpdateScheduledTransactionStatus,
    /// 编辑订阅计划续费字段（IPC `update_scheduled_subscription`）。
    UpdateScheduledSubscription,

    // ── 多端同步域（ADR-0091）：重放是行为编排之外的第 N 写入入口，外来 op
    //    实际应用即账本数据变化，条件发 `ledger:changed`（证据承载「有无应用」）──
    /// 多端同步轮次（IPC `sync_now`，issue #862）：发布自己流 + 拉取他人流并经
    /// 同步引擎幂等重放。证据 [`crate::signals::WriteEvidence::LedgerApplied`]——本轮实际应用
    /// （applied > 0）才广播参考失效；纯跳过 / 去重 / 压制 / 挂起的轮次零变化
    /// 不广播（与「零变化不广播」同一品味）。
    SyncRound,

    // ── 设置域：刻意零信号（设备偏好 / 引导配置，无 `ledger:*` 失效语义；
    //    设置页自读回显）──
    /// 自动备份开关（IPC `set_auto_backup_enabled`，写 `app_settings` KV，ADR-0017）。
    SetAutoBackupEnabled,
    /// 自动备份目录推送（IPC `set_auto_backup_dir`，前端 localStorage 权威的进程内
    /// 镜像，ADR-0016；首次兜底备份若触发，经 [`WriteOp::AutoBackupDeepPath`]
    /// 发信号，与本命令键无关）。
    SetAutoBackupDir,
    /// 定时计划自动执行开关推送（IPC `set_auto_execution_enabled`，进程级标志镜像）。
    SetAutoExecutionEnabled,
    /// 提交数据目录更改意图（IPC `submit_data_location_change`，写 ADR-0018 引导
    /// 指针文件）：重启后生效，失效信号无消费窗口。
    SubmitDataLocationChange,
    /// 恢复默认数据位置（IPC `restore_default_data_location`，同上写引导指针文件）。
    RestoreDefaultDataLocation,
    /// 设置后端日志等级（IPC `set_log_level`，写 `app_settings` 的 `logging.level`，
    /// ADR-0006）：刻意零信号——设置不是账本数据（ADR-0032 置脏豁免），也不属参考 /
    /// 价格 / 备份任何失效语义；设置页自读回显。
    SetLogLevel,
    /// 设置本位币基准（IPC `set_base_currency`，issue #858，写 `app_settings` 的
    /// `ledger.base_currency` 并同事务产出同步 op）：刻意零信号——字典与流水未变，
    /// 设置页自读回显；与 [`WriteOp::SetLogLevel`] 的区别是本写经 `write_entry`
    /// （op 与设置写同事务，非置脏豁免路径——基准是账本数据）。
    SetBaseCurrency,
    /// 多端同步通道配置（IPC `set_sync_channel_config`，issue #862，写
    /// `app_settings` 的 `sync.channel.config`，WebDAV 凭据属本机设备配置）：
    /// 刻意零信号——通道配置不同步、不属任何失效语义；设置页自读回显。
    SetSyncChannelConfig,
}
}
