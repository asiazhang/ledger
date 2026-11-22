import { h, type Component } from "vue";
import { NIcon } from "naive-ui";

export function renderRowMenuIcon(icon: Component): () => ReturnType<typeof h> {
  return () => h(NIcon, null, { default: () => h(icon) });
}

export function errorOptionProps(errorColor?: string): Record<string, unknown> {
  return errorColor === undefined
    ? {}
    : {
        props: {
          style: {
            color: errorColor,
            "--n-prefix-color": errorColor,
            "--n-option-text-color-hover": errorColor,
            "--n-option-text-color-active": errorColor,
          },
        },
      };
}
