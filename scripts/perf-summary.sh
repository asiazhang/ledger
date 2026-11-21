#!/bin/sh
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "用法：perf-summary.sh <report 文件> [原始报告折叠标题]" >&2
  exit 2
fi
REPORT=$1
LABEL=${2:-ledger-perf bench stdout}
if [ ! -f "$REPORT" ]; then
  echo "perf-summary：报告文件不存在：$REPORT" >&2
  exit 2
fi

TABLE=$(sed -E 's/ {2,}/\t/g' "$REPORT" | awk -F'\t' '
  NF >= 4 && $2 ~ /^[0-9]+\.[0-9]+$/ {
    ctx = $5
    for (i = 6; i <= NF; i++) ctx = ctx " " $i
    gsub(/\|/, "\\|", $1)
    gsub(/\|/, "\\|", ctx)
    printf "| %s | %s | %s | %s | %s |\n", $1, $2, $3, $4, ctx
  }')

if [ -z "$TABLE" ]; then
  echo "perf-summary：报告中未解析出任何基准数据行（print_report 格式漂移？）" >&2
  exit 1
fi

echo "| 基准 | min (ms) | avg (ms) | p95 (ms) | 规模备注 |"
echo "| --- | --- | --- | --- | --- |"
printf '%s\n' "$TABLE"
echo
echo "<details><summary>原始报告（${LABEL}）</summary>"
echo
echo '```text'
cat "$REPORT"
echo '```'
echo '</details>'
