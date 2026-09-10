#!/bin/sh
# 删除数据库后重启开发环境
# 用于调试 init_db 初始化逻辑、种子数据、Schema 迁移
#
# 重置范围（issue #993，解析语义与 data_location::configured_intent 一致）：
# 1. 默认应用数据目录（下方 DEFAULT_DIR）；
# 2. 默认目录下 data_location.json 指向的自定义库目录：
#    - 旧格式指针 {"data_dir": …} → 取 data_dir；
#    - 新格式注册表 {"version": 1, "books": [{id, name, dir}…], "active": …}
#      → 取活动账本（active 指向条目）的 dir；
#    - 文件缺失、无法解析或校验不通过（版本未知、清单/活动指针缺失或悬空、
#      条目字段缺失、id/目录重复登记）一律视同未配置，仅重置默认目录
#      （与 data_location::boot 的损坏回退原则一致）；
#    - 解析依赖 jq，未安装时跳过自定义目录并提示。
# 3. 注册表中非活动的其他账本目录：仅打印路径提示，不自动删除。
#
# 沿用「文件存在才删」语义：各处只删固定名 ledger.db，不动其他文件；
# 删除与否均打印实际路径。
set -eu
cd "$(dirname "$0")/.."

IDENTIFIER="com.zhangheng.ledger"
case "$(uname -s)" in
  Darwin) DEFAULT_DIR="$HOME/Library/Application Support/$IDENTIFIER" ;;
  Linux)  DEFAULT_DIR="$HOME/.local/share/$IDENTIFIER" ;;
  *) echo "❌ 不支持的系统: $(uname -s)"; exit 1 ;;
esac

REGISTRY_FILE="$DEFAULT_DIR/data_location.json"

# 与 src-tauri/src/db/book_registry.rs 的读取校验同构。输出三种形态之一：
#   {"kind":"corrupt"}（视同未配置）
#   {"kind":"pointer","dir":…}（旧格式指针）
#   {"kind":"registry","active_dir":…,"other_dirs":[…]}（新格式注册表）
# 非法 JSON / 字段类型不符等运行期错误由外层兜底为 corrupt。
# shellcheck disable=SC2016  # jq 过滤器内 $books/$active 是 jq 变量，不参与 shell 展开
JQ_RESOLVE='
  if has("version") and (.version != 1) then
    {"kind":"corrupt"}
  elif has("books") and has("active") then
    ([.books[] | {
        id:   (.id   // "" | gsub("^\\s+|\\s+$"; "")),
        name: (.name // "" | gsub("^\\s+|\\s+$"; "")),
        dir:  (.dir  // "" | gsub("^\\s+|\\s+$"; ""))}]) as $books
    | (.active | gsub("^\\s+|\\s+$"; "")) as $active
    | if ($books | length) == 0
         or ($books | any(.id == "" or .name == "" or .dir == ""))
         or ($books | map(.id) | unique | length) != ($books | length)
         or ($books | map(.dir) | unique | length) != ($books | length)
         or ($books | map(select(.id == $active)) | length) != 1
      then {"kind":"corrupt"}
      else
        {"kind":"registry",
         "active_dir": ($books | map(select(.id == $active)) | .[0] | .dir),
         "other_dirs": ($books | map(select(.id != $active) | .dir))}
      end
  elif has("books") or has("active") then
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
