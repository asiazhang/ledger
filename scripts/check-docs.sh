#!/bin/sh
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

if [ ! -f "$MAP" ]; then
  err "地图断链：地图文件 $MAP 不存在"
  map_refs=""
else
  map_refs=$(grep -oE 'docs/contexts/CONTEXT-[A-Za-z0-9_-]+\.md' "$MAP" | sort -u || true)
  for ref in $map_refs; do
    [ -f "$ref" ] || err "地图断链：$MAP 引用的分域文件不存在：$ref"
  done
fi
for f in "$CTX_DIR"/CONTEXT-*.md; do
  [ -e "$f" ] || continue
  case "$map_refs" in
    *"$f"*) ;;
    *) err "孤儿词汇表文件：$f 未在 $MAP 挂载" ;;
  esac
done

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

adr_nums=$(ls docs/adr 2>/dev/null | grep -E '^[0-9]{4}-' | sed 's/^\([0-9]\{4\}\)-.*/\1/' || true)
dup_nums=$(printf '%s\n' "$adr_nums" | sort | uniq -d || true)
for num in $dup_nums; do
  files=$(ls docs/adr | grep -E "^$num-" | sed 's|^|docs/adr/|' | paste -sd ' ' -)
  err "ADR 编号重复：编号 $num 同时被 $files 使用"
done

coord_re='(src-tauri/)?src/[A-Za-z0-9_./-]+|[A-Za-z0-9_-]+\.(rs|ts|vue)\b'
coord_whitelist=''  # 豁免文件清单（空格分隔仓库相对路径），当前留空
for f in $(find "$CTX_DIR" docs/model -type f -name '*.md' 2>/dev/null | sort); do
  case " $coord_whitelist " in
    *" $f "*) continue ;;
  esac
  coord_hits=$(grep -nE "$coord_re" "$f" || true)
  [ -z "$coord_hits" ] && continue
  printf '%s\n' "$coord_hits" | while IFS= read -r hit; do
    lineno=${hit%%:*}
    text=${hit#*:}
    err "代码坐标：$f:$lineno 出现实现坐标（甲类删，标尺见 CONTEXT-MAP 结构约定）：$text"
  done
done

world_file=src-tauri/tests/e2e/world.rs
cn_to_num() {
  case "$1" in
    一) echo 1 ;; 二|两) echo 2 ;; 三) echo 3 ;; 四) echo 4 ;; 五) echo 5 ;;
    六) echo 6 ;; 七) echo 7 ;; 八) echo 8 ;; 九) echo 9 ;; 十) echo 10 ;;
    *) echo 0 ;;
  esac
}
if [ ! -f "$world_file" ]; then
  err "快照分组：world 文件不存在：$world_file"
  world_groups=-1
else
  world_groups=$(grep -cE '^    pub [a-z][a-z_]*: [A-Za-z]+Group,' "$world_file" || true)
fi
testing_entry=docs/contexts/CONTEXT-testing.md
testing_num=$(sed -n '/^## 快照分组/,/^## /p' "$testing_entry" | grep -oE '[一二两三四五六七八九十]+组' | head -n 1 | sed 's/组$//')
if [ -z "$testing_num" ]; then
  err "快照分组：$testing_entry 快照分组词条未找到「N组」计数"
else
  testing_groups=$(cn_to_num "$testing_num")
  [ "$testing_groups" = "$world_groups" ] || \
    err "快照分组：$testing_entry 词条记 $testing_num（$testing_groups 组），world 实际 $world_groups 组，不一致"
fi
adr_group_file=docs/adr/0086-bdd-step-shared-layer-deepening.md
adr_num=$(grep -oE '[一二两三四五六七八九十]+分组' "$adr_group_file" | head -n 1 | sed 's/分组$//; s/组$//')
if [ -z "$adr_num" ]; then
  err "快照分组：$adr_group_file 未找到「N分组」计数"
else
  adr_groups=$(cn_to_num "$adr_num")
  [ "$adr_groups" = "$world_groups" ] || \
    err "快照分组：$adr_group_file 决策记 $adr_num（$adr_groups 组），world 实际 $world_groups 组，不一致"
fi

adr_readme=docs/adr/README.md
if [ ! -f "$adr_readme" ]; then
  err "ADR 索引：$adr_readme 不存在"
else
  for f in docs/adr/[0-9][0-9][0-9][0-9]-*.md; do
    [ -e "$f" ] || continue
    adr_file_num=$(basename "$f" | cut -c1-4)
    grep -qE "^- $adr_file_num " "$adr_readme" || \
      err "ADR 未入索引：$f 的编号 $adr_file_num 在 $adr_readme 无索引条目行（遗漏方向，与④编号唯一对偶）"
  done
fi

if [ -f "$adr_readme" ]; then
  index_nums=$(grep -oE '^- [0-9]{4} ' "$adr_readme" | grep -oE '[0-9]{4}' | sort -u || true)
  for num in $index_nums; do
    ls docs/adr/"$num"-*.md >/dev/null 2>&1 || \
      err "ADR 索引反查：$adr_readme 的现行条目 $num 无对应文件（删除 ADR 须同时移入 tombstone 段）"
  done
fi

model_readme=docs/model/README.md
mig_dir=src-tauri/migrations
if [ ! -f "$model_readme" ]; then
  err "Migration 索引：$model_readme 不存在"
elif [ ! -d "$mig_dir" ]; then
  err "Migration 索引：迁移目录 $mig_dir 不存在"
else
  for f in "$mig_dir"/V[0-9][0-9][0-9]__*.sql; do
    [ -e "$f" ] || continue
    mig_name=$(basename "$f")
    mig_name_re=$(printf '%s' "$mig_name" | sed 's/\./\\./g')
    grep -qE "^\| \`$mig_name_re\`" "$model_readme" || \
      err "Migration 未入索引：$mig_name 在 $model_readme 的 Migration 索引无表行（迁移落盘须同步补索引行）"
  done
  idx_names=$(grep -oE '^\| `V[0-9]{3}__[A-Za-z0-9_-]+\.sql`' "$model_readme" | sed 's/^| `//; s/`$//' | sort -u || true)
  for mig_name in $idx_names; do
    [ -f "$mig_dir/$mig_name" ] || \
      err "Migration 索引反查：$model_readme 索引表行 $mig_name 无对应迁移文件（迁移删除 / 改名须同步索引）"
  done
fi

if [ -s "$tmp" ]; then
  cat "$tmp"
  echo "❌ 文档一致性检查失败：$(wc -l <"$tmp" | tr -d ' ') 处问题（见上方 ✗ 列表）"
  exit 1
fi
echo "  ✓ 地图完整、术语唯一、导航一致、ADR 编号唯一、词汇表与模型文档坐标清零、快照分组组数一致、ADR 索引完整且可反查、Migration 索引双向全等"
