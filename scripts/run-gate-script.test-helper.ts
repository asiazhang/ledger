// 守门脚本包装测试的共享 run 辅助唯一定义点（issue #1439）：spawnSync('bun') 包装
// 此前在 13 个守门测试文件逐份手写：11 份 interface RunResult + function run 副本
// （check-test-support / check-infra-dml 带 = [] 缺省，check-style-blocks 带 cwd /
// 变体脚本覆盖），另 2 份同机制变体（check-i18n-keys / check-dialog-forms 把参数
// 构造烙进包装、无 RunResult），语义全同——以门槛同款运行时执行被测脚本（ADR-0083：
// bun 调用与 check.sh / CI 门槛调用同款，测的就是门槛路径），只取外部可观察接缝：
// 退出码与合并输出（stdout + stderr）。收敛后新增守门包装测试直接消费本文件，
// 不再复制副本；各测试文件只保留被测脚本路径定位（gateScript）与一步绑定 run
// （签名与既有调用点逐字兼容，style-blocks 的 cwd / 变体脚本覆盖、i18n-keys /
// dialog-forms 的领域参数构造留在绑定内）。测试与所测脚本同目录住（#1158）；
// 跨目录消费（src/__tests__）沿既有先例直引 ../../scripts/（被测脚本同款）。

import { spawnSync } from "node:child_process";
import { join } from "node:path";

/** 一次守门脚本运行的外部可观察结果（ADR-0083：退出码与输出即测试接缝） */
export interface RunResult {
  status: number;
  output: string;
}

/** 定位被测守门脚本：仓库根下 scripts/<name>.ts。
 *  （vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根定位） */
export function gateScript(name: string): string {
  return join(process.cwd(), "scripts", name);
}

/** 以 bun 运行被测守门脚本（spawnSync('bun') 与门槛调用同款，ADR-0083），
 *  返回退出码与合并输出（stdout + stderr）。args 缺省为空参（真实仓库默认
 *  路径场景）；opts.cwd 仅 style-blocks 需要（守门的路径归一化以 cwd 为基准，
 *  夹具运行时归一化须落在夹具上）。 */
export function runGateScript(
  script: string,
  args: readonly string[] = [],
  opts: { cwd?: string } = {},
): RunResult {
  const r = spawnSync("bun", [script, ...args], { encoding: "utf8", cwd: opts.cwd });
  return { status: r.status ?? -1, output: (r.stdout ?? "") + (r.stderr ?? "") };
}
