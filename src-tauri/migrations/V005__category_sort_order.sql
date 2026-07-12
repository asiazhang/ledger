-- V005 分类 sort_order + icon/color 种子数据补充
--
-- 为 categories 表新增 sort_order 列支持手动排序，
-- 并将现有 icon/color 空值补齐为默认 emoji 与色值。

ALTER TABLE categories ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

-- 旧数据以 rowid 作为初始排序值（按 kind 分组）
UPDATE categories SET sort_order = rowid WHERE sort_order = 0;

-- 更新种子数据的 icon 和 color（支出）
UPDATE categories SET icon = '🍜', color = '#FF6B6B' WHERE name = '餐饮' AND kind = 'expense';
UPDATE categories SET icon = '🚌', color = '#4ECDC4' WHERE name = '交通' AND kind = 'expense';
UPDATE categories SET icon = '🏠', color = '#45B7D1' WHERE name = '居住' AND kind = 'expense';
UPDATE categories SET icon = '👕', color = '#96CEB4' WHERE name = '衣物' AND kind = 'expense';
UPDATE categories SET icon = '📱', color = '#FFEAA7' WHERE name = '数码' AND kind = 'expense';
UPDATE categories SET icon = '📚', color = '#DDA0DD' WHERE name = '教育' AND kind = 'expense';
UPDATE categories SET icon = '🏥', color = '#FF8C94' WHERE name = '医疗' AND kind = 'expense';
UPDATE categories SET icon = '🎮', color = '#A8E6CF' WHERE name = '娱乐' AND kind = 'expense';
UPDATE categories SET icon = '💇', color = '#FDCB6E' WHERE name = '美容' AND kind = 'expense';
UPDATE categories SET icon = '🐱', color = '#E17055' WHERE name = '宠物' AND kind = 'expense';
UPDATE categories SET icon = '🎁', color = '#00CEC9' WHERE name = '礼赠' AND kind = 'expense';
UPDATE categories SET icon = '💼', color = '#636E72' WHERE name = '办公' AND kind = 'expense';
UPDATE categories SET icon = '❓', color = '#B2BEC3' WHERE name = '其他' AND kind = 'expense';

-- 更新种子数据的 icon 和 color（收入）
UPDATE categories SET icon = '💰', color = '#00B894' WHERE name = '工资' AND kind = 'income';
UPDATE categories SET icon = '📈', color = '#0984E3' WHERE name = '理财' AND kind = 'income';
UPDATE categories SET icon = '🎯', color = '#6C5CE7' WHERE name = '兼职' AND kind = 'income';
UPDATE categories SET icon = '🧧', color = '#E17055' WHERE name = '红包' AND kind = 'income';
UPDATE categories SET icon = '❓', color = '#B2BEC3' WHERE name = '其他' AND kind = 'income';

-- 子分类继承父分类的 icon
UPDATE categories SET icon = '🍜' WHERE name IN ('早餐','午餐','晚餐','零食','水果','买菜','外卖') AND kind = 'expense';
UPDATE categories SET icon = '🚌' WHERE name IN ('打车','公交','加油','停车','过路费') AND kind = 'expense';
UPDATE categories SET icon = '🏠' WHERE name IN ('房租','水电','物业','装修','家具') AND kind = 'expense';
UPDATE categories SET icon = '👕' WHERE name IN ('衣服','鞋包','配饰') AND kind = 'expense';
UPDATE categories SET icon = '📱' WHERE name IN ('手机','电脑','配件','软件') AND kind = 'expense';
UPDATE categories SET icon = '📚' WHERE name IN ('书籍','课程','考试') AND kind = 'expense';
UPDATE categories SET icon = '🏥' WHERE name IN ('门诊','药品','体检') AND kind = 'expense';
UPDATE categories SET icon = '🎮' WHERE name IN ('游戏','影视','运动','旅行') AND kind = 'expense';
UPDATE categories SET icon = '💇' WHERE name IN ('理发','护肤','化妆') AND kind = 'expense';
UPDATE categories SET icon = '🐱' WHERE name IN ('宠物食物','宠物医疗') AND kind = 'expense';
UPDATE categories SET icon = '🎁' WHERE name IN ('送礼','人情') AND kind = 'expense';
UPDATE categories SET icon = '💼' WHERE name IN ('文具','打印') AND kind = 'expense';
UPDATE categories SET icon = '💰' WHERE name IN ('基本工资','奖金','补贴') AND kind = 'income';
UPDATE categories SET icon = '📈' WHERE name IN ('利息','股息','租金') AND kind = 'income';
