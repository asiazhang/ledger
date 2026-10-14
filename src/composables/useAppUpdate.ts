import { computed, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "@ledger/api";
import { errorMessage } from "@ledger/utils/errors";
import type { UpdateSurface } from "@ledger/types";
import { useAppStore } from "@/stores/app";

/**
 * 应用更新前端编排（issue #1431 / ADR-0124 决策 4/7）：壳层跨域件——提醒弹层
 * 全询问式流程、「已提醒版本」记忆、手动检查与平台分支的单点状态。
 *
 * 形态仿标的信息同步 / 汇率同步先例（`useInstrumentInfoSync` / `useFxSync`）：
 * 更新流程全局只有一场，模块级单例 ref 持有阶段状态，设置卡片与提醒弹层消费
 * 同一份状态——弹层中途关闭下载不中断（后端编排继续），卡片状态行提供「继续
 * 安装」的再入口；不新增 Pinia store（临时任务态不符合设备偏好 store 定位）。
 *
 * **全询问式（ADR-0124 决策 4）**：remind（版本号＋发布说明）→ 确认 → 下载
 * （带进度，进度经后端事件驱动）→ 确认安装重启。每阶段可取消：关闭提醒弹层
 * 不触发下载，安装另有显式二次确认；关闭弹层本身永不触发下载/安装。
 *
 * **「已提醒版本」记忆（每版本至多自动提醒一次）**：启动事件弹层展示后记录
 * 版本号（应用设置 store 持久化，设备级、不随 Backup/Restore 迁移）；同版本
 * 后续启动事件不再弹层；设置页手动检查不受记忆限制、也不写记忆。
 *
 * **平台分支（ADR-0124 决策 1）**：surface = `updater`（桌面三平台）呈现完整
 * 更新面；`release-page`（deb 与 Android）无下载/安装动作——提醒事件忽略，
 * 设置卡片降级为打开发布页引导。
 *
 * **错误就地呈现（ADR-0050）**：检查/下载/安装失败经码化错误本地化文本就地
 * 渲染（卡片错误位 / 弹层错误位），不走全局 toast——错误就地是弹层交互本体
 * （同「自动解锁确认失败不弹 toast」先例）。
 */

/** 启动延迟静默检查发现新版本的前端通知事件（后端常量单点 `UPDATE_AVAILABLE_EVENT`）。 */
export const UPDATE_AVAILABLE_EVENT = "ledger:update-available";

/** 下载进度事件（issue #1430；后端常量单点 `UPDATE_DOWNLOAD_PROGRESS_EVENT`）。 */
export const UPDATE_DOWNLOAD_PROGRESS_EVENT = "ledger:update-download-progress";

/** 发布页地址（deb 与 Android 引导落点）：GitHub Releases 最新版。 */
export const RELEASE_PAGE_URL = "https://github.com/asiazhang/ledger/releases/latest";

/** 更新流程阶段：idle 未开始；remind 提醒待确认；downloading 下载中；downloaded 已下载待安装。 */
export type UpdateStage = "idle" | "remind" | "downloading" | "downloaded";

/** 设置页手动检查的就地结果（idle 初始；checking 进行中；latest 已是最新；available 发现新版本；error 失败）。 */
export type ManualCheckState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "latest" }
  | { kind: "available"; version: string; notes: string | null }
  | { kind: "error"; message: string };

// —— 模块级单例状态（更新流程全局只有一场，切页不丢） ——

/** 更新入口形态：null = 尚未探测完成（探测在宿主挂载时发起，一次）。 */
const surface = ref<UpdateSurface | null>(null);

/** 流程阶段（提醒/下载/安装共享的版本与发布说明挂相邻 ref，阶段迁移不丢载荷）。 */
const stage = ref<UpdateStage>("idle");
const version = ref<string | null>(null);
const notes = ref<string | null>(null);
/** 下载进度（后端进度事件驱动；total 为清单未给出总大小时的空态）。 */
const received = ref(0);
const total = ref<number | null>(null);

