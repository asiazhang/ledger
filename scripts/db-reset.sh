#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

IDENTIFIER="com.zhangheng.ledger"
case "$(uname -s)" in
  Darwin) DEFAULT_DIR="$HOME/Library/Application Support/$IDENTIFIER" ;;
  Linux)  DEFAULT_DIR="$HOME/.local/share/$IDENTIFIER" ;;
  *) echo "❌ 不支持的系统: $(uname -s)"; exit 1 ;;
esac

REGISTRY_FILE="$DEFAULT_DIR/data_location.json"

JQ_RESOLVE='
  (.version // null) as $version
  | (.books // null) as $books_raw
  | (.active // null) as $active_raw
  | (.relocation // null) as $reloc_raw
  | if $version != null and $version != 1 then
      {"kind":"corrupt"}
    elif $books_raw != null and $active_raw != null then
      if ($books_raw | type) != "array" then
        {"kind":"corrupt"}
      else
        ([$books_raw[] | {
            id:   (.id   // "" | gsub("^\\s+|\\s+$"; "")),
            name: (.name // "" | gsub("^\\s+|\\s+$"; "")),
            dir:  (.dir  // "" | gsub("^\\s+|\\s+$"; ""))}]) as $books
        | ($active_raw | gsub("^\\s+|\\s+$"; "")) as $active
        | ($reloc_raw | if . == null then null else
            {book_id:  (.book_id // "" | gsub("^\\s+|\\s+$"; "")),
             from_dir: (.from_dir // "" | gsub("^\\s+|\\s+$"; ""))} end) as $reloc
        | if ($books | length) == 0
             or ($books | any(.id == "" or .name == "" or .dir == ""))
             or ($books | map(.id) | unique | length) != ($books | length)
             or ($books | map(.dir) | unique | length) != ($books | length)
             or ($books | map(select(.id == $active)) | length) != 1
             or ($reloc != null and
                 ($reloc.book_id == "" or $reloc.from_dir == ""
                  or ($books | map(select(.id == $reloc.book_id)) | length) != 1))
          then {"kind":"corrupt"}
          else
            {"kind":"registry",
             "active_dir": ($books | map(select(.id == $active)) | .[0] | .dir),
             "other_dirs": ($books | map(select(.id != $active) | .dir))}
          end
      end
    elif $books_raw != null or $active_raw != null then
      {"kind":"corrupt"}
    else
      (.data_dir // "" | gsub("^\\s+|\\s+$"; "")) as $dir
      | if $dir == "" then {"kind":"corrupt"} else {"kind":"pointer", "dir": $dir} end
    end'

remove_db_in_dir() {
  _dir="$1"
  _db="$_dir/ledger.db"
  if [ -f "$_db" ]; then
    rm -f "$_db"
    echo "🗑 已删除数据库: $_db"
  else
    echo "ℹ 数据库不存在，将首次初始化: $_db"
  fi
}

CUSTOM_DIR=""
OTHER_DIRS=""
if [ -f "$REGISTRY_FILE" ]; then
  if ! command -v jq >/dev/null 2>&1; then
    echo "⚠ 检测到 $REGISTRY_FILE 但未安装 jq，无法解析自定义数据位置，仅重置默认目录"
  else
    _STATE="$(jq -c "$JQ_RESOLVE" "$REGISTRY_FILE" 2>/dev/null)" || _STATE='{"kind":"corrupt"}'
    [ -n "$_STATE" ] || _STATE='{"kind":"corrupt"}'
    _KIND="$(printf '%s' "$_STATE" | jq -r '.kind // "corrupt"')"
    case "$_KIND" in
      pointer)
        CUSTOM_DIR="$(printf '%s' "$_STATE" | jq -r '.dir')"
        ;;
      registry)
        CUSTOM_DIR="$(printf '%s' "$_STATE" | jq -r '.active_dir // ""')"
        OTHER_DIRS="$(printf '%s' "$_STATE" | jq -r '.other_dirs // [] | .[]')"
        ;;
      *)
        echo "⚠ $REGISTRY_FILE 无法解析，视同未配置，仅重置默认目录"
        ;;
    esac
    if [ "$CUSTOM_DIR" = "$DEFAULT_DIR" ]; then
      CUSTOM_DIR=""
    fi
  fi
fi

remove_db_in_dir "$DEFAULT_DIR"

if [ -n "$CUSTOM_DIR" ]; then
  echo "📍 检测到自定义数据位置，一并重置: $CUSTOM_DIR"
  remove_db_in_dir "$CUSTOM_DIR"
fi

if [ -n "$OTHER_DIRS" ]; then
  echo "ℹ 注册表中还有其他账本目录，不自动删除（如需清理请手动处理）:"
  printf '%s\n' "$OTHER_DIRS" | while IFS= read -r _d; do
    if [ -n "$_d" ] && [ "$_d" != "$DEFAULT_DIR" ] && [ "$_d" != "$CUSTOM_DIR" ]; then
      echo "  - $_d"
    fi
  done
fi

echo "▶ 启动开发环境"
exec pnpm run tauri dev
