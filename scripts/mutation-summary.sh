#!/bin/sh
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
