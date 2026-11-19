#!/bin/sh
# 守门脚本 TS 类型检查的宿主无关执行器（issue #734/#740 门保留，宿主迁移
# #2171/#2172）：vue-tsc 随 Web 参照前端退役，其存在的唯一理由（vue 单文件
# 组件类型面）不复存在；typescript 7 = Go 端口原生 tsc，直接类型检查 TS 工程。
# 工具链形态（用户裁决 2026-11-19，方案 C——仓库零 manifest、运行时拉取、
# 不考虑离线环境）：三包版本钉死在本脚本（单一事实源），经 bun 装入用户缓存
# 目录后取 .bin/tsc 执行。不用 bunx：实测 bunx 多 --package 不共装依赖，
# tsc 解析不到 bun-types/@types/node。缓存目录按钉死版本串键控——版本升级
# 改本脚本即自动换目录重建，陈缓存不干扰。
# 类型库解析：tsc 只从工程目录向上找 node_modules，缓存目录不在查找链上；
# 原生 tsc 7 不消费 NODE_PATH（实测），经仓库根 node_modules 符号链接补
# （.gitignore 既有豁免，工作树零 manifest）。
#
# 用法：scripts/check-scripts-types.sh（check.sh 与 CI gates job 同挂，双宿主
# 共用本脚本即版本钉死无双源漂移）。
set -eu
cd "$(dirname "$0")/.."

PINS="typescript@7.0.2 bun-types@1.4.0 @types/node@22.20.1"
# 版本串作目录名：@ → -、/ → -、空格 → _（@types/node 的斜杠不进目录名）
TOOLCHAIN_DIR="${LEDGER_TSC_HOME:-${XDG_CACHE_HOME:-$HOME/.cache}/ledger}/tsc/$(printf '%s' "$PINS" | tr ' @/' '_-_')"

if [ ! -x "$TOOLCHAIN_DIR/node_modules/.bin/tsc" ]; then
  mkdir -p "$TOOLCHAIN_DIR"
  ( cd "$TOOLCHAIN_DIR" && bun init -y >/dev/null && bun add --silent $PINS )
fi

# 类型库解析：tsc 只从工程目录向上找 node_modules（原生 tsc 7 不消费 NODE_PATH，
# 实测）；仓库根已有 .gitignore 豁免 node_modules，在此放一个指向工具链
# node_modules 的符号链接（缺或指错即重建），类型库与 tsc 二进制同源单点。
if [ "$(readlink node_modules 2>/dev/null)" != "$TOOLCHAIN_DIR/node_modules" ]; then
  rm -rf node_modules
  ln -s "$TOOLCHAIN_DIR/node_modules" node_modules
fi

"$TOOLCHAIN_DIR/node_modules/.bin/tsc" --noEmit -p tsconfig.scripts.json
