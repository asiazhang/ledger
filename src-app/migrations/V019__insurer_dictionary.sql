-- V019: 保司字典（Insurer）表 + 常用国内保司种子（issue #712 / ADR-0082 决策 1/4）
-- 保险公司升格为保险域自有独立字典：保司是公共机构名（客观事实数据），不复用
-- 核心交易域商户（商户是个人消费轨迹，两画不同构）；保单以 insurer_id 引用
-- （换轨由后续票实施，本迁移只建字典本体与种子）。
-- 字典语义照抄商户先例（V001 merchants）：参考数据模式——软删除（is_deleted）+
-- 审计字段（created_at/updated_at/version/device_id）；name 在用行全库唯一（仅在用行
-- 唯一：软删后同名可重建，软删行不占名字），以 partial unique index 落地；
-- 名字字典：无 icon/color 等视觉字段（issue #223 先例）。
--
-- 种子：幂等预置 30 家常用国内保司（人身险头部 16 + 财产险头部 14，财产险覆盖
-- 车险场景）；命名用常用简称（「平安人寿」非「中国平安人寿保险股份有限公司」），
-- 用户怎么叫、字典怎么存。机制照 V004 种子先例：确定性 UUID v5 +
-- 按名 INSERT OR IGNORE（种子重复执行幂等：同名不重复建）；种子行为普通字典行
-- （可软删、无特殊标记）；后续版本可在新迁移继续幂等补种。
-- 与商户「强个人属性、不 seed」（V001）差异化：保司是公共机构名，开箱即用价值成立。
-- 确定性 UUID 生成规则（可复现）：
--   UUID v5(命名空间 32e918c6-84ad-58a6-b37d-66e77122ccef, "insurer:<名称>")
-- created_at / updated_at 与 now_iso() 保持同格式：strftime('%Y-%m-%dT%H:%M:%SZ','now')。

CREATE TABLE IF NOT EXISTS insurers (
    id         TEXT PRIMARY KEY,                  -- 保司全局唯一 ID（种子为确定性 UUID v5，即席创建为 UUID v7）
    name       TEXT NOT NULL,                     -- 保司名称（常用简称），如「平安人寿」「人保财险」；在用行全库唯一
    created_at TEXT NOT NULL,                     -- 创建时间，UTC ISO 8601 格式
    updated_at TEXT NOT NULL,                     -- 最后修改时间，UTC ISO 8601 格式
    version    INTEGER NOT NULL DEFAULT 1,        -- 版本计数
    device_id  TEXT NOT NULL,                     -- 创建设备/最后修改设备标识
    is_deleted INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0, 1))  -- 软删除标志
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_insurers_name_active ON insurers(name) WHERE is_deleted = 0;

-- 人身险头部：16
INSERT OR IGNORE INTO insurers (id, name, created_at, updated_at, version, device_id) VALUES
  ('dc5b304a-d85d-5d5f-a749-c92894e92a41', '中国人寿', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('893dfb90-16a7-5c9d-9759-e1f32ed7db84', '平安人寿', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('8f7ac64f-ae56-52b6-823c-810fa5f28f2c', '太保寿险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('8c374c1e-2a97-5f72-87bc-c455f35c2e17', '泰康人寿', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('09fb985c-3dd1-5984-a3e8-da258880626d', '新华保险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('25b4c898-59df-5c4d-b442-9b71a5fbbbe4', '人保寿险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('91cc8562-10d7-5df1-9627-6bb52b9db106', '太平人寿', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('fa53142f-69b7-5873-9a94-b427a4dc2da4', '友邦人寿', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('3ba2b4bc-e452-531d-b7af-0dc97e0c9d1f', '中意人寿', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('3ee236da-9ebb-5ce0-8224-c2fc42dffd5e', '阳光人寿', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('07f0cf23-fb3b-597e-b7d9-9cd20c5aab3c', '富德生命人寿', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('87371ef1-7b2c-5ce8-ac6c-7775332e4285', '中宏人寿', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('c5b2b036-1159-5edc-83f5-3b5f97df5ef4', '工银安盛人寿', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('a75cd3ba-71bb-5f9f-9223-8c9ee9449ce3', '招商信诺人寿', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('2f55f203-2c35-5375-8594-bff8c2037e27', '人保健康', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('ad688e79-cba7-5343-9c3d-208de2478c77', '中汇人寿', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed');

-- 财产险头部（覆盖车险场景）：14
INSERT OR IGNORE INTO insurers (id, name, created_at, updated_at, version, device_id) VALUES
  ('80042dfa-c80d-531f-9c0c-eb0a60f97833', '人保财险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('da4d7eef-e1ba-5efc-99a3-064e59deffc5', '平安财险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('c6373393-82cf-51fc-b23c-3ed9cfd041bf', '太保财险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('fe9735e9-8b78-5663-8b97-362d11bf40f6', '国寿财险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('87c78202-984b-5eca-a47a-5e727b4525d9', '大地保险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('cdf337d8-01e8-572a-8464-4b22dc4d67a5', '阳光财险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('b2636ebd-55d1-51c7-8b3b-1fce8666223a', '众安保险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('4b7e78b4-d0ed-5c95-bd07-989deb588f77', '中华保险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('e891b564-ecb9-5bab-927b-b3d417d45cb9', '太平财险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('deb099a4-31b6-5ef9-a045-e51f76e9cf42', '华泰保险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('d0ec7b82-a413-5c82-8fe9-3bddc616d770', '永安保险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('296a7644-4949-5ea1-99a5-61b7475f7f48', '紫金保险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('bd9ca1a7-e9dc-506c-a409-445c2f489db9', '英大泰和', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('426a39e8-0dac-5f9a-ac5b-b02eaf1b0433', '华安保险', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed');
