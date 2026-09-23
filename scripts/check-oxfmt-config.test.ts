import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

// oxfmt 配置锚点断言（issue #1519 / ADR-0128）：oxc #25125 —— 配置文件非法时
// oxfmt 静默回退默认值，格式门若在错误风格下放行即是假绿，配置漂移必须在门禁
// 测试层 fail-loud。sortPackageJson 必须显式 false（保 package.json 手工键序，
// ADR-0128 决策 3），删字段回退为默认开启同样在此变红。
// 本文件原承载的「check.sh / CI 步骤接线断言」已移交守门接线测试
//（scripts/gate-mounts.test.ts，issue #1682）：接线成为守门家族 interface 的
// 一环，住可测试的登记（scripts/gate-mounts.ts 守门挂载登记）而非各门自证；
// 本文件只保留配置锚点这一独立守门意义。
// （仓库根 = vitest 进程 cwd，与 run-gate-script 助手同一观察。）

describe("oxfmt 配置锚点（防 #25125 式静默回退）", () => {
  it(".oxfmtrc.json 存在、可解析且风格锚点未漂移", () => {
    const raw = readFileSync(join(process.cwd(), ".oxfmtrc.json"), "utf8");
    const config = JSON.parse(raw) as { printWidth?: number; sortPackageJson?: boolean };
    expect(config.sortPackageJson).toBe(false);
    expect(config.printWidth).toBe(100);
  });
});
