// 生物门 PoC（issue #657 / 父任务 #645）：在一台机器上复现「签名形态 × 钥匙串
// 生物门」的完整证据矩阵。编译与签名矩阵由 scripts/biometry-poc/run.sh 一键执行。
//
// 用法：biometry-poc <acl|plain|la>
//
//   acl   条目挂 kSecAttrAccessControl(BIOMETRY_CURRENT_SET) + 显式
//         kSecAttrAccessGroup——原方案（ADR-0075 决策 3 + item 级 ACL 门）形状。
//         实测结论（macOS 26.6.2 arm64，Individual + Developer ID Application
//         证书，链 Developer ID Certification Authority G2）：
//         · 签名不带 keychain-access-groups entitlement → add 报 -34018
//           errSecMissingEntitlement（传统/数据保护钥匙串、裸二进制/.app bundle、
//           终端/LaunchServices 启动、有无 hardened runtime 均同）；
//         · 挂 keychain-access-groups（或 application-identifier + 两者组合）
//           entitlement → AMFI 校验失败，进程启动即被 SIGKILL——restricted
//           entitlement 必须有 provisioning profile 背书，Developer ID 不可得；
//         · 只挂 application-identifier → 进程能跑，但钥匙串不认（仍 -34018）。
//         即：item 级 ACL 门对 Developer ID 分发形态不可用。
//
//   plain 条目不带 ACL、不带 accessGroup——#662 开发回退同形。任何签名形态
//         （ad-hoc / Developer ID）均可写读，是方案 A 的存储原语。
//
//   la    plain 写入 + 读取前 LAContext.evaluatePolicy 应用层生物门——方案 A
//         完整路径，Developer ID + hardened runtime 下 PASS（实测 Touch ID
//         正常弹出、验证通过后读出）。
//
// service 与产品条目（com.zhangheng.ledger，见
// src-tauri/src/db/passphrase_cache.rs）刻意隔离，本 PoC 不触碰真实主口令缓存。

import Foundation
import LocalAuthentication
import Security

let mode = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ""

guard ["acl", "plain", "la"].contains(mode) else {
  print("用法: biometry-poc <acl|plain|la>")
  exit(2)
}

let service = "com.zhangheng.ledger.biometry-poc"
let account = "master-passphrase-poc"
// 仅 acl 模式使用；须与 keychain-access-groups entitlement 一致（Team ID 前缀）
let accessGroup = "75N2YA2H9Q.com.zhangheng.ledger"

func describe(_ status: OSStatus) -> String {
  switch status {
  case errSecSuccess: return "errSecSuccess"
  case errSecItemNotFound: return "errSecItemNotFound"
  case errSecUserCanceled: return "errSecUserCanceled"
  case errSecAuthFailed: return "errSecAuthFailed"
  case -34018: return "errSecMissingEntitlement"
  default: return "unknown"
  }
}

let baseQuery: [String: Any] = [
  kSecClass as String: kSecClassGenericPassword,
  kSecAttrService as String: service,
  kSecAttrAccount as String: account,
]

let preDeleteStatus = SecItemDelete(baseQuery as CFDictionary)
print("[SecItemDelete(预清理)] OSStatus=\(preDeleteStatus) (\(describe(preDeleteStatus)))")

// 1) 写入
var addQuery = baseQuery
addQuery[kSecValueData as String] = "biometry-poc-secret".data(using: .utf8)!
if mode == "acl" {
  var cfError: Unmanaged<CFError>?
  guard let accessControl = SecAccessControlCreateWithFlags(
    kCFAllocatorDefault,
    kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly,
    .biometryCurrentSet,
    &cfError
  ) else {
    print("[SecAccessControlCreateWithFlags] 失败：\(cfError?.takeRetainedValue().localizedDescription ?? "unknown")")
    exit(2)
  }
  addQuery[kSecAttrAccessControl as String] = accessControl
  addQuery[kSecAttrAccessGroup as String] = accessGroup
}
let addStatus = SecItemAdd(addQuery as CFDictionary, nil)
let tag = mode == "acl" ? "[SecItemAdd(ACL 门)]  " : "[SecItemAdd(无 ACL)]  "
print("\(tag) OSStatus=\(addStatus) (\(describe(addStatus)))")

// 2) 读取（la 模式先过应用层生物门）
var readStatus: OSStatus = errSecUserCanceled
var readBytes = 0
if mode != "la" {
  readStatus = readPlain(baseQuery, &readBytes)
  print("[SecItemCopyMatching]  OSStatus=\(readStatus) (\(describe(readStatus)))\(readStatus == errSecSuccess ? " bytes=\(readBytes)" : "")")
} else {
  let context = LAContext()
  var policyError: NSError?
  let canEvaluate = context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &policyError)
  print("[LAContext] 生物认证可用: \(canEvaluate)\(canEvaluate ? "" : "（\(policyError?.localizedDescription ?? "")）→ 产品回退手输")")
  if canEvaluate {
    var authOK = false
    let semaphore = DispatchSemaphore(value: 0)
    context.evaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, localizedReason: "解锁 OpenLedger PoC") { success, authError in
      authOK = success
      if let authError { print("[LAContext] 验证失败: \(authError.localizedDescription)") }
      semaphore.signal()
    }
    semaphore.wait()
    print("[LAContext] Touch ID 验证: \(authOK ? "通过" : "未通过")")
    if authOK {
      readStatus = readPlain(baseQuery, &readBytes)
      print("[SecItemCopyMatching]  OSStatus=\(readStatus) (\(describe(readStatus)))\(readStatus == errSecSuccess ? " bytes=\(readBytes)" : "")")
    }
  }
}

// 3) 清理
let deleteStatus = SecItemDelete(baseQuery as CFDictionary)
print("[SecItemDelete(清理)]  OSStatus=\(deleteStatus) (\(describe(deleteStatus)))")

let ok = addStatus == errSecSuccess && readStatus == errSecSuccess && deleteStatus == errSecSuccess
print(ok ? "RESULT: PASS" : "RESULT: FAIL")
exit(ok ? 0 : 1)

func readPlain(_ query: [String: Any], _ bytes: inout Int) -> OSStatus {
  var result: AnyObject?
  let status = SecItemCopyMatching(
    query.merging([kSecReturnData as String: true, kSecMatchLimit as String: kSecMatchLimitOne]) { _, new in new } as CFDictionary,
    &result
  )
  if status == errSecSuccess, let data = result as? Data { bytes = data.count }
  return status
}
