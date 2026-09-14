/**
 * 构建期 Git 版本信息。
 *
 * 生产路径：`vite.config.ts` 经 `define` 注入全局常量 `__GIT_SHA__`（完整 40 位）
 * 与 `__GIT_DIRTY__`（构建时工作树是否含未提交改动），`tauri dev` 与
 * `tauri build` 均经 Vite 生效；非 Git 目录降级为空值/不脏。
 *
 * 测试路径：Vitest 无 define，`typeof` 守卫先于标识符求值，测试可用
 * `vi.stubGlobal('__GIT_SHA__', ...)` 等价注入固定值。
 */
export function gitShaFull(): string {
  if (typeof __GIT_SHA__ === 'string') return __GIT_SHA__
  return ''
}

function isGitDirty(): boolean {
  return typeof __GIT_DIRTY__ === 'boolean' && __GIT_DIRTY__
}

/** 短 sha（前 7 位），脏树追加 `-dirty`；无法读取 Git 信息时返回空串。 */
export function gitVersionLabel(): string {
  const sha = gitShaFull()
  if (!sha) return ''
  return sha.slice(0, 7) + (isGitDirty() ? '-dirty' : '')
}
