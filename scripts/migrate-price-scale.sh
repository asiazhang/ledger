#!/bin/sh
# 一次性存量库处置脚本（issue #300 / ADR-0038）：价格刻度 ×100——「分」翻转为「万分之一元」。
#
# 背景：v0.3.0 及之前版本价格列以「分」（1/100 元）存储；新版本 schema（V002/V010
# 就地修改，BREAKING）把全部价格语义列重定义为万分之一元（0.0001 元，基金净值
# 4 位小数保真）。存量库需按 CHANGELOG「Unreleased」BREAKING 条目执行本脚本一次，
# 否则价格显示 100 倍错乱。
#
# 脚本动作（单事务，原子执行）：
#   1. ×100 修正确定性派生的真实记录三列：
#      security_transactions.price_cents（成交单价）
#      security_lots.cost_per_unit_cents（每份成本）
#      security_lot_sales.cost_per_unit_cents（卖出时批次单位成本快照）
#   2. 清空可重建缓存（回填窗口本就两年，清空后重新同步零损失）：
#      market_prices（现价）/ price_history（价格历史）/ fx_rate_history（汇率历史）
#   3. 重建 v_holdings 视图（表达式含 ÷100 换算，与新 V002 同源；存量库保留旧视图
#      定义会让市值 100 倍错乱，故必须重建）。
#   4. 落 app_settings 标记（migration.price_scale_x100_v4），重复执行直接拒绝。
#
# 不动的内容：交易金额列（amount_cents 等）与手续费（fee_cents）仍是整数分；
# exchange_rates / fx_rate_history.rate 是比值非价格；instruments 字典不涉刻度。
#
# 执行后请重新同步价格：标的页「全量同步」修字典 + 「同步持仓价格」回填现价与
# 近两年走势（基金净值同步属后续议题，本脚本不含手动价/基金净值的处置义务——
# 本票执行时库内无手动价，见 #300 协调注记）。
#
# 用法:
#   ./scripts/migrate-price-scale.sh               # 处置默认位置的账本库
#   ./scripts/migrate-price-scale.sh /path/db.db   # 处置指定库
#
# 依赖: sqlite3 CLI（macOS 自带；Linux 多数发行版预装）
set -eu
cd "$(dirname "$0")/.."

IDENTIFIER="com.zhangheng.ledger"
case "$(uname -s)" in
  Darwin) DEFAULT_DB="$HOME/Library/Application Support/$IDENTIFIER/ledger.db" ;;
  Linux)  DEFAULT_DB="$HOME/.local/share/$IDENTIFIER/ledger.db" ;;
  *) echo "❌ 不支持的系统: $(uname -s)"; exit 1 ;;
esac

DB="${1:-$DEFAULT_DB}"
if [ ! -f "$DB" ]; then
  echo "❌ 未找到数据库: $DB"
  echo "   可显式传入路径: ./scripts/migrate-price-scale.sh /path/to/ledger.db"
  exit 1
fi
echo "📍 数据库: $DB"

if ! command -v sqlite3 >/dev/null 2>&1; then
  echo "❌ 未安装 sqlite3 CLI。macOS 可用 'brew install sqlite' 安装。"
  exit 1
fi

# 幂等守卫：已执行过则拒绝（标记落 app_settings，key 规范 <feature>.<name>；
# 仅本脚本读写、后端 SettingKey 枚举无消费方，故不走 settings 模块收口）。
DONE=$(sqlite3 "$DB" "SELECT COUNT(*) FROM app_settings WHERE key='migration.price_scale_x100_v4';" 2>/dev/null || echo 0)
if [ "$DONE" != "0" ]; then
  echo "✅ 该库已执行过价格刻度 ×100 处置（app_settings 标记存在），无需重复执行。"
  exit 0
fi

ROWS_TX=$(sqlite3 "$DB" "SELECT COUNT(*) FROM security_transactions WHERE price_cents IS NOT NULL;")
ROWS_LOT=$(sqlite3 "$DB" "SELECT COUNT(*) FROM security_lots;")
ROWS_SALE=$(sqlite3 "$DB" "SELECT COUNT(*) FROM security_lot_sales;")
echo "▶ 将处理：成交单价 $ROWS_TX 行、每份成本 $ROWS_LOT 行、卖出成本快照 $ROWS_SALE 行"
echo "  并清空现价/价格历史/汇率历史缓存（重新同步即回填）。"

printf "确认执行？此操作不可撤销（建议先在应用内做一次手动备份）。[y/N] "
read -r ANSWER
case "$ANSWER" in
  y | Y | yes | YES) ;;
  *) echo "已取消。"; exit 0 ;;
esac

sqlite3 "$DB" <<'SQL'
BEGIN IMMEDIATE;

