import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  CHECK_SH_FILE,
  CI_NON_GATE_RUNS,
  CI_WORKFLOW_FILE,
  GATE_MOUNTS,
  type GateHostFile,
  type GateMount,
} from "./gate-mounts.ts";

const repoRoot = process.cwd();

function readHost(file: string): string {
  return readFileSync(join(repoRoot, file), "utf8");
}

function mounted(line: string, prefix: string): boolean {
  const t = line.trim();
  return t !== "" && !t.startsWith("#") && t.startsWith(prefix);
}

function mountOf(
  entry: GateMount,
  file: GateHostFile,
): { prefix: string; where: string } | undefined {
  return entry.hosts.find((h) => h.file === file);
}

function checkShGateLines(content: string): string[] {
  const out: string[] = [];
  let started = false;
  for (const raw of content.split("\n")) {
    const t = raw.trim();
    if (!started) {
      if (t.startsWith('echo "▶')) started = true;
      continue;
    }
    if (t === "" || t.startsWith("#") || t.startsWith("echo")) continue;
    out.push(t);
  }
  return out;
}

function ciRunLinesByJob(content: string): Map<string, string[]> {
  const jobs = new Map<string, string[]>();
  let inJobs = false;
  let current = "";
  for (const raw of content.split("\n")) {
    if (raw === "jobs:") {
      inJobs = true;
      continue;
    }
    if (!inJobs) continue;
    const jobMatch = /^ {2}([A-Za-z_][A-Za-z0-9_-]*):\s*$/.exec(raw);
    if (jobMatch !== null) {
      current = jobMatch[1] ?? "";
      jobs.set(current, []);
      continue;
    }
    if (current === "") continue;
    const withoutDecoration = raw.trim().replace(/^- /, "");
    if (!withoutDecoration.startsWith("run:")) continue;
    const value = withoutDecoration.slice("run:".length).trim();
    if (value === "" || value.startsWith("|") || value.startsWith(">")) continue;
    jobs.get(current)?.push(withoutDecoration);
  }
  return jobs;
}

function prefixesFor(file: GateHostFile): string[] {
  return GATE_MOUNTS.map((entry) => mountOf(entry, file)?.prefix).filter(
    (p): p is string => p !== undefined,
  );
}

describe("守门挂载登记（删除即变红，issue #1682）", () => {
  it("登记结构不变量：名称唯一、每条含 check.sh 宿主与 issue/ADR 出处", () => {
    expect(GATE_MOUNTS.length).toBeGreaterThan(0);
    const names = GATE_MOUNTS.map((entry) => entry.name);
    expect(new Set(names).size, "登记名称必须唯一").toBe(names.length);
    for (const entry of GATE_MOUNTS) {
      expect(entry.refs.trim(), `出处（导航字段）缺失：${entry.name}`).not.toBe("");
      const sh = mountOf(entry, CHECK_SH_FILE);
      expect(sh, `缺 check.sh 宿主声明：${entry.name}`).toBeDefined();
      expect(sh?.prefix.trim(), `check.sh 执行行形态为空：${entry.name}`).not.toBe("");
      expect(sh?.where.trim(), `宿主定位说明为空：${entry.name}`).not.toBe("");
    }
  });

  it("check.sh 执行行 ↔ 登记双向全等（删执行行即红、删登记条目即红）", () => {
    const lines = checkShGateLines(readHost(CHECK_SH_FILE));
    expect(lines.length, "check.sh 门执行行提取为空——门标签结构漂移会让核对空转").toBeGreaterThan(
      0,
    );
    const prefixes = prefixesFor(CHECK_SH_FILE);
    for (const line of lines) {
      const claims = prefixes.filter((prefix) => mounted(line, prefix));
      expect(claims.length, `check.sh 执行行未被任何登记条目认领（删登记条目即红）：${line}`).toBe(
        1,
      );
    }
    for (const entry of GATE_MOUNTS) {
      const prefix = mountOf(entry, CHECK_SH_FILE)?.prefix ?? "";
      expect(
        lines.some((line) => mounted(line, prefix)),
        `登记条目在 check.sh 无执行行（删宿主挂载即红）：${entry.name}`,
      ).toBe(true);
    }
  });

  it("CI 步骤 ↔ 登记按宿主投影双向全等（删步骤即红、删登记条目即红）", () => {
    const ciPrefixes = prefixesFor(CI_WORKFLOW_FILE);
    expect(ciPrefixes.length, "CI 宿主零登记——挂载丢失会让本核对空转").toBeGreaterThan(0);
    const jobs = ciRunLinesByJob(readHost(CI_WORKFLOW_FILE));
    const gateJobs = [...jobs.entries()]
      .filter(([, lines]) => lines.some((line) => ciPrefixes.some((p) => line.startsWith(p))))
      .map(([job]) => job);
    expect(gateJobs.length, "CI 无任何门步骤命中登记（登记的 CI 挂载全部丢失）").toBeGreaterThan(0);
    for (const job of gateJobs) {
      for (const line of jobs.get(job) ?? []) {
        const claims = ciPrefixes.filter((prefix) => line.startsWith(prefix));
        if (CI_NON_GATE_RUNS.includes(line)) {
          expect(claims.length, `CI 非门步骤被登记误认领（豁免清单与登记冲突）：${line}`).toBe(0);
          continue;
        }
        expect(claims.length, `CI ${job} 步骤未被任何登记条目认领（删登记条目即红）：${line}`).toBe(
          1,
        );
      }
    }
    const allCiLines = [...jobs.values()].flat();
    for (const entry of GATE_MOUNTS) {
      const prefix = mountOf(entry, CI_WORKFLOW_FILE)?.prefix;
      if (prefix === undefined) continue;
      expect(
        allCiLines.some((line) => line.startsWith(prefix)),
        `登记条目在 CI 无步骤（删宿主挂载即红）：${entry.name}`,
      ).toBe(true);
    }
  });

  it("echo 展示行与注释行结构上不可能命中挂载（行首前缀判定，#1112 假绿教训）", () => {
    const invocation = "bun scripts/check-frontend-structure.ts";
    expect(mounted(`echo "▶ 前端结构守门 (${invocation})"`, invocation)).toBe(false);
    expect(mounted(`# ${invocation}`, invocation)).toBe(false);
    expect(mounted(`  ${invocation}  `, invocation)).toBe(true);
  });
});
