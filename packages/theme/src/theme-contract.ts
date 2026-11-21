import type { GlobalThemeOverrides } from "naive-ui";
import type { Theme } from "./theme";
import { darkOverrides, lightOverrides } from "./overrides";
import { darkThemeClass, lightThemeClass } from "./app-theme.css.ts";

export interface AppThemeContract {
  rootClass: string;
  overrides: GlobalThemeOverrides;
}

export function resolveAppTheme(theme: Theme): AppThemeContract {
  return theme === "dark"
    ? { rootClass: darkThemeClass, overrides: darkOverrides }
    : { rootClass: lightThemeClass, overrides: lightOverrides };
}

export function bindRootThemeClass(rootClass: string): void {
  for (const cls of [darkThemeClass, lightThemeClass]) {
    document.body.classList.toggle(cls, cls === rootClass);
  }
}
