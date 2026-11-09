-- V026：信用卡档案列——信用额度 / 账单日 / 还款日（spec #1327 / ADR-0119）。加列只增，
-- 不改任何既有对象、零 CHECK 改动、零 BREAKING。
--
-- The three columns are the credit-card profile fields of an account. Only type
-- `credit` may carry them; they are archival values — they take part in no balance,
-- no net-worth and no conversion reckoning (the balance remains the single answer to
-- "how much do I still owe").
--
-- credit_limit_cents：信用额度（整数分，> 0；NULL = 未设置）。额度币种恒为账户
-- 自身币种（账户是单币种实体），不另绑第二币种。
-- statement_day / due_day：账单日与还款日的**声明值**（1–31 的「每月第 N 日」），
-- NULL = 未设置；两者彼此独立、不强制先后（银行存在「账单日 25 日 / 还款日 14 日」
-- 的跨月形态）。存储保留声明值原样，月末钳制（当月无该日取当月最后一天）只发生在
-- 派生「下次账单日 / 下次还款日」时，不落库。
--
-- schema 层纪律（与 V023 同款）：只设可空列，不设约束——「仅 credit 账户可携带」
-- 是跨类型规则，`ALTER TABLE ADD COLUMN` 无法追加多列 CHECK；范围校验同样不收进列
-- CHECK，使字段校验保持单源（域层守卫 + 码化错误），不出现第二套口径。

ALTER TABLE accounts ADD COLUMN credit_limit_cents INTEGER;
ALTER TABLE accounts ADD COLUMN statement_day INTEGER;
ALTER TABLE accounts ADD COLUMN due_day INTEGER;
