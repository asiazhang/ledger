// 接线型守门测试的共享断言辅助唯一定义点（issue #1487）：hasCommandLine 与
// repoRoot 定位此前在 check.sh / CI 门接线测试文件逐份手写（check-sh-rustdoc-gate
// / check-sh-gate-off 各一份逐字副本，#1473 交付审查发现），纪律同
// run-gate-script.test-helper.ts（#1439，spawnSync run 包装收敛先例）：共享辅助
// 收敛、新增门接线测试直接消费本文件、不再复制副本。边界：本文件只承载「文件
// 内容 → 非注释行片段断言」与仓库根定位；脚本运行包装归 run-gate-script
// .test-helper.ts，测试与所测脚本同目录住（#1158）。

/** 定位仓库根：vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根
 *  （run-gate-script.test-helper.ts gateScript 同款观察；函数形态规避加载期求值）。 */
export function repoRoot(): string {
  return process.cwd()
}

/** 非注释行里是否含全部给定片段（注释行不算命令，与 check-structure.ts 同口径）。 */
export function hasCommandLine(content: string, ...needles: string[]): boolean {
  return content
    .split('\n')
    .some(
      (line) =>
        !line.trim().startsWith('#') && needles.every((n) => line.includes(n)),
    )
}
