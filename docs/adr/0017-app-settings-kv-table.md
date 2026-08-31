# ADR 0017: 后端应用配置统一存 `app_settings` KV 表

后端权威的应用配置与引擎运行时状态需要持久化到 `ledger.db`（见 ADR-0016 的论证），但**不**为每个功能建专表：新增一张通用 KV 表 `app_settings(key TEXT PRIMARY KEY, value TEXT)` 作为唯一落点，key 以 `<feature>.<name>` 点分命名、在 Rust 侧用枚举集中定义（`settings` 模块收口 get/set），值用 serde_json 序列化、类型由读取方声明。后续任何新配置项的成本是「加一个枚举变体 + 一行默认值」，零迁移。

## 背景

AutoBackup（issue #123 / ADR-0016）最初规划为单行表 `auto_backup_state`。审视后发现这开创了不可扩展的先例——每个带配置的功能都建一张表，配置持久化没有归口。真正要回答的问题是「谁消费、谁权威」，而不是「每项配置存哪」：

| 层次 | 例子 | 去处 |
|---|---|---|
| 设备偏好（前端独享消费） | theme、backupDir、backupMaxCount | localStorage（现状不变） |
| 后端权威的配置/运行时状态 | `auto_backup.enabled`、dirty 标记 | **`app_settings` KV 表** |
| 需要关系结构的实体（多行、可查询、外键） | transactions、accounts | 独立表 |

## 决策

1. **存储层**：V008 迁移建通用 `app_settings` KV 表；dirty / last_backup_at 等高频状态也是普通 key，单行主键 UPDATE 开销可忽略。
2. **代码层**：`src-tauri/src/settings.rs` 收口读写接口，key 枚举集中定义，杜绝字符串字面量散落在 commands。
3. **API 层保持领域形状，KV 不透传给前端**：命令仍是语义化的领域命令（如 `get_auto_backup_state` / `set_auto_backup_enabled`，聚合多个 key 返回类型化 DTO），不做通用的 `get_setting(key)`/`set_setting(key,value)` IPC——写路径是行为不是赋值（校验、副作用需要有唯一落点），且通用写会让前端触达纯后端内部状态（如 dirty），腐蚀权威边界。将来若出现批量只读的真实场景，可加带 key 白名单的 `get_app_settings(keys)`，写永远走领域命令。

## 考虑过的替代方案

- **每功能一张专表**：表数量随功能线性增长，无归口，否决。
- **独立 JSON/TOML 配置文件**（Rust 管理）：需自行处理原子写与损坏回退，且游离在 Backup/Restore 之外，「恢复备份 → 调度状态重置」语义断裂；KV 表天然继承 SQLite 的事务性、迁移体系与整库快照——旧快照缺此表即取默认值，行为免费正确。
- **全放 localStorage**：后台线程读不到、AI HTTP API 写路径绕过前端、Backup/Restore 不携带，ADR-0016 已否决。

## 后果

- 约定入 AGENTS.md 口径：前端独享消费 → localStorage；后端消费或参与 Backup/Restore 语义 → `app_settings`；有关系结构需求 → 才配独立表。单行状态表此后不再出现。
- value 无 schema 约束，键名拼写错误只能在读取时暴露——由枚举集中定义缓解（枚举外的 key 视为 bug）。

## 修订记录

- **2026-08-31（issue #307 / ADR-0042）：设备级开关例外。** 定时计划「自动执行」开关虽由后端调度线程消费，但刻意**不进** `app_settings`：该表随 Backup/Restore 迁移，存进去就成了全账本单值，表达不了「这台执行、那台不执行」，也会把自动化意外迁移到新设备。它按 ADR-0042 落为设备偏好（前端 localStorage 单一来源 + 后端运行时镜像推送，备份目录镜像先例），是「后端消费 → `app_settings`」约定的一条显式例外：判据从「谁消费」让位给「是否应随账本迁移」。
