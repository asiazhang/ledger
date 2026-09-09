-- V021：多端同步合并语义（issue #856 / ADR-0091 决策 4/5/6）——LWW 裁决检索、
-- ParkedOp 挂起队列。新增对象，不改任何既有列语义（已发布迁移不动）。
--
-- sync_ops.entity_id：命令指向的实体 id（DomainCommand::subject，与 entity 列
-- 同源于载荷）。LWW 裁决（同实体并发编辑取全序末者）按 (entity, entity_id) 检索
-- 本地日志中全序更后的 op；无实体指向的命令（如期次触发，其冲突域是
-- OccurrenceKey）存空串。重放与本地产出经同一写入口（sync_engine::ops）填充。
--
-- sync_parked_ops：不可重放 op 的统一归宿（ParkedOp，ADR-0091 决策 6）——外键
-- 依赖失败与 schema 版本偏斜双向。op **不落** sync_ops（未应用，重投递会重试），
-- 挂起行按 op_id 幂等覆盖（重复投递不堆积）；重投递成功即出队（同 op_id 删除）。
-- park_code 为码化原因（前端按码本地化），park_message 为中文详情。

ALTER TABLE sync_ops ADD COLUMN entity_id TEXT NOT NULL DEFAULT '';

CREATE INDEX idx_sync_ops_entity_id ON sync_ops (entity, entity_id);

CREATE TABLE sync_parked_ops (
    op_id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    clock INTEGER NOT NULL,
    schema_version INTEGER NOT NULL,
    entity TEXT NOT NULL,
    entity_id TEXT NOT NULL DEFAULT '',
    payload TEXT NOT NULL,
    park_code TEXT NOT NULL,
    park_message TEXT NOT NULL,
    parked_at TEXT NOT NULL
);
