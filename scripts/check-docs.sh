#!/bin/sh
# 文档一致性检查：地图完整性 / 术语唯一 / 导航一致 / ADR 编号唯一
#
# 四项校验：
#   ① CONTEXT-MAP.md 与 docs/contexts/CONTEXT-*.md 一一对应（地图断链、未挂地图的孤儿文件均报错）
#   ② 术语全库唯一：分域词汇表条目标题（^## ）按括号前主干归一后比对，重复即报错
#   ③ 导航一致：AGENTS.md 与 CONTEXT-MAP.md 引用的仓库内文件/目录必须存在（导航指向已删除文件即报错）
#   ④ ADR 编号唯一：docs/adr/ 下文件名前缀编号不得重复（同号不同义会误导读者与 AI 助手）
#
# 任一校验失败即非零退出；错误信息为中文并定位到文件与术语。
# 已挂入 scripts/check.sh 质量门槛序列，也可独立运行：scripts/check-docs.sh
set -eu
cd "$(dirname "$0")/.."

MAP=CONTEXT-MAP.md
CTX_DIR=docs/contexts
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

err() {
  printf '文档一致性：✗ %s\n' "$1" >>"$tmp"
}

echo "▶ 文档一致性检查 (scripts/check-docs.sh)"

# ── ① 地图 ↔ 分域文件一一对应 ────────────────────────────────────────────
if [ ! -f "$MAP" ]; then
  err "地图断链：地图文件 $MAP 不存在"
  map_refs=""
else
  map_refs=$(grep -oE 'docs/contexts/CONTEXT-[A-Za-z0-9_-]+\.md' "$MAP" | sort -u || true)
  # 断链：地图引用了不存在的分域文件（$map_refs 刻意不加引号以按空白分词，每行一个路径）
  for ref in $map_refs; do
    [ -f "$ref" ] || err "地图断链：$MAP 引用的分域文件不存在：$ref"
  done
fi
# 孤儿：分域文件存在但未挂到地图
for f in "$CTX_DIR"/CONTEXT-*.md; do
  [ -e "$f" ] || continue
  case "$map_refs" in
    *"$f"*) ;;
    *) err "孤儿词汇表文件：$f 未在 $MAP 挂载" ;;
  esac
done

# ── ② 术语全库唯一（条目标题按括号前主干归一后比对） ──────────────────────
# 重复清单先落 dup_output 变量，再经临时文件在主 shell 报错——避免管道子 shell 丢计数。
dup_output=$(awk '
  FNR == 1 { file = FILENAME }
  /^## / {
    title = substr($0, 4)
    p = index(title, "（"); q = index(title, "(")
    if (p == 0 || (q > 0 && q < p)) p = q
    if (p > 0) title = substr(title, 1, p - 1)
    gsub(/^[ \t]+/, "", title); gsub(/[ \t]+$/, "", title)
    if (title == "") next
    if (title in seen) {
      print title "\t" seen[title] "\t" file
    } else {
      seen[title] = file
    }
  }
' "$CTX_DIR"/CONTEXT-*.md || true)
if [ -n "$dup_output" ]; then
  printf '%s\n' "$dup_output" | while IFS='	' read -r term f1 f2; do
    err "术语重复：「$term」同时定义于 $f1 与 $f2（按括号前主干归一比对，重复处：$f2）"
  done
fi

# ── ③ 导航一致（引用的仓库内文件/目录必须存在） ──────────────────────────
# 从导航入口文档提取仓库内路径：反引号行内代码与 Markdown 链接目标。
# 只认形如仓库存放路径的候选（字母数字 _ . / -）：含 / 的路径，或无路径分隔但以 .md 结尾的
# 根目录文档导航（覆盖校验③的旧根 CONTEXT.md 场景）；跳过命令、符号引用（xxx::yyy、列名）与 URL。
check_nav() {
  nav_file=$1
  if [ ! -f "$nav_file" ]; then
    err "导航断链：导航入口文件 $nav_file 不存在"
    return
  fi
  {
    grep -oE '`[^`]+`' "$nav_file" | sed 's/^`//; s/`$//' || true
    grep -oE '\]\(([^)h][^)]*)\)' "$nav_file" | sed 's/^](//; s/)$//' || true
  } | grep -E '^[A-Za-z0-9_][A-Za-z0-9_./-]*$' | grep -E '/|\.md$' | sort -u |
  while IFS= read -r path; do
    case "$path" in
      */) [ -d "$path" ] || err "导航断链：$nav_file 引用的目录不存在：$path" ;;
      *)  [ -f "$path" ] || err "导航断链：$nav_file 引用的文件不存在：$path" ;;
    esac
  done
}
check_nav AGENTS.md
check_nav "$MAP"

# ── ④ ADR 编号唯一（文件名前缀编号不得重复） ──────────────────────────────────
adr_nums=$(ls docs/adr 2>/dev/null | grep -E '^[0-9]{4}-' | sed 's/^\([0-9]\{4\}\)-.*/\1/' || true)
dup_nums=$(printf '%s\n' "$adr_nums" | sort | uniq -d || true)
for num in $dup_nums; do
  files=$(ls docs/adr | grep -E "^$num-" | sed 's|^|docs/adr/|' | paste -sd ' ' -)
  err "ADR 编号重复：编号 $num 同时被 $files 使用"
done

# ── 结果 ────────────────────────────────────────────────────────────────
if [ -s "$tmp" ]; then
  cat "$tmp"
  echo "❌ 文档一致性检查失败：$(wc -l <"$tmp" | tr -d ' ') 处问题（见上方 ✗ 列表）"
  exit 1
fi
echo "  ✓ 地图完整、术语唯一、导航一致、ADR 编号唯一"
