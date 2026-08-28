/// <reference types="vite/client" />

// 构建期由 vite.config.ts define 注入（见 src/utils/git-info.ts）
declare const __GIT_SHA__: string;
declare const __GIT_DIRTY__: boolean;

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}
