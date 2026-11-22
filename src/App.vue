<script setup lang="ts">
import {
  computed,
  h,
  nextTick,
  ref,
  watch,
  watchEffect,
  type Component,
  type HTMLAttributes,
} from "vue";
import { RouterView, useRouter, useRoute } from "vue-router";
import {
  NConfigProvider,
  NMessageProvider,
  NDialogProvider,
  enUS,
  dateEnUS,
  NLayout,
  NLayoutSider,
  NLayoutContent,
  NMenu,
  NIcon,
  NButton,
  darkTheme,
  zhCN,
  dateZhCN,
  type MenuOption,
} from "naive-ui";
import AppDropdown from "@ledger/ui-kit/AppDropdown.vue";
import MobileNavShell from "@/components/MobileNavShell.vue";
import {
  HomeOutline,
  SwapHorizontalOutline,
  SearchOutline,
  WalletOutline,
  BarChartOutline,
  TrendingUpOutline,
  CubeOutline,
  CalculatorOutline,
  ChevronForwardOutline,
  LayersOutline,
  SparklesOutline,
  SettingsOutline,
  RepeatOutline,
  StorefrontOutline,
  ShieldCheckmarkOutline,
  UmbrellaOutline,
  EyeOutline,
  EyeOffOutline,
  FlagOutline,
} from "@vicons/ionicons5";
import { useAppStore } from "@/stores/app";
import { currentLocale, t } from "@ledger/i18n";
import { viewLabel } from "@ledger/i18n/view-label";
import { bindRootThemeClass, resolveAppTheme } from "@ledger/theme/theme-contract";
import { useEncryptionGate } from "@/backup/useEncryptionGate";
import UnlockScreen from "@/backup/UnlockScreen.vue";
import StartupFailureScreen from "@/backup/StartupFailureScreen.vue";
import DevicePreferenceSyncHost from "@/components/DevicePreferenceSyncHost.vue";
import UpdateReminderHost from "@/components/UpdateReminderHost.vue";
import MessageSinkBridge from "@/components/MessageSinkBridge.vue";
import GlobalBusyBar from "@/components/GlobalBusyBar.vue";
import GlobalErrorBanner from "@/components/GlobalErrorBanner.vue";
import BookSidebarEntry from "@/settings/BookSidebarEntry.vue";
import { loadSidebarCollapsed, saveSidebarCollapsed } from "@ledger/utils/view-state";
import { shortcutHint, useViewShortcuts } from "@/composables/useViewShortcuts";
import { useInputMode } from "@/composables/useInputMode";
import {
  useSidebarOrderStore,
  isSidebarSortAction,
  groupOfView,
  buildSidebarSortMenuOptions,
  FIRST_VIEW,
  PENULTIMATE_VIEW,
  LAST_VIEW,
  type ViewName,
  type ContainableViewName,
} from "@/stores/sidebar-order";
import { useFeatureToggleStore } from "@/settings/feature-toggles";
import { useWindowGuard } from "@/composables/useWindowGuard";
import { useSystemBack } from "@/composables/useSystemBack";
import { useWindowTier } from "@ledger/window-tier";

const router = useRouter();
const route = useRoute();

const sidebarOrder = useSidebarOrderStore();
const { applySidebarSort, applyMoveIntoMore, resetSidebarOrder, isSidebarMember } = sidebarOrder;
const featureToggles = useFeatureToggleStore();
const { viewShortcuts } = useViewShortcuts(router);

useWindowGuard();

useSystemBack();

const tier = useWindowTier();

const siderWidth = computed(() => (currentLocale.value === "en-US" ? 200 : 160));

const sidebarCollapsed = ref(loadSidebarCollapsed());
function updateSidebarCollapsed(collapsed: boolean) {
  sidebarCollapsed.value = collapsed;
  saveSidebarCollapsed(collapsed);
}
const store = useAppStore();

const appTheme = computed(() => resolveAppTheme(store.theme));
watchEffect(() => bindRootThemeClass(appTheme.value.rootClass));

watch(
  currentLocale,
  async () => {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      void getCurrentWindow().setTitle(t("common.app.name"));
    } catch {}
  },
  { immediate: true },
);

const naiveLocale = computed(() => (currentLocale.value === "en-US" ? enUS : zhCN));
const naiveDateLocale = computed(() => (currentLocale.value === "en-US" ? dateEnUS : dateZhCN));

const { locked, bootFailed, probe } = useEncryptionGate();
void probe();

const viewIcons: Record<string, Component> = {
  dashboard: HomeOutline,
  transactions: SwapHorizontalOutline,
  search: SearchOutline,
  accounts: WalletOutline,
  reports: BarChartOutline,
  investments: TrendingUpOutline,
  items: CubeOutline,
  budget: CalculatorOutline,
  ai: SparklesOutline,
  settings: SettingsOutline,
  scheduled: RepeatOutline,
  merchants: StorefrontOutline,
  policies: ShieldCheckmarkOutline,
  physicalAssets: CubeOutline,
  savingsGoals: FlagOutline,
  insurers: UmbrellaOutline,
};

