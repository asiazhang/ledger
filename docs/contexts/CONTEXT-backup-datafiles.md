# 领域词汇表：备份与数据文件

> Ledger 领域词汇表的备份与数据文件分域。全部分域与彼此关系见 `CONTEXT-MAP.md`；决策记录见 `docs/adr/`（ADR-0007 / ADR-0016 / ADR-0018 等）。
> 跨域共享术语（DefaultCurrency、AppSettings、ViewState 等）见核心交易域 `CONTEXT-core.md`、参考数据与设置域 `CONTEXT-reference-settings.md`、界面状态与交互域 `CONTEXT-ui-interaction.md`，本文不复制定义。
> 若与代码行为冲突，以代码为准并同步修正本文件。

## Backup（备份）

- **定义**：账本数据库的完整文件级快照，产物为包含数据库文件与元数据（备份时间、应用版本、schema 版本、来源标记 `kind`）的 zip 包。触发方式分两种：用户手动触发（ManualTrigger 语境，见 BackupTrigger / ManagedBackup），或系统自动定时触发（AutoBackup）。
- **边界**：
  - 是文件级快照，不是语义级导出：不按记录或表选择内容，恢复即整库还原。
  - 与 AI 导入域的 Import（AI 驱动的语义级写入）和投资域行情同步（InstrumentSync 全量同步 / HoldingPriceSync 增量同步）是三条互不交叉的数据通道：恢复 ≠ 导入，备份 ≠ 同步。
  - 不含界面状态与偏好（界面状态与交互域 WindowState、ViewState、参考数据与设置域 Appearance、核心交易域 DefaultCurrency 等），那些属设备本地偏好，不随备份迁移。
  - 明文存放，由用户自行妥善保管。
  - 产物内容与格式由手动与自动共用一套机制（VACUUM INTO 快照 + zip 打包，见 ADR-0007 / ADR-0016）；两类备份仅触发来源与命名前缀不同，并以元数据 `kind: "auto"|"manual"` 显式区分来源（issue #127）；旧版本备份缺该字段时按 "manual" 处理，列表与恢复不报错。
- **别名**：不使用"导出"（语义级、可选择性）、"快照"（偏技术）等词。

## Restore（恢复）

- **定义**：用一份 Backup 快照替换当前数据库的破坏性操作，执行前自动为当前数据创建 RestoreSafetyBackup。
- **边界**：
  - 替换式还原，不是合并式导入（合并属于 AI 导入域 Import 通道的职责）。
  - 备份来自更高版本应用时拒绝恢复；来自旧版本时允许，恢复后自动迁移升级。
  - 恢复成功后应用自动重启，以全新状态加载数据。
- **别名**：不使用"导入"（语义级写入，与恢复是两种操作）。

## RestoreSafetyBackup（恢复安全备份）

- **定义**：Restore 执行前，系统自动为当前数据库创建的备份，用于恢复出错时回滚。
- **边界**：由系统自动创建、自动命名，用户无需干预；与用户手动创建的 Backup 存放位置不同。

## BackupDirectory（备份目录）

- **定义**：用户配置的默认备份存放位置；配置后"一键备份"直接写入该目录，无需每次选择。
- **边界**：属设备本地偏好（与参考数据与设置域 Appearance、核心交易域 DefaultCurrency 同类），不进入 `ledger.db`，也不随 Backup/Restore 迁移。
- **别名**：不使用"导出目录"（备份 ≠ 导出）。

## BackupRetentionLimit（备份保留上限）

- **定义**：用户可配置的受管备份最大保留数量，默认 30，可调范围 1–100。
- **边界**：
  - 属设备本地偏好（与 BackupDirectory 同类），不进入 `ledger.db`，也不随 Backup/Restore 迁移。
  - 只约束 ManagedBackup（受管备份）；ManualBackup（另存备份）不受约束。
  - 手动与自动触发的 ManagedBackup 同等对待、共享同一上限：最旧淘汰，不区分来源（ADR-0016）。
  - 上限调小时立即滚动清理到新值；受管备份写入后自动滚动清理。
- **别名**：不使用"最大保存文件个数"（口语化）、"保留策略"（偏宽泛）。

## BackupPruning（备份滚动清理）

- **定义**：把 ManagedBackup 数量修剪到 BackupRetentionLimit 之内的过程：删除最旧的超出部分。
- **边界**：
  - 触发点：任何一次成功写入且落点为受管备份之后；上限调小时立即执行。
  - 排序以备份文件名时间戳为准，解析失败回退文件修改时间。
  - 删除失败的文件跳过并报告，不中断其余清理。
  - 与 RestoreSafetyBackup（恢复安全备份）无关：后者在应用数据目录、命名不同，不受清理影响。

## ManagedBackup（受管备份）

