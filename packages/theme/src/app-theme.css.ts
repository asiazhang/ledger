import { createTheme, createThemeContract, globalStyle } from "@vanilla-extract/css";
import { NEUTRAL_TOKENS, type NeutralTokens } from "./design-tokens";

/**
 * 新方案主题（vanilla-extract 主题合同，issue #888 / ADR-0093）：
 * 亮/暗两个主题类持有同一组合同变量（appVars），取值从中性 Design Tokens
 * 单一来源（`design-tokens.ts`）构建期派生——两套视觉口径同源，一致性由派生
 * 保证而非人工同步。
 *
 * 约定：
 * - **派生方向单向**：本模块是 token 的消费方，不持独立副本；token 增删字段时
 *   合同形状（ContractOf）编译报错，强制同步。
 * - **主题类绑定应用根元素**（document.body，由 `theme-contract.ts` 的
 *   `bindRootThemeClass` 承担），由 Appearance 设备偏好驱动，与组件库主题切换
 *   并行不互扰，不出现第二主题状态源；teleport 到 body 的弹层同域继承变量。
 * - **合同变量只在此翻转取值**：消费方（sprinkles / 旁路样式文件）一律经
 *   `var()` 引用，不烘焙具体色值——主题切换 = 根元素主题类整体换装这一个动作。
 * - 组件库主题覆盖走 `overrides.ts`（同样从 token 派生），与本模块并行。
 */

/** 合同形状 = NeutralTokens 的深 null 投影：token 增删字段时此处编译报错。 */
type ContractOf<T> = { [K in keyof T]: T[K] extends string ? null : ContractOf<T[K]> };

const contract: ContractOf<NeutralTokens> = {
  radius: { small: null, base: null, large: null },
  background: { body: null, card: null, popover: null, modal: null },
  border: { border: null, divider: null },
  text: { primary: null, secondary: null, tertiary: null },
};

export const appVars = createThemeContract(contract);

export const darkThemeClass = createTheme(appVars, NEUTRAL_TOKENS.dark);
export const lightThemeClass = createTheme(appVars, NEUTRAL_TOKENS.light);

/**
 * 应用根底色与基色文字（全局单点）：主题类绑在 `document.body` 上，故本体规则即
 * 「当前主题的页面底色 + 正文色」。缺了这条，未显式取色的原生元素（`<span>`、
 * 计数、说明句）继承浏览器默认的 `#000`——暗色下是黑字落在近黑底上（实测 1.1:1，
 * 等于不可见），亮色下页面底色也会在浏览器/无窗口底板环境下露白。
 */
globalStyle("body", {
  backgroundColor: appVars.background.body,
  color: appVars.text.primary,
});
