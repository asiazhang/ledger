import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { hasCommandLine, repoRoot } from "./has-command-line.test-helper.ts";

// cargo doc 门禁接线测试（issue #1139）：ledger-infra 的 rustdoc 告警（intra-doc
// link 漂移等）以 `RUSTDOCFLAGS='-D warnings' cargo doc -p ledger-infra --no-deps`
// 清零并纳入门禁。本测试是「删除即变红」负向判据的载体（ADR-0087 断言强度）：
// 从 scripts/check.sh 删掉该步骤、或从 CI（build.yml backend-lint job）删掉对应
// step，至少一条断言变红——门禁接线不靠评审记忆守卫。（断言辅助消费
// scripts/has-command-line.test-helper.ts，#1487 上收唯一定义点）

describe("cargo doc 门禁接线（check.sh 质量门槛宿主）", () => {
  it("scripts/check.sh 含 cargo doc -D warnings 步骤（ledger-infra，--no-deps）", () => {
    const checkSh = readFileSync(join(repoRoot(), "scripts", "check.sh"), "utf8");
    expect(hasCommandLine(checkSh, "cargo doc", "ledger-infra", "--no-deps", "-D warnings")).toBe(
      true,
    );
  });

  it("CI backend-lint job 含同款 cargo doc 步骤（删步骤即红）", () => {
    const workflow = readFileSync(join(repoRoot(), ".github", "workflows", "build.yml"), "utf8");
    expect(hasCommandLine(workflow, "cargo doc", "ledger-infra", "--no-deps", "-D warnings")).toBe(
      true,
    );
  });
});
