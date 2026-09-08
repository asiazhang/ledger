-- V020：多端同步元数据（issue #855 / ADR-0091）——设备标识、端内单调逻辑时钟
-- 与操作日志（OpLog）。新增表，不改任何既有对象（已发布迁移不动）。
--
-- sync_device：每端单行（首条写路径按需生成 DeviceId 并持久化；重装/换机 =
-- 新库 = 新标识，属合法路径）。logical_clock 为端内单调逻辑时钟，本地 op 落
-- 日志时 +1，随写事务提交/回滚，是跨端全序 (clock, device_id) 的排序依据。
--
-- sync_ops：op 只追加、不改写（与 migration 同一纪律）。op_id 为幂等去重键
-- （同一 op 重复投递不产生第二次效果）；device_id 是**来源设备**——外来 op 的
-- 来源设备在本机 sync_device 无行，故本列刻意不设外键；payload 为语义命令
-- （DomainCommand）JSON 序列化，含源端折算结果，重放不依赖本地汇率表。
-- entity 列为载荷的实体判别（与 payload 内 tag 同源），供 SQL 层按实体检索；
-- 不设 CHECK 闭集：DomainCommand 面只增不改，新增实体走新增值（新 migration
-- 不为开放枚举重构表）。
-- (device_id, clock) 唯一：端内时钟分配严格递增，索引同时充当全序检索路径。

CREATE TABLE sync_device (
    id TEXT PRIMARY KEY,
    logical_clock INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE sync_ops (
    op_id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    clock INTEGER NOT NULL,
    schema_version INTEGER NOT NULL,
    entity TEXT NOT NULL,
    payload TEXT NOT NULL,
    recorded_at TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_sync_ops_device_clock ON sync_ops (device_id, clock);
