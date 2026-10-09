#!/bin/sh
# mutation-frontend-summary.sh —— 把 Stryker 的 JSON 报告转成 GitHub Job Summary
# 的 Markdown 片段（spec #1816 / ADR-0137 决策 3 / 前端接入 #1832）。
#
# 观测期不判定（ADR-0137）：本脚本只做计数与清单展示，不做任何阈值判定——
# 存活清单是「看报告优化」的输入，不是门禁输出。金丝雀验收（已知 killer 变异
# 必须报 Killed）是判定，归 workflow 步骤承担，不在本脚本。
#
# 用法：mutation-frontend-summary.sh <mutation.json 路径>
#   JSON 由 Stryker json reporter 产出（默认 reports/mutation/mutation.json，
#   schema = mutation-testing-elements）。
# 输出：Markdown（计数表 + 存活/无覆盖清单折叠块）写到 stdout，由调用方重定向
# 进 $GITHUB_STEP_SUMMARY。
#
# 解析口径（真源 = mutation-testing-elements schema）：计数从
# .files[].mutants[].status 聚合——Killed / Survived / NoCoverage / Timeout /
# CompileError / RuntimeError / Ignored / Pending。CompileError 与 RuntimeError
# 是类型检查或运行期不过的变异体（等同 cargo-mutants 的 unviable），不计入
# 分数分母。.files 缺失或 mutants 非数组即报错退出（fail loud，CI 步骤变红）。
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

# status 聚合计数；.files 缺失 = 报告格式漂移，jq -e 空输出即非零退出。
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

# 分数口径（mutation-testing-elements）：分母 = killed + survived + nocoverage +
# timeout（CompileError/RuntimeError 不计入）；分母为 0 时分数不显示。
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

# 存活 + 无覆盖清单是报告主体（看报告优化的输入）；有则折叠展示，无则一句话说明。
list_rows() {
  # 每行一个 Markdown 表格行：| 文件:行 | 变异器 | 替换 |（字段内竖线先转义防串列）
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
