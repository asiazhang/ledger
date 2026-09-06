#!/bin/sh
# perf-summary.sh —— 把 ledger-perf bench / bench-import 的人读 stdout 报告
# 转成 GitHub Job Summary 的 Markdown 片段（issue #462；bench-import 接入
# issue #532）。
# 阈值判定由工具自身承担（bench 的 --max-p95-ms，issue #493 / ADR-0068，
# exit code 表达结果；bench-import 无门禁、纯观测）；本脚本仍只做格式
# 转换，不做任何阈值判定。
#
# 用法：perf-summary.sh <report.txt> [原始报告折叠标题]
#   <report.txt> 是 `ledger-perf bench` 或 `ledger-perf bench-import` 的
#   stdout 报告文件（CI 里由 `… | tee …-report.txt` 落盘）。两者报告行
#   格式同构，本脚本通用。
#   [原始报告折叠标题] 缺省 "ledger-perf bench stdout"。
# 输出：Markdown（基准表格 + 折叠的原始报告）写到 stdout，由调用方重定向进
# $GITHUB_STEP_SUMMARY。
#
# 解析口径（真源 = bench.rs / bench_import.rs 的 print_report 行格式，两者
# 同构）：
#   {基准名}{连续 2+ 空格 padding}{min}{avg}{p95}{连续 2 空格}{规模备注}
# 把连续 2+ 空格视作列分隔后，第 2 列为纯数字的行才是数据行——报告头、
# 表头、尾注等说明行天然被过滤；一行都解析不出即视为报告格式漂移，报错退出
# （fail loud，CI 步骤变红）。
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
