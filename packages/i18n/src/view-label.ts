import { t } from "./index";

export function viewLabel(name: string): string {
  return t(`common.nav.${name}`);
}
