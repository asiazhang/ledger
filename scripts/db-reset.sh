#!/bin/sh
# 删除数据库后重启开发环境
# 用于调试 init_db 初始化逻辑、种子数据、Schema 迁移
set -eu
cd "$(dirname "$0")/.."

IDENTIFIER="com.zhangheng.ledger"
case "$(uname -s)" in
  Darwin) DB="$HOME/Library/Application Support/$IDENTIFIER/ledger.db" ;;
  Linux)  DB="$HOME/.local/share/$IDENTIFIER/ledger.db" ;;
  *) echo "❌ 不支持的系统: $(uname -s)"; exit 1 ;;
esac

if [ -f "$DB" ]; then
  rm -f "$DB"
  echo "🗑 已删除数据库: $DB"
else
  echo "ℹ 数据库不存在，将首次初始化: $DB"
fi

echo "▶ 启动开发环境"
exec pnpm run tauri dev
