import { afterAll, describe, expect, it } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  BOOT_WIRING,
  GUARDED_NAMES,
  LANE_FILES,
  LANE_SCHEDULER_REL,
  LANE_WIRE_TOKEN,
  ORCHESTRATOR_FILE,
  ORCHESTRATOR_FN,
} from "../scripts/check-background-services.ts";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

const script = gateScript("check-background-services.ts");
const run = (args: string[]) => runGateScript(script, args);

const tempDirs: string[] = [];
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

const orchestratorPaired = `pub fn ${ORCHESTRATOR_FN}(app: &tauri::AppHandle) {
    backup::start_scheduler(app);
    sync_engine::start_triggers(app);
    market_sync::start_history_backfill(app);
    market_sync::start_daily_price_refresh(app);
    market_sync::start_daily_fx_sync(app);
    app_update::start_auto_update_check(app);
}
`;

const bootWiring = `pub fn run() {
    backup::install_after_commit_hook();
    scheduled_transactions::auto_run::register_after_occurrence_hook(backup::occurrence_dirty_hook);
    backup::register_catch_up_hook(scheduled_transactions::auto_run::catch_up_hook);
}
`;

function makeFixture(overrides: Record<string, string> = {}): string[] {
  const root = mkdtempSync(join(tmpdir(), "check-background-services-"));
  tempDirs.push(root);
  const namesByPath = new Map<string, string[]>();
  for (const guarded of GUARDED_NAMES) {
    for (const path of guarded.wholeFile) {
      namesByPath.set(path, [...(namesByPath.get(path) ?? []), guarded.name]);
    }
  }
  for (const [relPath, names] of namesByPath) {
    const abs = join(root, relPath);
    mkdirSync(join(abs, ".."), { recursive: true });
    const laneWire = (LANE_FILES as readonly string[]).includes(relPath)
      ? `lane::start_daily_lane(app, &SPAWNED, timings, |h| round(h));\n`
      : "";
    writeFileSync(abs, `pub use crate::x::{${names.join(", ")}}; // 再导出桩\n${laneWire}`);
  }
  const files: Record<string, string> = {
    [ORCHESTRATOR_FILE]: bootWiring + orchestratorPaired,
    [LANE_SCHEDULER_REL]:
      "pub fn start_daily_lane() {\n    tauri::async_runtime::spawn(async move {\n        tokio::time::sleep(std::time::Duration::from_millis(1)).await;\n    });\n}\n",
    ...overrides,
  };
  for (const [relPath, content] of Object.entries(files)) {
    const file = join(root, relPath);
    mkdirSync(join(file, ".."), { recursive: true });
    writeFileSync(file, content);
  }
  return [root];
}

