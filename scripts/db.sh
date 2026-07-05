#!/bin/sh
# 定位并查看 SQLite 数据库
# macOS: ~/Library/Application Support/com.zhangheng.ledger/ledger.db
# Linux: ~/.local/share/com.zhangheng.ledger/ledger.db
#
# 用法:
#   ./scripts/db.sh                 # 打印路径 + 表列表 + 结构 + 行数
#   ./scripts/db.sh "SELECT * FROM transactions LIMIT 5;"
#   ./scripts/db.sh                 # 也可传 sqlite3 子命令，如 ".schema accounts"
set -eu
cd "$(dirname "$0")/.."

IDENTIFIER="com.zhangheng.ledger"
case "$(uname -s)" in
  Darwin) DB="$HOME/Library/Application Support/$IDENTIFIER/ledger.db" ;;
  Linux)  DB="$HOME/.local/share/$IDENTIFIER/ledger.db" ;;
  *) echo "❌ 不支持的系统: $(uname -s)"; exit 1 ;;
esac

if [ ! -f "$DB" ]; then
  echo "❌ 未找到数据库: $DB"
  echo "   请先运行 ./scripts/dev.sh 启动应用以初始化数据库"
  exit 1
fi

echo "📍 数据库路径: $DB"
echo

if ! command -v sqlite3 >/dev/null 2>&1; then
  echo "⚠ 未安装 sqlite3 CLI，仅打印路径。macOS 可用 'brew install sqlite' 安装。"
  exit 0
fi

if [ "$#" -gt 0 ]; then
  # 既支持 SQL 语句，也支持 sqlite3 点命令（如 .schema）
  exec sqlite3 -header -column "$DB" "$@"
fi

echo "▶ 表列表:"
sqlite3 "$DB" ".tables"
echo
echo "▶ 表结构:"
sqlite3 "$DB" ".schema"
echo
echo "▶ 各表行数:"
sqlite3 -header -column "$DB" "
  SELECT 'currencies'   AS tbl, COUNT(*) AS rows FROM currencies
  UNION ALL SELECT 'accounts',     COUNT(*) FROM accounts
  UNION ALL SELECT 'categories',   COUNT(*) FROM categories
  UNION ALL SELECT 'transactions', COUNT(*) FROM transactions
  UNION ALL SELECT 'budgets',      COUNT(*) FROM budgets;
"
echo
echo "提示: 自定义查询 ->  ./scripts/db.sh \"SELECT * FROM transactions LIMIT 5;\""
echo "提示: 查看某表结构 -> ./scripts/db.sh \".schema accounts\""
