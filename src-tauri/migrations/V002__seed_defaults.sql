-- V002 默认种子数据：币种与常用分类
-- 与原 db.rs::seed_defaults 等价的迁移化版本。
-- currencies.code 为主键，用 INSERT OR IGNORE 去重；
-- categories.name 无唯一约束，用 WHERE NOT EXISTS 基于 (name,kind) 去重，
-- 保证对已有数据的老用户库升级时安全（不产生重复分类）。
-- created_at 与 now_iso() 保持同格式：strftime('%Y-%m-%dT%H:%M:%SZ','now')。

INSERT OR IGNORE INTO currencies (code, name, symbol, decimal_places) VALUES
  ('CNY', '人民币', '¥', 2),
  ('USD', '美元', '$', 2),
  ('EUR', '欧元', '€', 2);

WITH seed(name, kind) AS (
  VALUES
    ('餐饮', 'expense'), ('交通', 'expense'), ('购物', 'expense'), ('住房', 'expense'),
    ('娱乐', 'expense'), ('医疗', 'expense'), ('教育', 'expense'), ('其他支出', 'expense'),
    ('工资', 'income'), ('奖金', 'income'), ('投资收益', 'income'), ('其他收入', 'income')
)
INSERT INTO categories (name, kind, created_at)
SELECT s.name, s.kind, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM seed s
WHERE NOT EXISTS (
  SELECT 1 FROM categories c WHERE c.name = s.name AND c.kind = s.kind
);
