import { createTheme, createThemeContract, globalStyle } from "@vanilla-extract/css";
import { NEUTRAL_TOKENS, type NeutralTokens } from "./design-tokens";

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

globalStyle("body", {
  backgroundColor: appVars.background.body,
  color: appVars.text.primary,
});
