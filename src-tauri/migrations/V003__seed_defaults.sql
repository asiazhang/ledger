-- V002 默认种子数据：币种与分类
-- currencies.code 为主键，用 INSERT OR IGNORE 去重；
-- 顶级分类用 WHERE NOT EXISTS 基于 (name,kind) 去重；
-- 二级分类用 JOIN 父分类定位 parent_id，并按 (name,kind,parent_id) 三元组去重。
-- created_at 与 now_iso() 保持同格式：strftime('%Y-%m-%dT%H:%M:%SZ','now')。
-- 应用未发布前直接扩充种子；无「退款报销」分类（退款走 transactions.kind='refund'，复用原支出交易分类）。

INSERT OR IGNORE INTO currencies (code, name, symbol, decimal_places) VALUES
  ('CNY', '人民币', '¥', 2),
  ('USD', '美元', '$', 2),
  ('EUR', '欧元', '€', 2),
  ('JPY', '日元', '¥', 2),
  ('GBP', '英镑', '£', 2),
  ('HKD', '港币', 'HK$', 2),
  ('AUD', '澳元', 'A$', 2),
  ('CAD', '加元', 'C$', 2),
  ('KRW', '韩元', '₩', 0),
  ('SGD', '新加坡元', 'S$', 2),
  ('CHF', '瑞士法郎', 'Fr.', 2);

-- 支出顶级分类：13
WITH seed(name) AS (
  VALUES
    ('餐饮'), ('交通'), ('购物'), ('住房'), ('娱乐'),
    ('医疗'), ('教育'), ('其他支出'), ('生活缴费'), ('人情'),
    ('金融保险'), ('数码产品'), ('汽车')
)
INSERT INTO categories (name, kind, created_at)
SELECT s.name, 'expense', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM seed s
WHERE NOT EXISTS (
  SELECT 1 FROM categories c WHERE c.name = s.name AND c.kind = 'expense' AND c.parent_id IS NULL
);

-- 支出二级分类：61
WITH seed(name, parent_name) AS (
  VALUES
    -- 餐饮
    ('早餐', '餐饮'), ('午餐', '餐饮'), ('晚餐', '餐饮'),
    ('零食饮料', '餐饮'), ('外卖', '餐饮'), ('聚餐', '餐饮'),
    -- 交通
    ('公交地铁', '交通'), ('出租车', '交通'), ('火车机票', '交通'), ('共享出行', '交通'),
    -- 汽车
    ('加油', '汽车'), ('充电', '汽车'),
    ('停车', '汽车'), ('过路费', '汽车'),
    ('保养', '汽车'), ('维修', '汽车'),
    ('洗车', '汽车'), ('年检', '汽车'),
    ('车险', '汽车'), ('美容改装', '汽车'),
    ('违章罚款', '汽车'),
    -- 购物
    ('服饰鞋包', '购物'), ('日用百货', '购物'),
    ('生鲜食材', '购物'), ('美妆护肤', '购物'), ('母婴用品', '购物'),
    -- 住房
    ('房租', '住房'), ('房贷', '住房'), ('装修', '住房'), ('家居家具', '住房'), ('家用电器', '住房'),
    -- 娱乐
    ('电影演出', '娱乐'), ('游戏', '娱乐'), ('旅行出游', '娱乐'),
    ('订阅会员', '娱乐'), ('健身运动', '娱乐'),
    -- 医疗
    ('门诊挂号', '医疗'), ('药品', '医疗'), ('体检', '医疗'), ('住院手术', '医疗'),
    -- 教育
    ('书籍', '教育'), ('培训课程', '教育'), ('学费', '教育'), ('文具', '教育'),
    -- 生活缴费
    ('话费', '生活缴费'), ('宽带', '生活缴费'),
    ('水费', '生活缴费'), ('电费', '生活缴费'), ('燃气费', '生活缴费'), ('物业费', '生活缴费'),
    -- 人情
    ('礼金红包', '人情'), ('请客送礼', '人情'),
    -- 金融保险
    ('金融费用', '金融保险'),
    ('寿险健康险', '金融保险'), ('财产险', '金融保险'),
    -- 数码产品
    ('手机', '数码产品'), ('电脑', '数码产品'), ('平板', '数码产品'),
    ('耳机音箱', '数码产品'), ('智能穿戴', '数码产品'),
    ('游戏机', '数码产品'), ('软件服务', '数码产品'), ('数码配件', '数码产品')
)
INSERT INTO categories (name, kind, parent_id, created_at)
SELECT s.name, 'expense', p.id, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM seed s
JOIN categories p ON p.name = s.parent_name AND p.kind = 'expense' AND p.parent_id IS NULL
WHERE NOT EXISTS (
  SELECT 1 FROM categories c WHERE c.name = s.name AND c.kind = 'expense' AND c.parent_id = p.id
);

-- 收入顶级分类：5
WITH seed(name) AS (
  VALUES
    ('工资'), ('奖金'), ('投资收益'), ('其他收入'), ('兼职劳务')
)
INSERT INTO categories (name, kind, created_at)
SELECT s.name, 'income', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM seed s
WHERE NOT EXISTS (
  SELECT 1 FROM categories c WHERE c.name = s.name AND c.kind = 'income' AND c.parent_id IS NULL
);

-- 收入二级分类：11
WITH seed(name, parent_name) AS (
  VALUES
    -- 工资
    ('基本工资', '工资'), ('加班费', '工资'), ('补贴', '工资'),
    -- 奖金
    ('年终奖', '奖金'), ('绩效奖金', '奖金'),
    -- 投资收益
    ('股票分红', '投资收益'), ('基金收益', '投资收益'), ('理财利息', '投资收益'),
    -- 兼职劳务
    ('兼职', '兼职劳务'), ('劳务报酬', '兼职劳务'),
    -- 其他收入
    ('物品售出', '其他收入')
)
INSERT INTO categories (name, kind, parent_id, created_at)
SELECT s.name, 'income', p.id, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM seed s
JOIN categories p ON p.name = s.parent_name AND p.kind = 'income' AND p.parent_id IS NULL
WHERE NOT EXISTS (
  SELECT 1 FROM categories c WHERE c.name = s.name AND c.kind = 'income' AND c.parent_id = p.id
);