function renderMenuIcon(name: string) {
  return () => h(NIcon, { size: 18 }, { default: () => h(viewIcons[name]) });
}

function renderItem(name: ViewName | ContainableViewName, key: string | null): MenuOption {
  return {
    key: name,
    icon: renderMenuIcon(name),
    label: () =>
      h(
        "div",
        {
          style:
            "display:flex;justify-content:space-between;align-items:center;gap:12px;padding-right:2px",
        },
        [
          h("span", viewLabel(name)),
          key === null
            ? null
            : h("span", { style: "font-size:12px;opacity:.55" }, shortcutHint(key)),
        ],
      ),
  };
}

const inputMode = useInputMode();

function buildMenuOptions(keyOf: Map<string, string | null>): MenuOption[] {
  const item = (name: ViewName | ContainableViewName) => renderItem(name, keyOf.get(name) ?? null);
  const closed = (name: ContainableViewName) => featureToggles.isFeatureClosed(name);
  return [
    item(FIRST_VIEW),
    ...sidebarOrder.sidebarGroups.map((g): MenuOption => {
      const visibleContainment = sidebarOrder.sidebarContainment[g.id].filter(
        (name) => !closed(name),
      );
      return {
        type: "group",
        key: `sidebar-group:${g.id}`,
        label: () =>
          h("div", { class: "sidebar-group-title" }, [
            h("span", t(`common.sidebarGroup.${g.id}`)),
            visibleContainment.length > 0
              ? h(
                  "a",
                  {
                    class: ["group-more-link", { "is-active": route.name === `${g.id}-more` }],
                    title: visibleContainment
                      .map((n) => viewLabel(n))
                      .join(currentLocale.value === "en-US" ? ", " : "、"),
                    onClick: () => {
                      void router.push({ name: `${g.id}-more` });
                    },
                  },
                  [
                    h(
                      NIcon,
                      { size: 14, class: "group-more-icon" },
                      { default: () => h(LayersOutline) },
                    ),
                    t("common.nav.more"),
                    h(
                      NIcon,
                      { size: 12, class: "group-more-caret" },
                      { default: () => h(ChevronForwardOutline) },
                    ),
                  ],
                )
              : null,
          ]),
        children: g.views.filter((name) => !closed(name)).map((name) => item(name)),
      };
    }),
    item(PENULTIMATE_VIEW),
    item(LAST_VIEW),
  ];
}

const menuOptions = computed<MenuOption[]>(() =>
  buildMenuOptions(
    inputMode.value === "pointer"
      ? new Map(viewShortcuts.value.map((s) => [s.name, s.key]))
      : new Map(),
  ),
);

const drawerMenuOptions = computed<MenuOption[]>(() => buildMenuOptions(new Map()));

function nodeProps(option: MenuOption) {
  const name = option.key as string;
  if (!isSidebarMember(name)) return {};
  return {
    onContextmenu: (e: MouseEvent) => showSortMenu(e, name),
  } as unknown as HTMLAttributes & Record<string, string | number | undefined>;
}

const sortMenuShow = ref(false);
const sortMenuX = ref(0);
const sortMenuY = ref(0);
const sortTarget = ref<ContainableViewName | null>(null);

const sortMenuOptions = computed(() => {
  const target = sortTarget.value;
  const gid = target ? groupOfView(target) : null;
  if (!target || !gid) return [];
  return buildSidebarSortMenuOptions(target, sidebarOrder.sidebarGroupOrders[gid]);
});

function showSortMenu(e: MouseEvent, name: ContainableViewName) {
  sortTarget.value = name;
  sortMenuX.value = e.clientX;
  sortMenuY.value = e.clientY;
  sortMenuShow.value = false;
  void nextTick(() => {
    sortMenuShow.value = true;
  });
}

function onSortMenuSelect(key: string) {
  sortMenuShow.value = false;
  const target = sortTarget.value;
  if (!target) return;
  if (key === "reset") {
    resetSidebarOrder();
    return;
  }
  if (key === "intoMore") {
    applyMoveIntoMore(target);
    return;
  }
  if (isSidebarSortAction(key)) {
    applySidebarSort(target, key);
  }
}

function handleSelect(key: string) {
  router.push({ name: key });
}

const brandRow = () => {
  const privacyLabel = store.amountPrivacyEnabled
    ? t("common.amountPrivacy.show")
    : t("common.amountPrivacy.hide");
  return h(
    "div",
    {
      style:
        "display:flex;align-items:center;justify-content:space-between;gap:4px;min-width:0;padding:12px 8px 12px 16px;font-size:18px;font-weight:600",
    },
    [
      h("span", `📒 ${t("common.app.name")}`),
      h(
        NButton,
        {
          size: "tiny",
          quaternary: true,
          circle: true,
          "aria-pressed": store.amountPrivacyEnabled,
          title: privacyLabel,
          "aria-label": privacyLabel,
          onClick: () => store.setAmountPrivacyEnabled(!store.amountPrivacyEnabled),
        },
        {
          icon: () =>
            h(
              NIcon,
              { size: 16 },
              { default: () => h(store.amountPrivacyEnabled ? EyeOffOutline : EyeOutline) },
            ),
        },
      ),
    ],
  );
};

