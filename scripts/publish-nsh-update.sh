#!/usr/bin/env bash
# nsh package 发布接缝（issue #2134 / ADR-0145 修订注记 2026-11-15）：打包 +
# 更新清单生成 + 签名一次完成，打印上传清单——发版形态 = nsh package（词汇表
# 「更新工件」：目录包既是安装形态也是更新形态）。
#
# 前置（一次性人工步骤，见 src-app/packaging/README.md）：
#   - `nsh keygen keys/update` 产 Ed25519 密钥对；`.pub` 内容已烧入
#     src-app/nsh.json 的 updater.publicKey（占位值必须先替换）。
#   - `.secret` 只在本发布机使用，不进仓、不入 CI。
#
# 用法（在仓库根执行）：
#   scripts/publish-nsh-update.sh <发布说明.md> [key 秘钥路径]
#   例：scripts/publish-nsh-update.sh /tmp/changes-0.11.3.md keys/update.secret
#
# 产物（.nsh/out/）：
#   Ledger/                                  ← 目录包（安装形态）
#   Ledger-<version>-linux-x86_64.tar.gz     ← 更新工件（更新形态，平台命名）
#   latest.json + latest.json.sig            ← 更新清单 + 分离签名
#
# 三件套（archive + latest.json + latest.json.sig）作为 GitHub Release assets
# 上传（updater.github 源 = asiazhang/ledger）；draft / prerelease 不被更新器
# 提供——发布前把 Release 标为正式版。`nsh.json` version 必须比上一发布增长
# （更新器版本只升不降），三处版本一致性由 scripts/check_versions.py 守门。
set -eu
cd "$(dirname "$0")/.."

NOTES_FILE="${1:?用法: scripts/publish-nsh-update.sh <发布说明.md> [key 秘钥路径]}"
KEY="${2:-keys/update.secret}"

if ! command -v nsh >/dev/null 2>&1; then
  echo "✗ 未检测到 nsh CLI（0.5.1+）：更新打包与清单签名依赖 nsh package / nsh update-manifest" >&2
  exit 1
fi
if [ ! -f "$NOTES_FILE" ]; then
  echo "✗ 发布说明文件不存在: $NOTES_FILE" >&2
  exit 1
fi
if [ ! -f "$KEY" ]; then
  echo "✗ 签名私钥不存在: $KEY（一次性人工步骤见 src-app/packaging/README.md）" >&2
  exit 1
fi
VERSION=$(python3 -c 'import json; print(json.load(open("src-app/nsh.json", encoding="utf-8"))["version"])')
echo "▶ 打包 nsh package（version=$VERSION，Linux release 构建）"
( cd src-app && nsh package --target linux )

ARCHIVE="src-app/.nsh/out/Ledger-${VERSION}-linux-x86_64.tar.gz"
if [ ! -f "$ARCHIVE" ]; then
  echo "✗ 未找到平台命名更新工件: $ARCHIVE（updater 配置在场时 nsh package 才产平台命名 archive）" >&2
  exit 1
fi

echo "▶ 生成更新清单 + 分离签名（Ed25519）"
( cd src-app/.nsh/out && nsh update-manifest "Ledger-${VERSION}-linux-x86_64.tar.gz" \
  --entry ../.. --key "$KEY" --notes "$NOTES_FILE" )

echo ""
echo "▶ 上传清单（三件套作为 GitHub Release assets 上传，Release 须为正式版）："
echo "  src-app/.nsh/out/Ledger-${VERSION}-linux-x86_64.tar.gz"
echo "  src-app/.nsh/out/latest.json"
echo "  src-app/.nsh/out/latest.json.sig"
echo ""
echo "端到端人工验收清单（首个带更新器的版本执行，见 issue #2134）："
echo "  1. 安装 .nsh/out/Ledger/（目录包直跑）确认当前版本为 $VERSION 的上一版"
echo "  2. 上传三件套后启动应用 → 设置 → 通用 → 检查更新 → 发现新版本"
echo "  3. 下载 → 确认安装（更新前备份 pre-update.db.zip 在应用数据目录生成）"
echo "  4. 应用自动重启后版本号为 $VERSION，账本数据完好"
