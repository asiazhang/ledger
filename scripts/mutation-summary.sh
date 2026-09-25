#!/bin/sh
# mutation-summary.sh —— 把 cargo-mutants 的 mutants.out 结果目录转成 GitHub
# Job Summary 的 Markdown 片段（spec #1816 / ADR-0137 每日变异测试）。
#
# 观测期不判定（ADR-0137）：本脚本只做计数与清单展示，不做任何阈值判定——
# 存活清单是「看报告优化」的输入，不是门禁输出。
#
# 用法：mutation-summary.sh <mutants.out 目录>
#   目录由 `cargo mutants --output <dir>` 产出（内含 outcomes.json 等）。
# 输出：Markdown（计数表 + 存活/超时/不可行清单折叠块）写到 stdout，由调用方
# 重定向进 $GITHUB_STEP_SUMMARY。
#
# 解析口径（真源 = cargo-mutants outcomes.json 顶层字段）：
#   total_mutants / missed / caught / timeout / unviable —— 工具自身聚合计数；
#   清单行取 missed.txt / timeout.txt / unviable.txt（工具落盘的变异名清单）。
# outcomes.json 缺失或字段不全即报错退出（fail loud，CI 步骤变红）。
set -eu

if [ "$#" -ne 1 ]; then
  echo "用法：mutation-summary.sh <mutants.out 目录>" >&2
  exit 2
fi
DIR=$1

if [ ! -f "$DIR/outcomes.json" ]; then
  echo "变异报告：✗ $DIR/outcomes.json 不存在（cargo-mutants 未完成或目录传错）" >&2
  exit 1
fi

# jq 取聚合字段；字段缺失 = 报告格式漂移，jq -e 空输出即非零退出。
get() {
  jq -er "$1" "$DIR/outcomes.json"
}

total=$(get '.total_mutants')
missed=$(get '.missed')
caught=$(get '.caught')
timeout=$(get '.timeout')
unviable=$(get '.unviable')

echo "### 计数（观测期，无门禁阈值）"
echo
echo "| 变异体 | 被击杀 | 存活 | 超时 | 不可行 | 合计 |"
echo "|---|---|---|---|---|---|"
echo "| 数量 | $caught | $missed | $timeout | $unviable | $total |"
echo

# 存活清单是报告主体（看报告优化的输入）；有则折叠展示，无则一句话说明。
if [ -s "$DIR/missed.txt" ]; then
  missed_n=$(wc -l < "$DIR/missed.txt" | tr -d ' ')
  echo "<details>"
  echo "<summary>存活变异体（missed）：$missed_n 条</summary>"
  echo
  echo '```'
  cat "$DIR/missed.txt"
  echo '```'
  echo "</details>"
  echo
else
  echo "> ✅ 无存活变异体（$caught/$((caught + missed)) 击杀；超时与不可行不计入）"
  echo
fi

if [ -s "$DIR/timeout.txt" ]; then
  timeout_n=$(wc -l < "$DIR/timeout.txt" | tr -d ' ')
  echo "<details>"
  echo "<summary>超时变异体（timeout）：$timeout_n 条 —— 多为变异导致死循环，按「被击杀」口径之外单列，需人工确认测试预算</summary>"
  echo
  echo '```'
  cat "$DIR/timeout.txt"
  echo '```'
  echo "</details>"
  echo
fi

if [ -s "$DIR/unviable.txt" ]; then
  unviable_n=$(wc -l < "$DIR/unviable.txt" | tr -d ' ')
  echo "<details>"
  echo "<summary>不可行变异体（unviable）：$unviable_n 条 —— 编译不过的变异，不计入分母</summary>"
  echo
  echo '```'
  cat "$DIR/unviable.txt"
  echo '```'
  echo "</details>"
  echo
fi
