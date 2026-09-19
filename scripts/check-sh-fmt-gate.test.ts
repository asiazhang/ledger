import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { hasCommandLine, repoRoot } from "./has-command-line.test-helper.ts";

// oxfmt 格式检查门禁接线测试（issue #1519 / ADR-0128）：前端格式化以 `oxfmt
// --check` 纳入质量门。本测试是「删除即变红」负向判据的载体（ADR-0087 断言
// 强度）：从 scripts/check.sh 删掉该步骤、或从 CI（build.yml frontend job）删掉
// 对应 step，至少一条断言变红——门禁接线不靠评审记忆守卫。（断言辅助消费
// scripts/has-command-line.test-helper.ts，#1487 上收唯一定义点；接线先例
// check-sh-rustdoc-gate.test.ts）
//
// .oxfmtrc.json 解析断言另有一层守门意义：oxc #25125，配置文件非法时 oxfmt
// 静默回退默认值——格式门若在错误风格下放行即是假绿，配置漂移必须在门禁
// 测试层 fail-loud。sortPackageJson 必须显式 false（保 package.json 手工键序，
// ADR-0128 决策 3），删字段回退为默认开启同样在此变红。

describe("oxfmt 格式检查门禁接线（check.sh 质量门槛宿主）", () => {
  // 断言对准执行行（pnpm exec oxfmt --check .）而非 echo 文案：echo 同样含
  // 「oxfmt --check」字样，删掉真实命令只留 echo 时断言不得假绿（ADR-0087
  // 断言强度：对准用户可观察的门禁执行，不对准日志文案）。
  it("scripts/check.sh 含 oxfmt --check 执行步骤", () => {
    const checkSh = readFileSync(join(repoRoot(), "scripts", "check.sh"), "utf8");
    expect(hasCommandLine(checkSh, "pnpm exec oxfmt --check .")).toBe(true);
  });

  it("CI frontend job 含同款 oxfmt --check 步骤（删步骤即红）", () => {
    const workflow = readFileSync(join(repoRoot(), ".github", "workflows", "build.yml"), "utf8");
    expect(hasCommandLine(workflow, "pnpm exec oxfmt --check .")).toBe(true);
  });

  it(".oxfmtrc.json 存在、可解析且风格锚点未漂移（防 #25125 式静默回退）", () => {
    const raw = readFileSync(join(repoRoot(), ".oxfmtrc.json"), "utf8");
    const config = JSON.parse(raw) as { printWidth?: number; sortPackageJson?: boolean };
    expect(config.sortPackageJson).toBe(false);
    expect(config.printWidth).toBe(100);
  });
});
