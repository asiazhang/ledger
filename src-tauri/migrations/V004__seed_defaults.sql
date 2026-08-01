-- V003 默认种子数据：币种与分类
-- currencies.code 为主键，用 INSERT OR IGNORE 去重；
-- categories 主键为 UUID，默认分类使用基于 name+kind 的确定性 UUID v5，
-- 保证所有设备初始化后默认分类的 sync_id 一致。
-- created_at / updated_at 与 now_iso() 保持同格式：strftime('%Y-%m-%dT%H:%M:%SZ','now')。
-- 应用未发布前直接扩充种子；无「退款报销」分类（退款走 transactions.kind='refund'，复用原支出交易分类）。

INSERT OR IGNORE INTO currencies (code, name, symbol, decimal_places) VALUES
  ('CNY', '人民币', '¥', 2),
  ('USD', '美元', '$', 2),
  ('EUR', '欧元', '€', 2),
  ('JPY', '日元', '¥', 0),
  ('GBP', '英镑', '£', 2),
  ('HKD', '港币', '$', 2),
  ('AUD', '澳元', 'A$', 2),
  ('CAD', '加元', 'C$', 2),
  ('KRW', '韩元', '₩', 0),
  ('SGD', '新加坡元', 'S$', 2),
  ('CHF', '瑞士法郎', 'Fr.', 2);

