import { DOMWrapper, type VueWrapper } from '@vue/test-utils'

/**
 * 测试侧 DOM 查找助手的单一出口（issue #746，ADR-0085 决策 6）。
 *
 * 收编全仓测试文件中按钮查找（文本匹配 / body-teleport / data-testid 三变体）
 * 与输入查找家族的同构副本：既有形态为「`findAll('button')` + 文本过滤 +
 * 调用方自行断言」与「弹窗 teleport 到 body 后 `document.body.querySelector`」
 * 两类，本文件在单点收窄为带类型的查找函数。
 *
 * 约定（两种未命中形态，均不抛错，命中与否由调用方按用例语义断言）：
 * - 文本/body 两变体返回 `DOMWrapper | undefined`，未命中为 undefined；
 * - testid 变体与 findInput 返回 test-utils 的不存在 wrapper（`exists()` 为
 *   false），保持与 `wrapper.find` 直用形态一致——「找不到即失败」的期望
 *   文案属于用例，不属于查找；
 * - 文本匹配默认包含匹配（`includes`），`{ exact: true }` 切换精确匹配
 *   （trim 后全等）；wrapper 侧用 test-utils 的 `text()`，body 侧用
 *   `textContent?.trim()`，两者口径一致；
 * - `flushPromises` 刻意不在此包装（ADR-0085 决策 6）：直连官方导入不是布线
 *   噪音，包装反增间接层。
 */

/** 文本匹配选项：`exact` 缺省为包含匹配，置 true 为 trim 后全等。 */
export interface FindByTextOptions {
  exact?: boolean
}

/** 输入查找选项：placeholder / type 任选其一或组合。 */
export interface FindInputOptions {
  placeholder?: string
  type?: string
}

/** wrapper 范围内按文本找按钮（既有 `findAll('button')` + 过滤形态的收口）。 */
export function findButton(
  wrapper: VueWrapper,
  text: string,
  options: FindByTextOptions = {},
): DOMWrapper<HTMLButtonElement> | undefined {
  const hit = wrapper.findAll('button').find((b) =>
    options.exact ? b.text() === text : b.text().includes(text),
  )
  return hit as DOMWrapper<HTMLButtonElement> | undefined
}

/** wrapper 范围内按 data-testid 找按钮（既有 `wrapper.find('[data-testid="…"]')` 形态的收口）。 */
export function findButtonByTestId(wrapper: VueWrapper, testid: string): DOMWrapper<HTMLButtonElement> {
  return wrapper.find(`[data-testid="${testid}"]`) as DOMWrapper<HTMLButtonElement>
}

/** body 范围内按文本找按钮（弹窗经 NModal teleport 到 body 后的查找形态）。 */
export function findBodyButton(
  text: string,
  options: FindByTextOptions = {},
): DOMWrapper<HTMLButtonElement> | undefined {
  const hit = Array.from(document.body.querySelectorAll('button')).find((b) => {
    const t = b.textContent?.trim() ?? ''
    return options.exact ? t === text : t.includes(text)
  })
  return hit ? new DOMWrapper(hit) : undefined
}

/** body 范围内按 data-testid 找按钮（弹窗内按钮的既有 testid 查找形态）。 */
export function findBodyButtonByTestId(testid: string): DOMWrapper<HTMLButtonElement> | undefined {
  const el = document.body.querySelector(`[data-testid="${testid}"]`)
  return el ? new DOMWrapper(el as HTMLButtonElement) : undefined
}

/** 输入查找家族：裸 `input` / `input[placeholder="…"]` / `input[type="…"]` 的公共形态。 */
export function findInput(
  wrapper: VueWrapper,
  options: FindInputOptions = {},
): DOMWrapper<HTMLInputElement> {
  let selector = 'input'
  if (options.placeholder !== undefined) selector += `[placeholder="${options.placeholder}"]`
  if (options.type !== undefined) selector += `[type="${options.type}"]`
  return wrapper.find(selector) as DOMWrapper<HTMLInputElement>
}
