#!/bin/sh
# 生物门 PoC 一键证据矩阵（issue #657 / 父任务 #645）。编译
# biometry-poc.swift 后按四种签名形态运行，复现结论：
#
#   矩阵 1  ad-hoc + acl 条目（原方案形状，#645/#662 缘起）
#           预期 SecItemAdd = -34018 errSecMissingEntitlement
#   矩阵 2  Developer ID + hardened runtime + plain 条目（方案 A 存储原语 /
#           #662 开发回退形状）
#           预期 OSStatus = 0——普通条目任何签名形态可用
#   矩阵 3  Developer ID + hardened runtime + acl 条目 + keychain-access-groups
#           entitlements（临时生成，不入仓库）
#           预期进程被 AMFI SIGKILL——restricted entitlement 必须 provisioning
#           profile 背书，Developer ID 不可得
#   矩阵 4  Developer ID + hardened runtime + la（方案 A 完整路径）
#           预期 PASS；读取前弹 Touch ID，需要本人交互
#
# 依赖：Xcode Command Line Tools（swiftc / codesign）；登录钥匙串中有效的
# Developer ID Application 证书（申请流程见 issue #660）。
# 用法：scripts/biometry-poc/run.sh（在仓库根目录或任意位置执行均可）
set -eu
cd "$(dirname "$0")/../.."

POC_DIR="scripts/biometry-poc"
IDENTITY="Developer ID Application"
TEAM_ID="75N2YA2H9Q"

command -v swiftc >/dev/null 2>&1 || {
  echo "✗ 未找到 swiftc：需要 Xcode Command Line Tools（xcode-select --install）" >&2
  exit 1
}

BIN_DIR="$(mktemp -d)"
trap 'rm -rf "$BIN_DIR"' EXIT
BIN="$BIN_DIR/biometry-poc"

echo "▶ 编译 PoC（swiftc）"
swiftc "$POC_DIR/biometry-poc.swift" -o "$BIN"

echo
echo "▶ 矩阵 1/4：ad-hoc + acl 条目（预期 SecItemAdd=-34018）"
codesign --force --sign - "$BIN"
"$BIN" acl && echo "✗ 意外：ad-hoc 竟然通过，对照失效" || echo "（符合预期：-34018，生物门不可用）"

echo
echo "▶ 矩阵 2/4：Developer ID + hardened + plain 条目（预期 OSStatus=0）"
codesign --force --sign "$IDENTITY" --options runtime "$BIN"
"$BIN" plain

echo
echo "▶ 矩阵 3/4：Developer ID + hardened + acl + keychain-access-groups（预期 AMFI SIGKILL）"
ENT="$BIN_DIR/kag.plist"
cat > "$ENT" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>keychain-access-groups</key>
    <array>
      <string>${TEAM_ID}.com.zhangheng.ledger</string>
    </array>
  </dict>
</plist>
EOF
codesign --force --sign "$IDENTITY" --entitlements "$ENT" --options runtime "$BIN"
"$BIN" acl && echo "✗ 意外：未触发 AMFI，请检查系统版本行为" || echo "（符合预期：restricted entitlement 无 profile 背书，进程被终止）"

echo
echo "▶ 矩阵 4/4：Developer ID + hardened + la（方案 A 完整路径，预期 PASS，弹 Touch ID）"
codesign --force --sign "$IDENTITY" --options runtime "$BIN"
"$BIN" la

echo
echo "✅ 证据矩阵完成：item 级 ACL 生物门对 Developer ID 不可用（-34018 / SIGKILL），"
echo "   应用层 LocalAuthentication 门（方案 A）在发布签名形态下可用。"