-- 支出顶级分类：13
INSERT OR IGNORE INTO categories (id, name, kind, icon, created_at, updated_at, version, device_id) VALUES
  ('95d6dc66-12c4-5f2b-bf9b-1d439a9c8100', '餐饮', 'expense', 'RestaurantOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('6f7a88e1-fb21-5409-b6b3-606787668c02', '交通', 'expense', 'BusOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('3feb7580-9bad-5c6a-bf4f-db9e59eb3e64', '购物', 'expense', 'CartOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('7913daff-f5fc-5ce2-98a0-85c5f0c53db9', '住房', 'expense', 'HomeOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('805a7628-6497-5252-b4ab-a76361e5aa0a', '娱乐', 'expense', 'GameControllerOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('f0683ffe-fe9c-593f-8701-4ec1c296b32c', '医疗', 'expense', 'MedkitOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('b41989ae-e78a-59f2-9c02-4f904d8e6841', '教育', 'expense', 'SchoolOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('f24916fd-6c9a-5ecd-afa5-09c1bcc5590a', '其他支出', 'expense', 'EllipsisHorizontalOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('3f673b97-a17f-5dc5-92fb-5bd4d40b7b2c', '生活缴费', 'expense', 'FlashOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('7e0c4d7e-15e9-5cbf-a3c9-059d14a86383', '人情', 'expense', 'GiftOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('cabb2911-56c1-51b8-b6c7-e4cffbcabac4', '金融保险', 'expense', 'CardOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('6f3bfe07-0782-52f4-8984-b147205dcba0', '数码产品', 'expense', 'PhonePortraitOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('6b978f24-2393-56a4-b3df-50c5d054cbc9', '汽车', 'expense', 'CarOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed');

-- 支出二级分类：61
INSERT OR IGNORE INTO categories (id, name, kind, parent_id, created_at, updated_at, version, device_id) VALUES
  ('7506e9a7-5fdb-54da-abca-a43248d373d7', '早餐', 'expense', '95d6dc66-12c4-5f2b-bf9b-1d439a9c8100', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('b4dab9dd-446c-588a-8b1b-551d93aa46d4', '午餐', 'expense', '95d6dc66-12c4-5f2b-bf9b-1d439a9c8100', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('a15e90fa-8e51-5b9b-a385-86e5d1b7af79', '晚餐', 'expense', '95d6dc66-12c4-5f2b-bf9b-1d439a9c8100', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('8719b774-6c64-53a3-aba7-2e8d3d4c4739', '零食饮料', 'expense', '95d6dc66-12c4-5f2b-bf9b-1d439a9c8100', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('c3da3cb9-109d-5eb2-9ae0-1d28d03cb8d8', '外卖', 'expense', '95d6dc66-12c4-5f2b-bf9b-1d439a9c8100', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('35b2d713-e18c-5b71-b415-72a2cce7a38e', '聚餐', 'expense', '95d6dc66-12c4-5f2b-bf9b-1d439a9c8100', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('4ca4a055-881d-51a0-8179-f10596a2dd2e', '公交地铁', 'expense', '6f7a88e1-fb21-5409-b6b3-606787668c02', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('388d9afc-10f4-55b1-a527-351029dc75ca', '出租车', 'expense', '6f7a88e1-fb21-5409-b6b3-606787668c02', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('417019f1-4738-56fc-a813-5444f174d984', '火车机票', 'expense', '6f7a88e1-fb21-5409-b6b3-606787668c02', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('1d00d4d0-3cd5-56cd-b74e-acd97c604946', '共享出行', 'expense', '6f7a88e1-fb21-5409-b6b3-606787668c02', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('792c016d-9750-5db7-b145-3b0fc03b56a6', '加油', 'expense', '6b978f24-2393-56a4-b3df-50c5d054cbc9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('6ea93961-fe8f-5b8a-94fa-d7945196499c', '充电', 'expense', '6b978f24-2393-56a4-b3df-50c5d054cbc9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('0a94252d-cd63-5839-b8b7-14081d09a05e', '停车', 'expense', '6b978f24-2393-56a4-b3df-50c5d054cbc9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('57f5b583-42e5-5883-b322-b144083eb24b', '过路费', 'expense', '6b978f24-2393-56a4-b3df-50c5d054cbc9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('ada0c378-79b7-5d70-9589-1d30cbd43213', '保养', 'expense', '6b978f24-2393-56a4-b3df-50c5d054cbc9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('780ff9cd-0abd-533d-8da4-ae098a8689db', '维修', 'expense', '6b978f24-2393-56a4-b3df-50c5d054cbc9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('dac68491-f40f-51d4-9f85-2fa202618221', '洗车', 'expense', '6b978f24-2393-56a4-b3df-50c5d054cbc9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('5afc659f-ee8a-599e-a5b4-146767438b4b', '年检', 'expense', '6b978f24-2393-56a4-b3df-50c5d054cbc9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('0c70f3d4-913c-5298-b42b-3f88b7d781f8', '车险', 'expense', '6b978f24-2393-56a4-b3df-50c5d054cbc9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('f8c8fd81-a550-51b9-b3d4-774b2c788f42', '美容改装', 'expense', '6b978f24-2393-56a4-b3df-50c5d054cbc9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('f6674c96-20d0-58c0-96c0-146b195408d0', '违章罚款', 'expense', '6b978f24-2393-56a4-b3df-50c5d054cbc9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('1ea56f03-33f0-5091-afb4-324f5aa368dc', '服饰鞋包', 'expense', '3feb7580-9bad-5c6a-bf4f-db9e59eb3e64', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('23ba0e2f-debd-5a43-ab1d-b3b952c7239c', '日用百货', 'expense', '3feb7580-9bad-5c6a-bf4f-db9e59eb3e64', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('4f1c7b66-a151-5385-a932-2e6e915b309e', '生鲜食材', 'expense', '3feb7580-9bad-5c6a-bf4f-db9e59eb3e64', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('7fd5d27f-07d7-59ff-bef4-e668a7142f8c', '美妆护肤', 'expense', '3feb7580-9bad-5c6a-bf4f-db9e59eb3e64', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('d91d8d71-01ab-5ba0-97bc-b88682699e2e', '母婴用品', 'expense', '3feb7580-9bad-5c6a-bf4f-db9e59eb3e64', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('6de6e7bf-b225-5c8e-9885-740589635610', '房租', 'expense', '7913daff-f5fc-5ce2-98a0-85c5f0c53db9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('4b060377-24e1-5b2e-b8a5-68b9171f4204', '房贷', 'expense', '7913daff-f5fc-5ce2-98a0-85c5f0c53db9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('d2dcb050-7127-5a7b-b342-b5609bf42e00', '装修', 'expense', '7913daff-f5fc-5ce2-98a0-85c5f0c53db9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('a72f44e4-5f54-53ae-bfaf-4b7351cdaf2a', '家居家具', 'expense', '7913daff-f5fc-5ce2-98a0-85c5f0c53db9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('0fb634f6-314f-5c45-ba06-fc4915bfd8e3', '家用电器', 'expense', '7913daff-f5fc-5ce2-98a0-85c5f0c53db9', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('c8d62572-2740-51c0-b7b8-f56358162cae', '电影演出', 'expense', '805a7628-6497-5252-b4ab-a76361e5aa0a', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('34011c0a-8745-5dc3-bddc-da94da1683ab', '游戏', 'expense', '805a7628-6497-5252-b4ab-a76361e5aa0a', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('7b0fcc75-ce2d-5a5c-a2ed-e9f3b975578c', '旅行出游', 'expense', '805a7628-6497-5252-b4ab-a76361e5aa0a', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('be0c0a08-70db-599b-9818-373a1fd9f0e2', '订阅会员', 'expense', '805a7628-6497-5252-b4ab-a76361e5aa0a', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('cae8295d-152d-5dde-837e-1bd8397d2f33', '健身运动', 'expense', '805a7628-6497-5252-b4ab-a76361e5aa0a', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('144d356a-db63-5d4d-809a-a6a390b67070', '门诊挂号', 'expense', 'f0683ffe-fe9c-593f-8701-4ec1c296b32c', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('30903616-8239-54fc-ba84-864168e309e9', '药品', 'expense', 'f0683ffe-fe9c-593f-8701-4ec1c296b32c', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('baf2cedb-19bb-5e14-af52-78269e4945c6', '体检', 'expense', 'f0683ffe-fe9c-593f-8701-4ec1c296b32c', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('70f4cdf9-c9a3-57a8-bb30-c1a23ce219d9', '住院手术', 'expense', 'f0683ffe-fe9c-593f-8701-4ec1c296b32c', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('68c58238-44b0-583e-9b1f-a3a477b3876f', '书籍', 'expense', 'b41989ae-e78a-59f2-9c02-4f904d8e6841', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('62770b01-ffac-53ab-8afc-5233d031d3c2', '培训课程', 'expense', 'b41989ae-e78a-59f2-9c02-4f904d8e6841', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('c6c37d47-2a0d-56c9-8a82-3076d8f605b3', '学费', 'expense', 'b41989ae-e78a-59f2-9c02-4f904d8e6841', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('4a23ad92-03bc-579a-9061-a0c1433bc4af', '文具', 'expense', 'b41989ae-e78a-59f2-9c02-4f904d8e6841', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('3ab9bbc0-1a23-536d-b991-3fd72294980a', '话费', 'expense', '3f673b97-a17f-5dc5-92fb-5bd4d40b7b2c', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('c0d39249-9757-5907-bba9-e9753c72d267', '宽带', 'expense', '3f673b97-a17f-5dc5-92fb-5bd4d40b7b2c', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('c2f14160-af52-5ca9-a6d0-e3609a40ccb8', '水费', 'expense', '3f673b97-a17f-5dc5-92fb-5bd4d40b7b2c', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('3803e94d-1f9f-5dc6-a253-422e40e18ebf', '电费', 'expense', '3f673b97-a17f-5dc5-92fb-5bd4d40b7b2c', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('27b5a15c-694a-5207-91ea-5f2a8535b7e9', '燃气费', 'expense', '3f673b97-a17f-5dc5-92fb-5bd4d40b7b2c', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('aa93df37-1e65-508f-8662-e0684c32a89c', '物业费', 'expense', '3f673b97-a17f-5dc5-92fb-5bd4d40b7b2c', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('95797241-395b-50d2-af9f-f41c2d60c9cd', '礼金红包', 'expense', '7e0c4d7e-15e9-5cbf-a3c9-059d14a86383', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('e686de0c-ed13-57ac-923b-dae433118721', '请客送礼', 'expense', '7e0c4d7e-15e9-5cbf-a3c9-059d14a86383', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('f9cff38f-1143-5c92-a35f-d0ba08a45649', '金融费用', 'expense', 'cabb2911-56c1-51b8-b6c7-e4cffbcabac4', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('9a67f397-e0fd-5347-ba4d-f464f3f79cfc', '寿险健康险', 'expense', 'cabb2911-56c1-51b8-b6c7-e4cffbcabac4', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('f2bc93ed-e8ce-5eab-8fb0-62864a123b12', '财产险', 'expense', 'cabb2911-56c1-51b8-b6c7-e4cffbcabac4', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('45d38666-9898-581a-b156-c9141ef1efb1', '手机', 'expense', '6f3bfe07-0782-52f4-8984-b147205dcba0', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('89fe4c6d-27e8-5fee-a1cb-6dcdfd1c0126', '电脑', 'expense', '6f3bfe07-0782-52f4-8984-b147205dcba0', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('9de977b5-4054-5e6e-9be1-4cee9f9920e3', '平板', 'expense', '6f3bfe07-0782-52f4-8984-b147205dcba0', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('f004f7d4-b7e5-5788-ab01-08b5cef0cfb0', '耳机音箱', 'expense', '6f3bfe07-0782-52f4-8984-b147205dcba0', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('8192cf4b-4da8-560d-8bd5-1db61847e9f2', '智能穿戴', 'expense', '6f3bfe07-0782-52f4-8984-b147205dcba0', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('3b5fc105-8e66-5d7d-8fc3-4256b55f1682', '游戏机', 'expense', '6f3bfe07-0782-52f4-8984-b147205dcba0', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('7fc87869-f53c-51b4-aeca-ec061c89847a', '软件服务', 'expense', '6f3bfe07-0782-52f4-8984-b147205dcba0', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('35ae5a07-45b9-579e-a1ee-729dc66df036', '数码配件', 'expense', '6f3bfe07-0782-52f4-8984-b147205dcba0', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed');

-- 收入顶级分类：5
INSERT OR IGNORE INTO categories (id, name, kind, icon, created_at, updated_at, version, device_id) VALUES
  ('5c7b17d7-a3ec-59c0-b2ad-4a62ad32f2c3', '工资', 'income', 'WalletOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('a163e39c-8eb4-5317-8ef9-7c433897b569', '奖金', 'income', 'TrophyOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('0aacf353-c7a5-5ac1-8da6-5b8815ffcef7', '投资收益', 'income', 'TrendingUpOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('60549cc0-9b4b-584c-8891-9705c0416247', '其他收入', 'income', 'EllipsisHorizontalOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('17494b6f-b527-5c4b-9af4-ecff194dba7d', '兼职劳务', 'income', 'BriefcaseOutline', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed');

-- 收入二级分类：11
INSERT OR IGNORE INTO categories (id, name, kind, parent_id, created_at, updated_at, version, device_id) VALUES
  ('2aa9d903-b065-5327-9e2d-172609635ee3', '基本工资', 'income', '5c7b17d7-a3ec-59c0-b2ad-4a62ad32f2c3', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('51fdb1dd-ca63-5b9e-899e-e067d4b31d14', '加班费', 'income', '5c7b17d7-a3ec-59c0-b2ad-4a62ad32f2c3', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('cd17c51c-cbaf-5168-90ee-59d89e976bb7', '补贴', 'income', '5c7b17d7-a3ec-59c0-b2ad-4a62ad32f2c3', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('c75a9e8e-13e7-57ef-9aca-19a18bb24f56', '年终奖', 'income', 'a163e39c-8eb4-5317-8ef9-7c433897b569', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('0d0e378b-c2bd-5470-8cb2-3169636498d5', '绩效奖金', 'income', 'a163e39c-8eb4-5317-8ef9-7c433897b569', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('6eb846e6-94e4-56cb-b1e3-e62e9740bffe', '股票分红', 'income', '0aacf353-c7a5-5ac1-8da6-5b8815ffcef7', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('a68e5626-f952-57f0-9123-b700ec762897', '基金收益', 'income', '0aacf353-c7a5-5ac1-8da6-5b8815ffcef7', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('2c758516-ac9d-53c8-9e13-2ba9086e55dc', '理财利息', 'income', '0aacf353-c7a5-5ac1-8da6-5b8815ffcef7', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('4e41cb64-9647-5106-bb5e-4502d8912c0a', '兼职', 'income', '17494b6f-b527-5c4b-9af4-ecff194dba7d', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('03f7c38e-4935-5fdd-bd2d-a08361ebbadb', '劳务报酬', 'income', '17494b6f-b527-5c4b-9af4-ecff194dba7d', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed'),
  ('891d7c2e-65b3-59a2-83aa-c886d2d997a2', '物品售出', 'income', '60549cc0-9b4b-584c-8891-9705c0416247', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1, 'seed');
