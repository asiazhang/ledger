-- V002 默认种子数据：币种与分类
-- currencies.code 为主键，用 INSERT OR IGNORE 去重；
-- 顶级分类用 WHERE NOT EXISTS 基于 (name,kind) 去重；
-- 二级分类用 JOIN 父分类定位 parent_id，并按 (name,kind,parent_id) 三元组去重。
-- created_at 与 now_iso() 保持同格式：strftime('%Y-%m-%dT%H:%M:%SZ','now')。
-- 应用未发布前直接扩充种子；无「退款报销」分类（退款走 transactions.kind='refund'，复用原支出交易分类）。

INSERT OR IGNORE INTO currencies (code, name, symbol, decimal_places) VALUES
  ('CNY', '人民币', '¥', 2),
  ('USD', '美元', '$', 2),
  ('EUR', '欧元', '€', 2);

-- 顶级分类（16）：支出 11 + 收入 5
WITH seed(name, kind) AS (
  VALUES
    ('餐饮', 'expense'), ('交通', 'expense'), ('购物', 'expense'), ('住房', 'expense'),
    ('娱乐', 'expense'), ('医疗', 'expense'), ('教育', 'expense'), ('其他支出', 'expense'),
    ('通讯', 'expense'), ('人情', 'expense'), ('金融保险', 'expense'),
    ('工资', 'income'), ('奖金', 'income'), ('投资收益', 'income'), ('其他收入', 'income'),
    ('兼职劳务', 'income')
)
INSERT INTO categories (name, kind, created_at)
SELECT s.name, s.kind, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM seed s
WHERE NOT EXISTS (
  SELECT 1 FROM categories c WHERE c.name = s.name AND c.kind = s.kind
);

-- 二级分类（48）：支出 38 + 收入 10
-- parent_name 指向同 kind 的顶级分类；按 (name,kind,parent_id) 三元组去重，
-- 避免不同父下同名子类误判，也保证对已有数据的老用户库升级安全。
WITH seed(name, kind, parent_name) AS (
  VALUES
    -- 餐饮
    ('早餐', 'expense', '餐饮'), ('午餐', 'expense', '餐饮'), ('晚餐', 'expense', '餐饮'),
    ('零食饮料', 'expense', '餐饮'), ('外卖', 'expense', '餐饮'), ('聚餐', 'expense', '餐饮'),
    -- 交通
    ('公交地铁', 'expense', '交通'), ('出租车', 'expense', '交通'), ('加油', 'expense', '交通'),
    ('停车过路', 'expense', '交通'), ('火车机票', 'expense', '交通'),
    -- 购物
    ('服饰鞋包', 'expense', '购物'), ('日用百货', 'expense', '购物'), ('数码电器', 'expense', '购物'),
    ('美妆护肤', 'expense', '购物'), ('家居家具', 'expense', '购物'),
    -- 住房
    ('房租', 'expense', '住房'), ('物业费', 'expense', '住房'),
    ('水电燃气', 'expense', '住房'), ('宽带', 'expense', '住房'),
    -- 娱乐
    ('电影演出', 'expense', '娱乐'), ('游戏', 'expense', '娱乐'),
    ('旅行出游', 'expense', '娱乐'), ('订阅会员', 'expense', '娱乐'),
    -- 医疗
    ('门诊挂号', 'expense', '医疗'), ('药品', 'expense', '医疗'), ('体检', 'expense', '医疗'),
    -- 教育
    ('书籍', 'expense', '教育'), ('培训课程', 'expense', '教育'),
    ('学费', 'expense', '教育'), ('文具', 'expense', '教育'),
    -- 通讯
    ('话费', 'expense', '通讯'), ('流量套餐', 'expense', '通讯'),
    -- 人情
    ('礼金红包', 'expense', '人情'), ('请客送礼', 'expense', '人情'),
    -- 金融保险
    ('手续费', 'expense', '金融保险'), ('利息支出', 'expense', '金融保险'), ('保险费', 'expense', '金融保险'),
    -- 工资
    ('基本工资', 'income', '工资'), ('加班费', 'income', '工资'), ('补贴', 'income', '工资'),
    -- 奖金
    ('年终奖', 'income', '奖金'), ('绩效奖金', 'income', '奖金'),
    -- 投资收益
    ('股票分红', 'income', '投资收益'), ('基金收益', 'income', '投资收益'), ('理财利息', 'income', '投资收益'),
    -- 兼职劳务
    ('兼职', 'income', '兼职劳务'), ('劳务报酬', 'income', '兼职劳务')
)
INSERT INTO categories (name, kind, parent_id, created_at)
SELECT s.name, s.kind, p.id, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM seed s
JOIN categories p
  ON p.name = s.parent_name AND p.kind = s.kind AND p.parent_id IS NULL
WHERE NOT EXISTS (
  SELECT 1 FROM categories c
  WHERE c.name = s.name AND c.kind = s.kind AND c.parent_id = p.id
);
