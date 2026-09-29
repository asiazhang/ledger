-- V036：同步元数据拆库——四表迁入独立库 sync.db（issue #1871 / ADR-0139 决策 1/4）。
--
-- 同步元数据四表闭集（sync_device / sync_ops / sync_parked_ops /
-- sync_stream_positions）整体迁出主库，落与主库同目录的独立库文件 sync.db，经
-- ATTACH 为 `sync` 与主库同事务读写（挂载接线见建连收尾单点，issue #1869）；
-- 主库此后只存账本数据。清单边界（ADR-0139 决策 1）：app_settings 的同步相关
-- 键与业务主表自带的同步审计列（device_id / version / is_deleted）留守主库，
-- 不属搬迁对象。
--
-- 形态：本迁移以最终形态（V020/V021/V022 合成形态，含 entity_id 与两条索引）
-- 在 attached 侧建四表，逐表 INSERT…SELECT 搬数据，再 DROP 主库同名表。表结构与
-- V020–V022 完全同形同名同序——schema 漂移守卫（双库校验）以迁移链为参照，
-- 搬迁不产生形态分叉。
--
-- 原子性：rusqlite_migration 把待跑迁移批次包在单一事务里执行（非 WAL 下经
-- master journal 跨 ATTACH 库集合级原子，ADR-0139 决策 2——主库不设 WAL 是本
-- 搬迁的前提，与 ADR-0117 决策 6 互锁），中断即整体回滚、重启重跑收敛。
--
-- IF NOT EXISTS（而非裸 CREATE）：attached 侧已存在同名表时按「续完」处理——
-- 直接复用既有空表搬数据（PK 冲突即报错回滚，不静默覆盖）。覆盖的真实场景是
-- 票 07 的跨版本引导：旧版端单文件快照整库换入 main（四表随行、user_version
-- 回退）后，重启重放本迁移时 attached 侧已由挂载接线补建空表，复用即收敛。
--
-- 搬数据源用 main. 显式限定：搬迁窗口内主库与 attached 侧同名并存，
-- unqualified 名解析优先 main（挂载测试钉住），显式限定不依赖解析序。
-- DROP 同理显式 main. 限定——落点错误会把搬迁静默变成灾难。

CREATE TABLE IF NOT EXISTS sync.sync_device (
    id TEXT PRIMARY KEY,
    logical_clock INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sync.sync_ops (
    op_id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    clock INTEGER NOT NULL,
    schema_version INTEGER NOT NULL,
    entity TEXT NOT NULL,
    entity_id TEXT NOT NULL DEFAULT '',
    payload TEXT NOT NULL,
    recorded_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS sync.idx_sync_ops_device_clock
    ON sync_ops (device_id, clock);

CREATE INDEX IF NOT EXISTS sync.idx_sync_ops_entity_id
    ON sync_ops (entity, entity_id);
CREATE TABLE IF NOT EXISTS sync.sync_parked_ops (
    op_id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    clock INTEGER NOT NULL,
    schema_version INTEGER NOT NULL,
    entity TEXT NOT NULL,
    entity_id TEXT NOT NULL DEFAULT '',
    payload TEXT NOT NULL,
    park_code TEXT NOT NULL,
    park_params TEXT NOT NULL DEFAULT '[]',
    park_message TEXT NOT NULL,
    parked_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sync.sync_stream_positions (
    device_id TEXT PRIMARY KEY,
    applied_through INTEGER NOT NULL,
    updated_at TEXT NOT NULL
);

INSERT INTO sync.sync_device (id, logical_clock, created_at, updated_at)
    SELECT id, logical_clock, created_at, updated_at FROM main.sync_device;

INSERT INTO sync.sync_ops
    (op_id, device_id, clock, schema_version, entity, entity_id, payload, recorded_at)
    SELECT op_id, device_id, clock, schema_version, entity, entity_id, payload, recorded_at
    FROM main.sync_ops;

INSERT INTO sync.sync_parked_ops
    (op_id, device_id, clock, schema_version, entity, entity_id, payload,
     park_code, park_params, park_message, parked_at)
    SELECT op_id, device_id, clock, schema_version, entity, entity_id, payload,
           park_code, park_params, park_message, parked_at
    FROM main.sync_parked_ops;

INSERT INTO sync.sync_stream_positions (device_id, applied_through, updated_at)
    SELECT device_id, applied_through, updated_at FROM main.sync_stream_positions;

DROP TABLE main.sync_stream_positions;
DROP TABLE main.sync_parked_ops;
DROP TABLE main.sync_ops;
DROP TABLE main.sync_device;