- **定义**：位于配置的 BackupDirectory 内、按受管命名规则（`ledger-backup-YYYYMMDD-HHMMSS.db.zip` 手动 / `ledger-auto-YYYYMMDD-HHMMSS.db.zip` 自动，见 BackupTrigger）生成的备份文件；受 BackupRetentionLimit 约束。
- **边界**：
  - 来源分两类：手动（一键备份 / 使用默认文件名存入备份目录的"另存为"）与自动（AutoBackup）。
  - 两类受管备份在配额与滚动清理上同等对待（共享 BackupRetentionLimit，最旧淘汰，不区分来源，ADR-0016）。
  - 改名后不属于受管备份；不受管（另存到其它位置或改名）的文件永不被自动清理。
  - 受管判定按文件名前缀（`ledger-backup-` / `ledger-auto-`）识别：后端 `MANAGED_BACKUP_PREFIXES` 与前端 `isManagedBackupPath` 各持一份常量并保持一致（ADR-0016 已接受该取舍）。
- **别名**：不使用"自动备份"（那是 BackupTrigger 的自动来源，不是"受管"的同义）。

## ManualBackup（另存备份）

- **定义**：用户通过"另存为…"主动选择存放位置或文件名的备份文件。
- **边界**：若写入配置的 BackupDirectory 且文件名匹配自动命名规则，则视为 ManagedBackup；否则不受 BackupRetentionLimit 约束，永不被自动删除。

## BackupTrigger（备份触发来源）

- **定义**：区分一次 Backup 由谁发起的概念维度——**手动**（用户经设置页"一键备份/另存为"主动触发）或**自动**（AutoBackup 引擎按调度规则触发）。
- **边界**：
  - 手动/自动只影响命名前缀（`ledger-backup-` / `ledger-auto-`）与 zip 内 `backup.json` 的 `kind` 字段（`manual` / `auto`）；产物格式、恢复流程、受管属性完全一致（ADR-0016）。
  - 恢复安全备份（RestoreSafetyBackup）是系统自动创建但**不属于** AutoBackup（不经调度器、不标脏、不占配额、独立存放位置）。
- **别名**：不使用"来源"（过于宽泛）、"自动/手动备份"口语（正式术语为"备份触发来源"）。

## AutoBackup（自动备份）

- **定义**：由应用内置调度器按周期自动触发的 Backup，目标是让"数据有变化时不至于长期无备份"——用户忘了手动备份也不丢数据。触发条件为"距上次备份超过间隔（24 小时）且自上次备份以来数据有变化（DirtyMarker）"。
- **边界**：
  - 间隔固定为 24 小时（ADR-0016），锚点是"距上次备份"而非固定时刻；检查周期短（30 分钟轮询 + 写时顺带检查）但备份频率上限是每天一次。
  - 只在应用运行期间生效；系统休眠时调度暂停，唤醒后由短周期轮询在 30 分钟内补上。应用退出时若脏则兜底备份一次（不受每日约束）。
  - 首次启动若备份列表为空（不分手动/自动）则立即备份一次，保证装上当天就有一份。
  - BackupDirectory 未配置时自动备份静默不执行，设置页提示引导配置。
  - 自动备份的开关与调度状态（DirtyMarker、下次到期时间、上次备份时间）存于 `ledger.db` 的 AppSettings 表（参考数据与设置域，后端权威，ADR-0016/0017），随 Backup/Restore 迁移；`backupDir`/`backupMaxCount` 仍属设备本地偏好。
  - 失败不重试（保留 DirtyMarker，下个周期重试），成功不通知用户；产物同为 ManagedBackup，参与滚动清理。
  - 备份产物变更（自动备份完成 / 受管备份清理）成功后发出无 payload 的 `ledger:backups-changed` 信号（issue #129，与参考数据与设置域参考失效信号 `ledger:changed` 平行）；前端设置页订阅该信号自动刷新备份列表与自动备份状态。
- **别名**：不使用"定时备份"（偏计划任务语义）、"自动定时备份"（啰嗦，正式术语"自动备份"）。

## DirtyMarker（脏标记）

- **定义**：记录"自上次备份以来数据是否有变化"的布尔状态，是 AutoBackup 决定"到点是否真正执行备份"的唯一依据。
- **边界**：
  - 置真：任何一次业务写库成功后（核心交易域交易写入经 Writer 接缝、参考数据与设置域参考数据 CRUD、投资域市场数据写入），显式调用；备份成功后清真。
  - 恢复（Restore）成功后重置：恢复本身生成了 RestoreSafetyBackup，数据刚被校验，不产生"恢复后立即备份"。
  - 调度器自身的状态写入（开关/到期时间）不置真，避免自触发。
  - 失败保留：备份失败不清真，下个周期重试（ADR-0016）。
  - 属调度状态，存于 AppSettings 表（`auto_backup.dirty` 键）。
- **别名**：不使用"脏位"（偏底层实现）、"变更标记"（含糊）。
