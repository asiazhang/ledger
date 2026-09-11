#!/bin/sh
# 运行 Rust 单元测试
# 显式 --workspace：非虚拟 workspace 下 cargo 默认只作用于根包，缺范围参数会
# 静默漏跑成员 crate（spec #1086 / issue #1087）。
set -eu
cd "$(dirname "$0")/.."
( cd src-tauri && cargo test --workspace )
