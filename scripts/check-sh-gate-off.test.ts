import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

// gate-off 编译门接线测试（门本体 issue #1133 / 入 CI issue #1473）：infra 默认
// feature 不含 axum，clippy --all-features 下门内 cfg 恒被编译，gate-off 形态只有
// `cargo check -p ledger-infra`（默认 feature）核到——error.rs 在无 axum 依赖图下
// 必须独立成立，误在门外引用 axum 即红。该门此前只活在本地 check.sh，CI 完全
// 不可见；现并入 build.yml backend-lint job。本测试是「删除即变红」负向判据的
// 载体（ADR-0087 接线型判据）：从 scripts/check.sh 或 CI 删掉该步骤，至少一条
// 断言变红——门禁接线不靠评审记忆守卫（先例：check-sh-rustdoc-gate.test.ts、
// #959/#961 源码扫描守门）。
// （vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根定位文件）
const repoRoot = process.cwd()

/** 非注释行里是否含全部给定片段（注释行不算命令，与 check-structure.ts 同口径）。 */
function hasCommandLine(content: string, ...needles: string[]): boolean {
  return content
    .split('\n')
    .some(
      (line) =>
        !line.trim().startsWith('#') && needles.every((n) => line.includes(n)),
    )
}

describe('gate-off 编译门接线（check.sh 与 CI backend-lint 宿主）', () => {
  it('scripts/check.sh 含 cargo check -p ledger-infra 执行步骤（默认 feature）', () => {
    const checkSh = readFileSync(join(repoRoot, 'scripts', 'check.sh'), 'utf8')
    // 锚定执行行而非 echo 提示行：须含 `cd src-tauri && cargo check -p ledger-infra`
    // 形态——只删执行行、留 echo 提示的半删除同样变红。
    expect(
      hasCommandLine(checkSh, 'cd src-tauri', 'cargo check -p ledger-infra'),
    ).toBe(true)
  })

  it('CI backend-lint job 含同款 cargo check -p ledger-infra 步骤（删步骤即红）', () => {
    const workflow = readFileSync(
      join(repoRoot, '.github', 'workflows', 'build.yml'),
      'utf8',
    )
    expect(hasCommandLine(workflow, 'cargo check -p ledger-infra')).toBe(true)
  })
})
