-- V018 备注拼音首字母冗余列（issue #492 / 父 #489 B3，修订 ADR-0027 实现条款）
--
-- 目标：备注拼音搜索 2932ms → ≤200ms（CI p95），匹配语义与验收口径零变更
-- （ADR-0027 统一模糊搜索语义规格不动：原文连续子串 ∨ 拼音首字母子序列，
-- 词条 AND、字段 OR；FTS5 仍排除——子序列语义与倒排索引不兼容的结论仍成立）。
--
-- note_pinyin 为 note 的拼音首字母串（小写，与 Rust `pinyin_initials` 同规则），
-- 属派生冗余列（单一事实来源恒为 note）：搜索流式匹配阶段免逐行重算拼音。
-- 写入路径由 Writer 接缝在同一写入中维护（insert_row / update_row）；存量行由
-- Rust 惰性回填（搜索读路径按需分批回填，见 transaction::search）。列为可选
-- （NULL）：回填前的存量行按 note 现算拼音兜底匹配，语义不受回填进度影响。
ALTER TABLE transactions ADD COLUMN note_pinyin TEXT;

-- 惰性回填探针索引：仅「有备注但未回填」的存量行进索引，回填收敛后恒空——
-- 供搜索读路径 O(1) 探测积压，避免每次搜索为探测积压全表扫描。
CREATE INDEX IF NOT EXISTS idx_transactions_note_pinyin_backlog
    ON transactions(id) WHERE note_pinyin IS NULL AND note IS NOT NULL;

-- 搜索扫描覆盖索引：第一段最小列（列表序键 + id/note/note_pinyin/三个引用列）
-- 全部进索引，第一段流式扫描为 index-only（实测 50 万笔 860ms → 233ms——列表序
-- 索引供序时逐行回表取 note 等列是随机页读，为主要成本）。全部列为引用或派生列，
-- 不承载独立语义；查询侧含关键字路径以 INDEXED BY 钉定本索引（子序列语义决定
-- 全量扫描本质，先例 V016 月度表达式索引钉定）。
CREATE INDEX IF NOT EXISTS idx_transactions_note_search
    ON transactions(date, created_at, id, note, note_pinyin, account_id, merchant_id, category_id)
    WHERE is_deleted = 0;
