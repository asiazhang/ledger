-- V022：多端同步检查点位点表（issue #857 / ADR-0091 决策 9）。新增表，不改任何
-- 既有对象（已发布迁移不动）。
--
-- sync_stream_positions：本机对各来源设备 op 流的「已应用位点」——该流上已裁决
-- （应用/去重/压制/跳过）op 的连续前缀水位，applied_through 之前的 op 已全部并
-- 入本端状态（或作为 LWW 输者落日志可追溯）。位点用于三处：
--   1. Checkpoint 生成（各设备 op 流已应用位点，与全量快照构成检查点对）；
--   2. 增量拉取口径（只取位点之后的 op，位点之前的重投按位点门跳过）；
--   3. OpLog 截断安全界（只有位点之前的日志才可删；挂起 op 不进日志、位点不
--      越过它，天然不被截断）。
-- 位点在截断后依然留存（水位不因日志缩短而回退），是本机「写视角」与通道
-- 「大家写视角」之间的桥。device_id 为来源设备（外来流本机无行，不设外键，
-- 与 sync_ops.device_id 同款纪律）。

CREATE TABLE sync_stream_positions (
    device_id TEXT PRIMARY KEY,
    applied_through INTEGER NOT NULL,
    updated_at TEXT NOT NULL
);
