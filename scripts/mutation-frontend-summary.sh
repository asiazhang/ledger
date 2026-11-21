#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
  echo "用法：mutation-frontend-summary.sh <mutation.json 路径>" >&2
  exit 2
fi
REPORT=$1

if [ ! -f "$REPORT" ]; then
  echo "变异报告：✗ $REPORT 不存在（Stryker 未完成或路径传错）" >&2
  exit 1
fi

STATUS_JSON=$(jq -ce '
  .files
  | [to_entries[].value.mutants[] | .status]
  | {
      total: length,
      killed: map(select(. == "Killed")) | length,
      survived: map(select(. == "Survived")) | length,
      nocoverage: map(select(. == "NoCoverage")) | length,
      timeout: map(select(. == "Timeout")) | length,
      compile_error: map(select(. == "CompileError")) | length,
      runtime_error: map(select(. == "RuntimeError")) | length
    }
' "$REPORT")

total=$(jq -r '.total' <<<"$STATUS_JSON")
killed=$(jq -r '.killed' <<<"$STATUS_JSON")
survived=$(jq -r '.survived' <<<"$STATUS_JSON")
nocoverage=$(jq -r '.nocoverage' <<<"$STATUS_JSON")
timeout=$(jq -r '.timeout' <<<"$STATUS_JSON")
compile_error=$(jq -r '.compile_error' <<<"$STATUS_JSON")
runtime_error=$(jq -r '.runtime_error' <<<"$STATUS_JSON")

denominator=$((killed + survived + nocoverage + timeout))
if [ "$denominator" -gt 0 ]; then
  score_line="变异分数：$(( (killed + timeout) * 100 / denominator ))%（观测期，无门禁阈值）"
else
  score_line="变异分数：无（分母为 0）"
fi

echo "### 计数（观测期，无门禁阈值）"
echo
echo "| 被击杀 | 存活 | 无覆盖 | 超时 | 编译错 | 运行时错 | 合计 |"
echo "|---|---|---|---|---|---|---|"
echo "| $killed | $survived | $nocoverage | $timeout | $compile_error | $runtime_error | $total |"
echo
echo "> $score_line"
echo

list_rows() {
  jq -r --arg want "$1" '
    .files | to_entries[] as $f
    | $f.value.mutants[]
    | select(.status == $want)
    | [($f.key + ":" + (.location.start.line | tostring)), .mutatorName, .replacement]
    | map(gsub("[|]"; "&#124;"))
    | "| " + .[0] + " | " + .[1] + " | `" + .[2] + "` |"
  ' "$REPORT"
}

for entry in "Survived 存活变异体（Survived）" "NoCoverage 无覆盖变异体（NoCoverage，无测试命中其覆盖面）"; do
  want=${entry%% *}
  label=${entry#* }
  ROWS=$(list_rows "$want")
  if [ -n "$ROWS" ]; then
    n=$(printf '%s\n' "$ROWS" | wc -l | tr -d ' ')
    echo "<details>"
    echo "<summary>${label}：$n 条</summary>"
    echo
    echo "| 位置 | 变异器 | 替换 |"
    echo "|---|---|---|"
    printf '%s\n' "$ROWS"
    echo "</details>"
    echo
  fi
done

if [ "$survived" -eq 0 ] && [ "$nocoverage" -eq 0 ]; then
  echo "> ✅ 无存活且无无覆盖变异体（$killed/$denominator 击杀；编译错与运行时错不计入）"
  echo
fi