-- 1. 真实记录 ×100：分 → 万分之一元
UPDATE security_transactions SET price_cents = price_cents * 100 WHERE price_cents IS NOT NULL;
UPDATE security_lots        SET cost_per_unit_cents = cost_per_unit_cents * 100;
UPDATE security_lot_sales   SET cost_per_unit_cents = cost_per_unit_cents * 100;

-- 2. 清空可重建缓存（重新同步即回填）
DELETE FROM market_prices;
DELETE FROM price_history;
DELETE FROM fx_rate_history;

-- 3. 重建 v_holdings：表达式与新 V002 同源（数量 × 万分之一元单价 ÷ 100 = 金额分）。
--    ⚠ 与 src-tauri/migrations/V002__investment.sql 的视图定义保持一致；改其一必须同步另一个。
DROP VIEW IF EXISTS v_holdings;
CREATE VIEW v_holdings AS
SELECT
    h.id,
    h.account_id,
    h.instrument_id,
    h.quantity,
    h.cost_basis_cents,
    h.currency_code AS cost_currency_code,
    p.price_cents AS latest_price_cents,
    p.currency_code AS latest_price_currency_code,
    CASE
        WHEN p.price_cents IS NULL THEN NULL
        WHEN p.currency_code = a.currency_code THEN CAST(ROUND(h.quantity * p.price_cents / 100.0) AS INTEGER)
        WHEN er.rate IS NOT NULL THEN CAST(ROUND(h.quantity * p.price_cents * er.rate / 100.0) AS INTEGER)
        WHEN er_rev.rate IS NOT NULL THEN CAST(ROUND(h.quantity * p.price_cents / er_rev.rate / 100.0) AS INTEGER)
        ELSE NULL
    END AS market_value_cents,
    CASE
        WHEN p.price_cents IS NULL THEN NULL
        ELSE
            (CASE
                WHEN p.currency_code = a.currency_code THEN CAST(ROUND(h.quantity * p.price_cents / 100.0) AS INTEGER)
                WHEN er.rate IS NOT NULL THEN CAST(ROUND(h.quantity * p.price_cents * er.rate / 100.0) AS INTEGER)
                WHEN er_rev.rate IS NOT NULL THEN CAST(ROUND(h.quantity * p.price_cents / er_rev.rate / 100.0) AS INTEGER)
                ELSE NULL
            END)
            -
            (CASE
                WHEN h.currency_code = a.currency_code THEN h.cost_basis_cents
                WHEN ec.rate IS NOT NULL THEN CAST(ROUND(h.cost_basis_cents * ec.rate) AS INTEGER)
                WHEN ec_rev.rate IS NOT NULL THEN CAST(ROUND(h.cost_basis_cents / ec_rev.rate) AS INTEGER)
                ELSE NULL
            END)
    END AS unrealized_pnl_cents,
    h.updated_at
FROM (
    SELECT
        account_id || '-' || instrument_id || '-' || currency_code AS id,
        account_id,
        instrument_id,
        SUM(remaining_quantity) AS quantity,
        CAST(ROUND(SUM(remaining_quantity * cost_per_unit_cents) / 100.0) AS INTEGER) AS cost_basis_cents,
        currency_code,
        MAX(updated_at) AS updated_at
    FROM security_lots
    WHERE remaining_quantity > 0
      AND account_id IN (SELECT id FROM accounts WHERE is_deleted = 0)
    GROUP BY account_id, instrument_id, currency_code
) h
LEFT JOIN accounts a ON a.id = h.account_id
LEFT JOIN market_prices p ON p.instrument_id = h.instrument_id
LEFT JOIN exchange_rates er     ON er.base_code = p.currency_code     AND er.quote_code = a.currency_code
LEFT JOIN exchange_rates er_rev ON er_rev.base_code = a.currency_code AND er_rev.quote_code = p.currency_code
LEFT JOIN exchange_rates ec     ON ec.base_code = h.currency_code     AND ec.quote_code = a.currency_code
LEFT JOIN exchange_rates ec_rev ON ec_rev.base_code = a.currency_code AND ec_rev.quote_code = h.currency_code;

-- 4. 完成标记
INSERT INTO app_settings (key, value) VALUES ('migration.price_scale_x100_v4', 'done');

COMMIT;
SQL

echo "✅ 处置完成："
echo "   - 真实记录价格列已 ×100（成交单价 / 每份成本 / 卖出成本快照）"
echo "   - 现价 / 价格历史 / 汇率历史缓存已清空"
echo "   - v_holdings 视图已重建（万分之一元刻度口径）"
echo
echo "▶ 下一步：启动应用，在标的页执行「全量同步」与「同步持仓价格」回填价格数据。"