/** 弹层关闭旗标：remind 关闭即回到 idle；downloading/downloaded 关闭只收起弹层
 * （流程在后端继续），卡片状态行据旗标给出再入口。 */
const dismissed = ref(false);

/** 流程内错误（下载失败回 remind 重试、安装失败留 downloaded 重试），就地渲染。 */
const flowError = ref<string | null>(null);

/** 设置页手动检查就地结果。 */
const manualCheck = ref<ManualCheckState>({ kind: "idle" });

/** 弹层可见性：阶段非 idle 且未被收起。关闭动作永不触发下载/安装。 */
const modalOpen = computed(() => stage.value !== "idle" && !dismissed.value);

/** 下载进度百分比：总大小未知时为 null（弹层转不确定态）。 */
const downloadPercent = computed<number | null>(() => {
  if (stage.value !== "downloading" || total.value === null || total.value <= 0) return null;
  return Math.min(100, Math.floor((received.value / total.value) * 100));
});

// —— 事件订阅（模块生命周期内一次，注册失败静默——本地事件，先例 useFxSync） ——

let subscribed = false;

function ensureEventSubscription(): void {
  if (subscribed) return;
  subscribed = true;
  void listen<{ version?: unknown; notes?: unknown }>(UPDATE_AVAILABLE_EVENT, (event) => {
    const payload = event.payload;
    // 防脏 payload：版本号必须是非空字符串（兼容性警告随发布说明，可空）。
    if (typeof payload?.version !== "string" || payload.version === "") return;
    offerUpdate(payload.version, typeof payload.notes === "string" ? payload.notes : null);
  }).catch((e) => {
    console.warn(`订阅 ${UPDATE_AVAILABLE_EVENT} 失败`, e);
    subscribed = false;
  });
  void listen<{ received?: unknown; total?: unknown }>(UPDATE_DOWNLOAD_PROGRESS_EVENT, (event) => {
    const payload = event.payload;
    // 迟到事件不复活进度（阶段守卫，先例 useFxSync 在途计数）。
    if (stage.value !== "downloading") return;
    if (typeof payload?.received !== "number" || !Number.isFinite(payload.received)) return;
    received.value = payload.received;
    total.value =
      typeof payload.total === "number" && Number.isFinite(payload.total) ? payload.total : null;
  }).catch((e) => {
    console.warn(`订阅 ${UPDATE_DOWNLOAD_PROGRESS_EVENT} 失败`, e);
  });
}

// —— 动作 ——

/**
 * 启动提醒入口（更新可用事件 → 弹层）：surface 非 updater（deb/Android）忽略；
 * 同版本已提醒过则忽略（「已提醒版本」记忆）；下载/安装进行中的流程不打断。
 * 弹层展示即记录版本（每版本至多自动提醒一次）。
 */
function offerUpdate(newVersion: string, newNotes: string | null): void {
  if (surface.value !== "updater") return;
  const store = useAppStore();
  if (store.updateRemindedVersion === newVersion) return;
  if (stage.value === "downloading" || stage.value === "downloaded") return;
  version.value = newVersion;
  notes.value = newNotes;
  stage.value = "remind";
  dismissed.value = false;
  flowError.value = null;
  store.markUpdateReminded(newVersion);
}

/** 宿主挂载入口：探测更新入口形态（一次）并接事件订阅。 */
async function init(): Promise<void> {
  if (surface.value === null) {
    try {
      surface.value = await api.getUpdateSurface();
    } catch (e) {
      // 形态探测失败按 release-page 兜底（最小伤害：更新入口降级，不影响其余功能）。
      console.warn("查询更新入口形态失败", e);
      surface.value = "release-page";
    }
  }
  ensureEventSubscription();
}