const pageTitle = computed(() => (typeof route.name === "string" ? viewLabel(route.name) : ""));
</script>

<template>
  <!-- 非内联主题变量模式（issue #2186）：组件库 CSS 变量改为共享样式类下发，
       消除百行交易表格及全应用的逐元素内联 --n-* 样式体积与 WebKitGTK 重算开销 -->
  <NConfigProvider
    :theme="store.theme === 'dark' ? darkTheme : null"
    :theme-overrides="appTheme.overrides"
    :locale="naiveLocale"
    :date-locale="naiveDateLocale"
    inline-theme-disabled
  >
    <!-- 全局忙碌条（issue #500）：非模态环境指示，只随忙碌状态渲染，
         不注册 Overlay Suppression（ADR-0035 豁免，见词汇表词条） -->
    <GlobalBusyBar />
    <!-- 全局渲染错误提示条（issue #926）：非模态环境指示，同一豁免口径 -->
    <GlobalErrorBanner />
    <NMessageProvider>
      <NDialogProvider>
        <!-- 启动失败分支（issue #601）：失败恢复屏整体替代主界面 -->
        <StartupFailureScreen v-if="bootFailed" />
        <!-- 加密锁定分支（issue #570）：解锁屏整体替代主界面 -->
        <UnlockScreen v-if="locked" />
        <!-- Loadable toast sink 注册桥（ADR-0040）：必须在消息提供器子树内 -->
        <MessageSinkBridge v-if="!bootFailed && locked !== true" />
        <!-- 探测中（null）不渲染主界面；解锁/明文后随设备偏好推送宿主一起挂载 -->
        <template v-if="locked === false">
          <DevicePreferenceSyncHost />
          <!-- 应用更新提醒宿主（issue #1431 / ADR-0124）：形态探测与提醒事件订阅
               随已解锁分支挂载（锁定期间更新面不可达，先例设备偏好推送宿主） -->
          <UpdateReminderHost />
          <!-- 窗口分级分支（ADR-0088 决策 2/4）：≥840 桌面档渲染既有侧栏布局（一字不动）；
               <840 移动档渲染导航壳（顶栏 + 导航抽屉，词汇表「导航抽屉」）。两档消费同一份
               menuOptions / handleSelect 导航状态；右键排序菜单仅桌面档（桌面专属管理动作
               不上抽屉，ADR-0088 决策 10）。 -->
          <template v-if="tier === 'desktop'">
            <NLayout has-sider style="height: 100vh">
              <NLayoutSider
                bordered
                :width="siderWidth"
                :collapsed="sidebarCollapsed"
                :collapsed-width="0"
                show-trigger="arrow-circle"
                collapse-mode="width"
                @update:collapsed="updateSidebarCollapsed"
              >
                <div class="sider-column">
                  <component :is="brandRow" />
                  <div class="sider-menu-area">
                    <NMenu
                      :options="menuOptions"
                      :value="route.name as string"
                      :indent="16"
                      :node-props="nodeProps"
                      @update:value="handleSelect"
                    />
                  </div>
                  <!-- 侧栏左下角账本入口（issue #834 / ADR-0089）：当前账本名按钮 →
                   清单弹层（切换/新建/改名/移除）；折叠时入口以浮标图标形态可达 -->
                  <BookSidebarEntry :collapsed="sidebarCollapsed" />
                </div>
                <!-- 可排区右键组内排序菜单（issue #270/#359）：手动定位弹出 -->
                <AppDropdown
                  trigger="manual"
                  placement="bottom-start"
                  :show="sortMenuShow"
                  :x="sortMenuX"
                  :y="sortMenuY"
                  :options="sortMenuOptions"
                  style="max-width: 140px"
                  @select="onSortMenuSelect"
                  @clickoutside="sortMenuShow = false"
                />
              </NLayoutSider>
              <NLayoutContent content-style="padding: 0;" style="height: 100%">
                <div class="app-page-shell">
                  <!-- 屏名即 h1（tasteskill app 规则：每个屏有且仅有一个 h1，
                       且它就是屏名）；所有视图共用壳层这一处，不在各页再写一份标题 -->
                  <h1 class="page-title">{{ pageTitle }}</h1>
                  <RouterView />
                </div>
              </NLayoutContent>
            </NLayout>
          </template>
          <template v-else>
            <MobileNavShell
              :menu-options="drawerMenuOptions"
              :title="pageTitle"
              @select="handleSelect"
            >
              <!-- 品牌行复用侧栏标题行渲染函数：应用名 + 金额隐私眼睛按钮，
                   侧栏的另一壳层渲染，入口仍单点（spec #564 不设第二渲染点） -->
              <template #brand>
                <component :is="brandRow" />
              </template>
              <RouterView />
            </MobileNavShell>
          </template>
        </template>
      </NDialogProvider>
    </NMessageProvider>
  </NConfigProvider>
</template>
<style scoped>
.page-title {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
  line-height: 1.4;
  color: inherit;
}

.sider-column {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.sider-menu-area {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
</style>
