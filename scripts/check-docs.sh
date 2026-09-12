#!/bin/sh
# 文档一致性检查：地图完整性 / 术语唯一 / 导航一致 / ADR 编号唯一 / ADR 索引完整 /
# ADR 索引反查 / 代码坐标 / 快照分组组数一致
#
# 八项校验：
#   ① CONTEXT-MAP.md 与 docs/contexts/CONTEXT-*.md 一一对应（地图断链、未挂地图的孤儿文件均报错）
#   ② 术语全库唯一：分域词汇表条目标题（^## ）按括号前主干归一后比对，重复即报错
#   ③ 导航一致：AGENTS.md 与 CONTEXT-MAP.md 引用的仓库内文件/目录必须存在（导航指向已删除文件即报错）
#   ④ ADR 编号唯一：docs/adr/ 下文件名前缀编号不得重复（同号不同义会误导读者与 AI 助手）
#   ⑤ 代码坐标：分域词汇表与模型文档不得出现实现坐标（src-tauri/src/、src/ 路径式引用
#      及 .rs/.ts/.vue 文件名）——「代码可查事实不进文档」三层标尺的守门（标尺见
#      CONTEXT-MAP「结构约定」；扫描范围不含 ADR / agents / api 文档）
#   ⑥ 快照分组组数一致：CONTEXT-testing.md 快照分组词条与 ADR-0086 决策 6 记载的
#      组数，必须与 e2e world 实际快照分组数全等——「新快照必须归入既有分组」
#      纪律的守门，防止新组绕过规格修订静默入场（issue #955）
#   ⑦ ADR 索引完整：docs/adr/ 下每个现行 ADR 文件编号都必须在 docs/adr/README.md
#      有一条索引条目行——「新 ADR 落盘须同步入索引」纪律的守门，补④只查重复
#      不查遗漏的方向；判据取条目行而非「编号出现」，他行交叉引用不算（issue #1027）
#   ⑧ ADR 索引反查：README 现行条目的编号必须有对应文件——⑦ 的反方向，防「ADR 已
#      删除却仍留在现行索引」。tombstone 段用 `- **编号**` 形态，与现行条目天然区分
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

# ── ⑤ 代码坐标扫描（分域词汇表与模型文档禁止实现坐标） ──────────────────
# 「代码可查事实不进文档」三层标尺（甲删乙留丙留，详见 CONTEXT-MAP「结构约定」）：
# 甲类实现坐标——src-tauri/src/、src/ 路径式引用与 .rs/.ts/.vue 文件名——出现即失败；
# 术语专名（表名/视图名/信号名/接缝名）不在扫描范围。白名单刻意留空：清理完成后应零豁免通过。
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

# ── ⑥ 快照分组组数一致（testing 词条 / ADR-0086 决策 6 / e2e world 实际分组） ─
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

# ── ⑦ ADR 索引完整（docs/adr/ 下每个现行 ADR 文件须有 README 索引条目行） ──────
# 与④编号唯一对偶的遗漏方向：新 ADR 落盘却忘记入 docs/adr/README.md 时，读者与 AI
# 从索引找决策会漏读。判据取「README 有以 `- <编号> ` 开头的条目行」——他行的正文
# 交叉引用不算入索引（0013/0059 正因被他行旁引而骗过「编号出现即通过」的弱判据，
# issue #1027 实测）。已删除 ADR 无文件，不在扫描范围。
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

# ── ⑧ ADR 索引反查（README 现行条目编号须有对应文件） ──────────────────────
# ⑦ 只查「文件 → 索引」；删除 ADR 却忘记同步索引时，读者会照索引找一个不存在的
# 文件（ADR-0055 下线时的人工核对项）。tombstone 段用 `- **编号**`，不落入本判据。
if [ -f "$adr_readme" ]; then
  index_nums=$(grep -oE '^- [0-9]{4} ' "$adr_readme" | grep -oE '[0-9]{4}' | sort -u || true)
  for num in $index_nums; do
    ls docs/adr/"$num"-*.md >/dev/null 2>&1 || \
      err "ADR 索引反查：$adr_readme 的现行条目 $num 无对应文件（删除 ADR 须同时移入 tombstone 段）"
  done
fi

# ── 结果 ────────────────────────────────────────────────────────────────
if [ -s "$tmp" ]; then
  cat "$tmp"
  echo "❌ 文档一致性检查失败：$(wc -l <"$tmp" | tr -d ' ') 处问题（见上方 ✗ 列表）"
  exit 1
fi
echo "  ✓ 地图完整、术语唯一、导航一致、ADR 编号唯一、词汇表与模型文档坐标清零、快照分组组数一致、ADR 索引完整且可反查"