/** 设置页手动检查（不受「已提醒版本」记忆限制）：结果就地呈现、失败就地报错。 */
async function checkManually(): Promise<void> {
  if (surface.value !== "updater" || manualCheck.value.kind === "checking") return;
  manualCheck.value = { kind: "checking" };
  try {
    const outcome = await api.checkForUpdates();
    if (outcome.status === "update" && outcome.version !== null) {
      manualCheck.value = {
        kind: "available",
        version: outcome.version,
        notes: outcome.notes,
      };
    } else {
      // 相同 / 更旧同为「已是最新」：旧版继续使用无强制升级门槛（ADR-0124 决策 5）。
      manualCheck.value = { kind: "latest" };
    }
  } catch (e) {
    manualCheck.value = { kind: "error", message: errorMessage(e) };
  }
}

/** 确认下载（提醒弹层或设置卡片入口）：后端先更新前备份后带进度下载，失败回提醒就地报错。
 *  空闲态入口（卡片手动检查的就地结果）先以检查结果播种流程载荷再进入下载。 */
async function startDownload(): Promise<void> {
  if (stage.value === "idle") {
    if (manualCheck.value.kind !== "available") return;
    version.value = manualCheck.value.version;
    notes.value = manualCheck.value.notes;
    stage.value = "remind";
  }
  if (stage.value !== "remind" || version.value === null) return;
  stage.value = "downloading";
  received.value = 0;
  total.value = null;
  flowError.value = null;
  try {
    const outcome = await api.downloadUpdate();
    stage.value = "downloaded";
    version.value = outcome.version;
  } catch (e) {
    // 失败回提醒阶段：版本与发布说明保留，可就地重试（等待成功优于即刻失败）。
    stage.value = "remind";
    flowError.value = errorMessage(e);
  }
}

/** 收起弹层：remind 阶段关闭即取消（不触发下载）；下载/已下载只收起弹层，流程继续。 */
function dismissFlow(): void {
  if (stage.value === "remind") {
    stage.value = "idle";
    version.value = null;
    notes.value = null;
    flowError.value = null;
    return;
  }
  dismissed.value = true;
}

/** 从设置卡片状态行重开弹层（下载中/已下载的再入口；不重复发起任何请求）。 */
function reopenFlow(): void {
  dismissed.value = false;
}

/** 确认安装（二次确认入口）：安装已下载工件并自动重启；失败就地报错、现场可重试。 */
async function confirmInstall(): Promise<void> {
  if (stage.value !== "downloaded") return;
  flowError.value = null;
  try {
    // 成功路径不返回（进程随安装重启）；失败放回暂存可重试（后端编排）。
    await api.installUpdate();
  } catch (e) {
    flowError.value = errorMessage(e);
  }
}

/** 打开发布页（deb 与 Android 的更新入口）：系统默认浏览器。 */
async function openReleasePage(): Promise<void> {
  try {
    await openUrl(RELEASE_PAGE_URL);
  } catch (e) {
    console.warn("打开发布页失败", e);
  }
}

/**
 * 测试隔离用：清空全部接缝状态与订阅登记（先例 resetFxSyncForTest）。
 * 生产代码不得调用。
 */
export function resetAppUpdateForTest(): void {
  surface.value = null;
  stage.value = "idle";
  version.value = null;
  notes.value = null;
  received.value = 0;
  total.value = null;
  dismissed.value = false;
  flowError.value = null;
  manualCheck.value = { kind: "idle" };
  subscribed = false;
}

/** 应用更新编排（模块级单例）：状态与动作全集，设置卡片与提醒弹层宿主共享消费。
 *  启动提醒入口（offerUpdate）仅由更新可用事件回调消费，不对外暴露；原始进度
 *  received/total 经 downloadPercent 派生后消费，同不暴露。 */
export function useAppUpdate() {
  return {
    // 状态（只读消费）
    surface,
    stage,
    version,
    notes,
    downloadPercent,
    modalOpen,
    flowError,
    manualCheck,
    // 动作
    init,
    checkManually,
    startDownload,
    dismissFlow,
    reopenFlow,
    confirmInstall,
    openReleasePage,
  };
}
