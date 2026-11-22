import type {} from "vite/client";

declare global {
  const __GIT_SHA__: string;
  const __GIT_DIRTY__: boolean;
}

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}
