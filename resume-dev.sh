#!/usr/bin/env bash
# resume-dev.sh —— 恢复开发：仓库专属 rift 持久工作会话
#
# 会话名 = 仓库路径的稳定哈希，同一仓库永远回到同一会话（跨断线、跨重连）。
# 用法：
#   ./resume-dev.sh               进入/恢复仓库会话（有则 attach，无则创建）
#   ./resume-dev.sh <cmd> [args]  在会话里跑一次性命令，不进入交互
#   ./resume-dev.sh list          列出本机所有 rift 会话
#
# 依赖：rift（https://github.com/jrf/rift）
#   macOS: brew install --formula <rift.rb> 或 mise use ubi:jrf/rift
#   Linux: mise use ubi:jrf/rift 或 cargo install --git https://github.com/jrf/rift

set -euo pipefail

# ---------- 可配置 ----------
# 首次创建会话时执行的初始化（比如启动工具、source 环境）。
# 留空则不执行；这里可以改成你想要的，比如: INIT_CMD="pi"
INIT_CMD="pi"
# ---------------------------

# 会话名：仓库绝对路径的确定性哈希（规避特殊字符 + 撞名）
SESSION="dev-$(printf '%s' "$PWD" | cksum | cut -d' ' -f1)"

# list 子命令：列出所有会话
if [[ "${1:-}" == "list" ]]; then
    exec rift list
fi

# 判断会话是否活跃（存在且未结束）。rift list 输出含 ended= 表示已结束。
session_alive() {
    rift list 2>/dev/null | grep -q "^name=${SESSION}	" && \
        ! rift list 2>/dev/null | grep -q "^name=${SESSION}	.*ended="
}

# 清理已结束的残留会话（否则同名新会话创建会被拒绝）
if rift list 2>/dev/null | grep -q "^name=${SESSION}	"; then
    if ! session_alive; then
        rift kill "$SESSION" 2>/dev/null || true
    fi
fi

# 首次创建：detached 创建会话并执行初始化，随后 attach
if ! session_alive && [[ -n "$INIT_CMD" ]]; then
    rift attach -d "$SESSION" bash -lc "$INIT_CMD"
fi

# 有参数：在会话里跑一次性命令；无参数：进入交互（attach-or-create）
if [[ $# -gt 0 ]]; then
    exec rift run "$SESSION" "$@"
else
    exec rift attach "$SESSION"
fi