describe("check-background-services（后台服务成对拉起守门，issue #961）", () => {
  it("真实仓库默认通过：生产调用收敛于唯一编排点", () => {
    const r = run([]);
    expect(r.status).toBe(0);
    expect(r.output).toContain(ORCHESTRATOR_FN);
    expect(r.output).toContain(`受守入口 ${GUARDED_NAMES.length} 个`);
  });

  it("夹具成对形态通过", () => {
    const r = run(makeFixture());
    expect(r.status).toBe(0);
    expect(r.output).toContain("零脱离");
  });

  it("新加入口只调其中一个 → 失败并定位文件行号（验收判据）", () => {
    const args = makeFixture({
      "src/commands/boot.rs":
        "pub fn restart(app: &tauri::AppHandle) { backup::start_scheduler(&app); }\n",
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("src/commands/boot.rs:1");
    expect(r.output).toContain("start_scheduler");
  });

  it("非白名单文件的文档注释提及标识符不误报（掩码边界）", () => {
    const args = makeFixture({
      "src/commands/encryption.rs":
        "// 解锁后经 start_background_services 拉起 start_scheduler 与 start_triggers。\npub fn resume() {}\n",
    });
    const r = run(args);
    expect(r.status).toBe(0);
  });

  it("编排点函数体缺一侧 → 成组性破坏报红", () => {
    const args = makeFixture({
      [ORCHESTRATOR_FILE]: `pub fn ${ORCHESTRATOR_FN}(app: &tauri::AppHandle) {\n    backup::start_scheduler(app);\n}\n`,
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("成组性破坏");
    expect(r.output).toContain("start_triggers");
  });

  it("删除编排点函数 → 报红（删除即变红）", () => {
    const args = makeFixture({ [ORCHESTRATOR_FILE]: "pub fn unrelated() {}\n" });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("唯一编排点缺失");
  });

  it("编排点所在文件的函数体外调用 → 报红（单点限定函数体）", () => {
    const args = makeFixture({
      [ORCHESTRATOR_FILE]: `${orchestratorPaired}fn sneaky(app: &tauri::AppHandle) {\n    sync_engine::start_triggers(app);\n}\n`,
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("脱离唯一编排点");
  });

  it("直接调 start_sync_scheduler 绕过分平台门 → 报红（#863 缺陷 1 形态）", () => {
    const args = makeFixture({
      "src/commands/encryption.rs":
        "pub fn resume(app: &tauri::AppHandle) {\n    sync_engine::start_sync_scheduler(app);\n}\n",
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("start_sync_scheduler");
    expect(r.output).toContain("绕过分平台门");
  });

  it("编排点函数体内调 start_sync_scheduler → 同样报红（分平台门只住域内一处）", () => {
    const args = makeFixture({
      [ORCHESTRATOR_FILE]: `pub fn ${ORCHESTRATOR_FN}(app: &tauri::AppHandle) {\n    backup::start_scheduler(app);\n    sync_engine::start_triggers(app);\n    sync_engine::start_sync_scheduler(app);\n}\n`,
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("start_sync_scheduler");
  });

  it("白名单条目文件缺失 → 清单漂移报红（fail loud）", () => {
    const root = mkdtempSync(join(tmpdir(), "check-background-services-"));
    tempDirs.push(root);
    const orchestrator = join(root, ORCHESTRATOR_FILE);
    mkdirSync(join(orchestrator, ".."), { recursive: true });
    writeFileSync(orchestrator, orchestratorPaired);
    const r = run([root]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("白名单条目缺失");
  });

  it("启动接线明细注入摘要（issue #1088 提交点后置动作注册）", () => {
    const r = run(makeFixture());
    expect(r.status).toBe(0);
    expect(r.output).toContain(`启动接线 ${BOOT_WIRING.length} 项已接线`);
  });

  it("删除启动接线（写路径副作用接缝注册）→ 报红（删除即变红，#1088 / #1090 / #1091）", () => {
    const args = makeFixture({ [ORCHESTRATOR_FILE]: orchestratorPaired });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("启动接线缺失");
    for (const wiring of BOOT_WIRING) {
      expect(r.output).toContain(wiring.name);
    }
  });

  it("车道改回自建线程 → 车道执行器守门报红（删除即变红，#1413 / ADR-0125 决策 7）", () => {
    const lane = LANE_FILES[0];
    const srcArg = makeFixture({
      [lane]:
        "pub fn start_history_backfill(app: &tauri::AppHandle) {\n    std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_secs(1)); });\n}\n",
    });
    const r = run(srcArg);
    expect(r.status).toBe(1);
    expect(r.output).toContain("车道回归自建线程");
    expect(r.output).toContain("thread::spawn");
  });

  it("车道文件删掉共享调度循环接线 → 报红（删除即变红，#1622 收敛）", () => {
    const lane = LANE_FILES[1];
    const srcArg = makeFixture({
      [lane]:
        "use crate::lane::start_daily_lane;\npub fn start_daily_price_refresh(app: &tauri::AppHandle) {}\n",
    });
    const r = run(srcArg);
    expect(r.status).toBe(1);
    expect(r.output).toContain("车道调度接线缺失");
    expect(r.output).toContain(LANE_WIRE_TOKEN);
  });

  it("调度循环单点删掉异步执行器接线 → 报红（删除即变红，#1413 / #1622）", () => {
    const srcArg = makeFixture({
      [LANE_SCHEDULER_REL]: "pub fn start_daily_lane(app: &tauri::AppHandle) {}\n",
    });
    const r = run(srcArg);
    expect(r.status).toBe(1);
    expect(r.output).toContain("调度循环执行器接线缺失");
    expect(r.output).toContain("tauri::async_runtime::spawn");
  });

  it("crates 内新增调用点 → 报红并定位 crate 文件行号（扩面收口，#1472）", () => {
    const args = makeFixture({
      "crates/backup/src/extra_entry.rs":
        "pub fn restart(app: &tauri::AppHandle) {\n    backup::start_scheduler(app);\n}\n",
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("crates/backup/src/extra_entry.rs:2");
    expect(r.output).toContain("脱离唯一编排点");
    expect(r.output).toContain("start_scheduler");
  });

  it("src/ 与 crates/ 之外的根包文件命中 → 同样报红（扫全树而非两点）", () => {
    const args = makeFixture({
      "build.rs": "fn main() {\n    let _ = start_history_backfill;\n}\n",
    });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("build.rs:2");
    expect(r.output).toContain("脱离唯一编排点");
    expect(r.output).toContain("start_history_backfill");
  });

  it("target/ 内构建产物命中不参与扫描（排除 target/，防生成代码假红）", () => {
    const args = makeFixture({
      "target/debug/build/generated.rs": "pub fn generated() { start_triggers(); }\n",
    });
    const r = run(args);
    expect(r.status).toBe(0);
  });

  it("wholeFile 豁免文件夹具绿（豁免语义不变：crate 定义/再导出整文件放行）", () => {
    const args = makeFixture({
      "crates/backup/src/auto.rs":
        "pub fn start_scheduler() {}\nfn helper() {\n    start_scheduler();\n}\n",
    });
    const r = run(args);
    expect(r.status).toBe(0);
  });

  it("空扫描根拒绝假绿（零源文件即红，扩面后哨兵仍有效）", () => {
    const root = mkdtempSync(join(tmpdir(), "check-background-services-empty-"));
    tempDirs.push(root);
    const r = run([root]);
    expect(r.status).toBe(1);
    expect(r.output).toContain("扫描面提不出任何非测试 Rust 文件");
  });
  it("编排点缺应用更新启动延迟检查 → 报红（删除即变红，ADR-0124 决策 4 / issue #1429）", () => {
    const fiveServices = `pub fn ${ORCHESTRATOR_FN}(app: &tauri::AppHandle) {\n    backup::start_scheduler(app);\n    sync_engine::start_triggers(app);\n    market_sync::start_history_backfill(app);\n    market_sync::start_daily_price_refresh(app);\n    market_sync::start_daily_fx_sync(app);\n}\n`;
    const args = makeFixture({ [ORCHESTRATOR_FILE]: bootWiring + fiveServices });
    const r = run(args);
    expect(r.status).toBe(1);
    expect(r.output).toContain("成组性破坏");
    expect(r.output).toContain("start_auto_update_check");
  });
});
