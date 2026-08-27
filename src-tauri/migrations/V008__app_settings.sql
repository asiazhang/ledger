-- V008 应用配置 KV 表（ADR-0017，issue #130）
--
-- 后端权威的应用配置与运行时状态统一落此 KV 表，不再为每个功能建单行
-- 状态专表。key 规范 `<feature>.<name>`、由 src-tauri/src/settings.rs 的
-- SettingKey 枚举集中定义；value 为 serde_json 序列化结果，类型由读取方声明。
-- 读写一律经 settings 模块收口，禁止散落字符串字面量 SQL。

CREATE TABLE IF NOT EXISTS app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
