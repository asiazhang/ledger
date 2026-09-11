import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  BOOT_WIRING,
  GUARDED_NAMES,
  ORCHESTRATOR_FILE,
  ORCHESTRATOR_FN,
} from '../../scripts/check-background-services.ts'

// 被测对象是仓库工具脚本 scripts/check-background-services.ts（后台服务成对
// 拉起守门，issue #961）。脚本以 Bun 运行时执行（ADR-0083）：spawnSync('bun')
// 与门槛调用同款，测的就是门槛路径。按测试决策只测外部可观察结果——进程
// 退出码与输出，不测内部函数；通过位置参数把扫描目标指向临时夹具目录
// （check-structure.test.ts 同款先例；vitest 转换后 import.meta.url 非 file:
// scheme，取进程 cwd = 仓库根定位脚本）。
// 夹具白名单清单自脚本导出的 GUARDED_NAMES 派生（单一事实源，无双源漂移）。
const script = join(process.cwd(), 'scripts', 'check-background-services.ts')

interface RunResult {
  status: number
  output: string
}

function run(args: string[]): RunResult {
  const r = spawnSync('bun', [script, ...args], { encoding: 'utf8' })
  return { status: r.status ?? -1, output: (r.stdout ?? '') + (r.stderr ?? '') }
}

const tempDirs: string[] = []
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true })
})

/** 编排点夹具：函数体内两个成对入口同时出现（合法唯一形态） */
const orchestratorPaired = `pub fn ${ORCHESTRATOR_FN}(app: &tauri::AppHandle) {
    backup::start_scheduler(app);
    sync_engine::start_triggers(app);
}
`

/** 启动接线夹具：壳层启动处注册提交点后置动作（issue #1088 启动接线单点） */
const bootWiring = `pub fn run() {
    backup::install_after_commit_hook();
}
`

/**
 * 建临时夹具：按脚本导出的 GUARDED_NAMES 生成全部白名单条目文件（每文件
 * 写入映射到它的全部受守标识符）+ 编排点 `lib.rs`（成对形态），再按 overrides
 * 追加/覆盖文件。返回脚本参数（夹具 src 目录）。
 */
function makeFixture(overrides: Record<string, string> = {}): string[] {
  const src = mkdtempSync(join(tmpdir(), 'check-background-services-'))
  tempDirs.push(src)
  const namesByPath = new Map<string, string[]>()
  for (const guarded of GUARDED_NAMES) {
    for (const path of guarded.wholeFile) {
      namesByPath.set(path, [...(namesByPath.get(path) ?? []), guarded.name])
    }
  }
  for (const [relPath, names] of namesByPath) {
    const abs = join(src, relPath)
    mkdirSync(join(abs, '..'), { recursive: true })
    writeFileSync(abs, `pub use crate::x::{${names.join(', ')}}; // 再导出桩\n`)
  }
  const files: Record<string, string> = {
    [ORCHESTRATOR_FILE]: bootWiring + orchestratorPaired,
    ...overrides,
  }
  for (const [relPath, content] of Object.entries(files)) {
    const file = join(src, relPath)
    mkdirSync(join(file, '..'), { recursive: true })
    writeFileSync(file, content)
  }
  return [src]
}

describe('check-background-services（后台服务成对拉起守门，issue #961）', () => {
  it('真实仓库默认通过：生产调用收敛于唯一编排点', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(ORCHESTRATOR_FN)
    // 摘要中的受守入口数自脚本导出的 GUARDED_NAMES 派生（单一事实源）
    expect(r.output).toContain(`受守入口 ${GUARDED_NAMES.length} 个`)
  })

  it('夹具成对形态通过', () => {
    const r = run(makeFixture())
    expect(r.status).toBe(0)
    expect(r.output).toContain('零脱离')
  })

  it('新加入口只调其中一个 → 失败并定位文件行号（验收判据）', () => {
    const args = makeFixture({
      'commands/boot.rs': 'pub fn restart(app: &tauri::AppHandle) { backup::start_scheduler(&app); }\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('commands/boot.rs:1')
    expect(r.output).toContain('start_scheduler')
  })

  it('非白名单文件的文档注释提及标识符不误报（掩码边界）', () => {
    const args = makeFixture({
      'commands/encryption.rs':
        '// 解锁后经 start_background_services 拉起 start_scheduler 与 start_triggers。\npub fn resume() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('编排点函数体缺一侧 → 成对性破坏报红', () => {
    const args = makeFixture({
      [ORCHESTRATOR_FILE]:
        `pub fn ${ORCHESTRATOR_FN}(app: &tauri::AppHandle) {\n    backup::start_scheduler(app);\n}\n`,
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('成对性破坏')
    expect(r.output).toContain('start_triggers')
  })

  it('删除编排点函数 → 报红（删除即变红）', () => {
    const args = makeFixture({ [ORCHESTRATOR_FILE]: 'pub fn unrelated() {}\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('唯一编排点缺失')
  })

  it('编排点所在文件的函数体外调用 → 报红（单点限定函数体）', () => {
    const args = makeFixture({
      [ORCHESTRATOR_FILE]: `${orchestratorPaired}fn sneaky(app: &tauri::AppHandle) {\n    sync_engine::start_triggers(app);\n}\n`,
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('脱离唯一编排点')
  })

  it('直接调 start_sync_scheduler 绕过分平台门 → 报红（#863 缺陷 1 形态）', () => {
    const args = makeFixture({
      'commands/encryption.rs': 'pub fn resume(app: &tauri::AppHandle) {\n    sync_engine::start_sync_scheduler(app);\n}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('start_sync_scheduler')
    expect(r.output).toContain('绕过分平台门')
  })

  it('编排点函数体内调 start_sync_scheduler → 同样报红（分平台门只住域内一处）', () => {
    const args = makeFixture({
      [ORCHESTRATOR_FILE]: `pub fn ${ORCHESTRATOR_FN}(app: &tauri::AppHandle) {\n    backup::start_scheduler(app);\n    sync_engine::start_triggers(app);\n    sync_engine::start_sync_scheduler(app);\n}\n`,
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('start_sync_scheduler')
  })

  it('白名单条目文件缺失 → 清单漂移报红（fail loud）', () => {
    const src = mkdtempSync(join(tmpdir(), 'check-background-services-'))
    tempDirs.push(src)
    // 夹具只建编排点，不建白名单条目文件
    writeFileSync(join(src, ORCHESTRATOR_FILE), orchestratorPaired)
    const r = run([src])
    expect(r.status).toBe(1)
    expect(r.output).toContain('白名单条目缺失')
  })

  it('启动接线明细注入摘要（issue #1088 提交点后置动作注册）', () => {
    const r = run(makeFixture())
    expect(r.status).toBe(0)
    expect(r.output).toContain(`启动接线 ${BOOT_WIRING.length} 项已接线`)
  })

  it('删除启动接线（提交点后置动作注册）→ 报红（删除即变红，#1088）', () => {
    const args = makeFixture({ [ORCHESTRATOR_FILE]: orchestratorPaired })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('启动接线缺失')
    expect(r.output).toContain(BOOT_WIRING[0].name)
  })
})
