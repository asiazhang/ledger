import { computed, onMounted, ref, watch, type Ref } from "vue";
import { useMessage } from "naive-ui";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { judgeMinLengthText } from "@ledger/utils/field-error";
import {
  assessPassphraseStrength,
  type PassphraseStrengthAssessment,
} from "@ledger/utils/passphrase-strength";
import { useLoadable } from "@ledger/loadable";
import { restartAppShortly } from "@/backup/restart";
import { useAppStore } from "@/stores/app";
import { useEncryptionGate } from "@/backup/useEncryptionGate";
import type { EncryptionStatus } from "@ledger/types";

/**
 * useEncryptionTransitions——加密形态转换编排深模块（issue #1396 / #1306 结论）：
 * 加密模式三条用户发起转换（开启 / 修改主口令 / 关闭，域概念见备份与数据文件域
 * 词汇表「形态转换」）加上自动解锁启用小弹窗的完整编排——确认弹窗开合时序
 * （request 门槛 → show、confirm 先关再执行）、口令字段与字段错误态、口令强度
 * 实时显示、口令缓存同步、状态加载与首刷、busy 状态、成功/失败提示与重启触发。
 * 消费方 EncryptionSettings（设置页签面，留 src/settings/）是它的薄 adapter：
 * 只留四个 AppDangerConfirmModal / AppModal 的模板与 i18n 文案 props（「确认弹层
 * 文案留调用方」，ADR-0078）。
 *
 * 先例：ScheduledPlanList（ADR-0041 清单编排收为深模块、页签薄适配）、
 * useBookSwitcher / useRestoreFromFile（深模块 + 薄 adapter；ReturnType 导出面
 * 与 onMounted 首刷内化沿其形态）。留壳判据（ADR-0118 决策 1）：依赖
 * useEncryptionGate 模块级单例与 app store，不成包、随备份与数据文件域住
 * src/backup/（与 useEncryptionGate / restart 同区）。
 *
 * 行为等价义务（#1396，迁移自 EncryptionSettings.vue 逐字等价）：
 * - 确认弹窗**先关**再执行（确认弹窗不是重试现场——「形态转换」词条边界）；
 *   失败 toast 后表单字段保留，可修改后重新发起。
 * - 失败 toast 全是裸 errorMessage（收 Loadable 默认策略，ADR-0040）；本模块
 *   不动 TOAST_BASELINE（裸 errorMessage 不在守门基线内）。
 * - 缓存同步失败不阻断重启，仅 warning 提示未记住；关闭加密先清缓存再 toast
 *   再重启。
 * - 自动解锁确认失败**不弹 toast**：silent 实例 + error 就地渲染弹窗错误位；
 *   成功路径 success toast 直调保持。
 * - 重启时机不变：toast 先落地，restartAppShortly 延迟重启（ADR-0080；800ms
 *   归 restart.ts，本模块只触发）。
 */

/** 主口令最小长度（issue #650）：≥8，仅前端判定（后端契约不动）；走字段错误态
 *  既有口径（ADR-0058）：短口令即时红显、提交禁用，不拦截键入。组件的错误反馈
 *  文案（tooShort 插值）与模块的红显判定共用本常量，单点收口。 */
export const PASSPHRASE_MIN_LENGTH = 8;

/** 口令强度实时显示（issue #685，词汇表「口令强度」）：纯信息反馈，不拦截提交、
 *  不改提交可用性；只接新设主口令两框（开启加密「主口令」+ 修改主口令「新主口令」），
 *  确认字段与已存在口令的输入场景一律不接。判定与映射收口在
 *  @ledger/utils/passphrase-strength，此处只消费（最后一次胜出守卫保证逐键刷新不串档）。 */
function trackPassphraseStrength(source: Ref<string>) {
  const assessment = ref<PassphraseStrengthAssessment | null>(null);
  let latest = 0;
  watch(source, (value) => {
    const seq = ++latest;
    void assessPassphraseStrength(value).then((result) => {
      if (seq === latest) assessment.value = result;
    });
  });
  return assessment;
}

export function useEncryptionTransitions() {
  const message = useMessage();
  const store = useAppStore();
  const { rememberSupport, loadRememberSupport, syncRememberCache, clearRememberCache } =
    useEncryptionGate();

  // —— 状态加载（加密模式是文件属性，设置页卡片按探测结果分支形态） ——
  // 收 Loadable（ADR-0040）：loading/error 置收、裸 errorMessage toast（默认策略，
  // 与现状逐字等价）内化；error 位渲染错误位 + 重试按钮，status 只在成功后落位。
  const status = ref<EncryptionStatus | null>(null);
  const {
    loading: statusLoading,
    error: statusError,
    run: runStatusLoad,
  } = useLoadable(() => api.getEncryptionStatus());

  /** 拉取加密状态（onMounted 首刷 + 错误位重试按钮共用；useBookSwitcher 先例）。 */
  async function refresh(): Promise<void> {
    const next = await runStatusLoad();
    if (next !== null) status.value = next;
  }

  // —— 开启流（明文库形态）——
  const passphrase = ref("");
  const confirmPassphrase = ref("");
  const enableRemember = ref(false);
  const passphraseStrength = trackPassphraseStrength(passphrase);

  /** 新设主口令过短（字段错误态：格式类即时红，空值不在此列、走既有禁用逻辑）。 */
  const passphraseTooShort = computed(
    () => judgeMinLengthText(passphrase.value, PASSPHRASE_MIN_LENGTH).kind === "too-short",
  );

  /** 两次输入一致才允许提交（确认输错的即时反馈）。 */
  const mismatch = computed(
    () => confirmPassphrase.value.length > 0 && confirmPassphrase.value !== passphrase.value,
  );

  // 开启加密确认弹窗（issue #650 / ADR-0078）：error 级，承载无后门后果说明
  // （忘记主口令 = 数据不可恢复）。show 状态归模块，弹窗模板与文案留组件。
  const enableConfirmShow = ref(false);

  // busy 归零（issue #1396）：单向「发起→终态」动作的 busy 全部由 Loadable loading
  // 表达，不设独立闩锁 ref；重入守卫 = 动作入口 if (xLoading.value) return。
  const { loading: submitting, run: runEnable } = useLoadable(async () => {
    await api.enableEncryption(passphrase.value);
    const cached = await syncRememberCache(passphrase.value, enableRemember.value);
    if (!cached) message.warning(t("settings.data.encryption.rememberFailed"));
    passphrase.value = "";
    confirmPassphrase.value = "";
    message.success(t("settings.data.encryption.okToast"));
    // 转换已落盘，重启以凭新口令重新打开（toast 先落地，Restore 先例）。
    restartAppShortly();
  });

  /** 开启加密第一步：弹确认弹窗（无后门后果说明），确认后执行整库转换。 */
  function requestEnable() {
    if (submitting.value || mismatch.value || !passphrase.value || passphraseTooShort.value) return;
    enableConfirmShow.value = true;
  }

  /** 开启加密确认：先关弹窗再执行（确认弹窗不是重试现场）——转换 → 提示重启。 */
  async function confirmEnable() {
    enableConfirmShow.value = false;
    if (submitting.value || mismatch.value || !passphrase.value) return;
    await runEnable();
  }

  // —— 修改主口令流（已加密形态）：旧口令验证 + 新口令（含确认、须不同于旧口令）——
  const changeOld = ref("");
  const changeNew = ref("");
  const changeConfirm = ref("");
  // 「记住」勾选初值随当前偏好（与开启表单的 enableRemember 不同——轮换前可能已启用）。
  const changeRemember = ref(store.rememberPassphrase);
  const changeNewStrength = trackPassphraseStrength(changeNew);
  const changeMismatch = computed(
    () => changeConfirm.value.length > 0 && changeConfirm.value !== changeNew.value,
  );
  const changeUnchanged = computed(
    () => changeNew.value.length > 0 && changeNew.value === changeOld.value,
  );
  /** 轮换后的新主口令同样受最小长度约束（issue #650），不弱于初始要求。 */
  const changeNewTooShort = computed(
    () => judgeMinLengthText(changeNew.value, PASSPHRASE_MIN_LENGTH).kind === "too-short",
  );
  const changeReady = computed(
    () =>
      changeOld.value.length > 0 &&
      changeNew.value.length > 0 &&
      !changeMismatch.value &&
      !changeUnchanged.value &&
      !changeNewTooShort.value,
  );

  // 修改主口令确认弹窗（issue #650 / ADR-0078）：error 级——与开启加密风险同级
  // （遗忘新口令同样数据不可读）。
  const changeConfirmShow = ref(false);

  const { loading: submittingChange, run: runChange } = useLoadable(async () => {
    await api.changeEncryptionPassphrase(changeOld.value, changeNew.value);
    const cached = await syncRememberCache(changeNew.value, changeRemember.value);
    if (!cached) message.warning(t("settings.data.encryption.rememberFailed"));
    changeOld.value = "";
    changeNew.value = "";
    changeConfirm.value = "";
    message.success(t("settings.data.encryption.changeOkToast"));
    restartAppShortly();
  });

  /** 修改主口令第一步：弹 error 级确认弹窗（ADR-0078），确认后执行转换。 */
  function requestChange() {
    if (submittingChange.value || !changeReady.value) return;
    changeConfirmShow.value = true;
  }

  /** 修改主口令确认：先关弹窗再执行——旧口令验证通过后转入新口令的新库，完成后重启。 */
  async function confirmChange() {
    changeConfirmShow.value = false;
    if (submittingChange.value || !changeReady.value) return;
    await runChange();
  }

  // —— 关闭加密流（已加密形态）：需当前主口令——文件级转换凭口令读取密文库——
  const disablePassphrase = ref("");

  // 关闭加密确认弹窗（issue #652 / ADR-0078）：warning 级——破坏性但有兜底
  // （既有密文副本保留、可再开启）。
  const disableConfirmShow = ref(false);

  const { loading: submittingDisable, run: runDisable } = useLoadable(async () => {
    await api.disableEncryption(disablePassphrase.value);
    await clearRememberCache();
    disablePassphrase.value = "";
    message.success(t("settings.data.encryption.disableOkToast"));
    restartAppShortly();
  });

  /** 关闭加密第一步：弹 warning 级确认弹窗（兜底说明，ADR-0078），确认后执行转换。 */
  function requestDisable() {
    if (submittingDisable.value || !disablePassphrase.value) return;
    disableConfirmShow.value = true;
  }

  /** 关闭加密确认：先关弹窗再执行——整库转回明文库、清缓存后重启，不再出现解锁屏。 */
  async function confirmDisable() {
    disableConfirmShow.value = false;
    if (submittingDisable.value || !disablePassphrase.value) return;
    await runDisable();
  }

  // —— 自动解锁（issue #654 重做）：状态唯一事实源 = store.rememberPassphrase（偏好与
  // 钥匙串缓存同批建立/清除，无本地开关镜像）。「开关开着但未生效」的可持续中间态
  // 从形态上消灭：启用 = 「启用自动解锁…」按钮弹小窗，凭当前主口令建立缓存、成功才
  // 置偏好；关闭 = 立即清缓存恢复手输并提示。 ——
  /** 自动解锁是否已启用（唯一事实源 = 偏好，与钥匙串缓存同批建立/清除）。 */
  const autoUnlockOn = computed(() => store.rememberPassphrase);

  const autoUnlockModalShow = ref(false);
  const autoUnlockPass = ref("");
  // 确认失败不弹 toast（silent 实例，ADR-0040 静默 opt-out）：error 就地渲染弹窗
  // 错误位，弹窗保持打开可就地重试——口令错误可重试是本弹窗的交互本体。
  const {
    loading: autoUnlockSubmitting,
    error: autoUnlockError,
    run: runAutoUnlock,
  } = useLoadable(
    async () => {
      await api.setRememberPassphrase(autoUnlockPass.value);
      store.setRememberPassphrase(true);
      autoUnlockModalShow.value = false;
      autoUnlockPass.value = "";
      message.success(t("settings.data.encryption.rememberEnabled"));
    },
    { silent: true },
  );

  /** 启用自动解锁第一步：打开小弹窗（清上次输入与错误）。 */
  function openAutoUnlockModal() {
    autoUnlockPass.value = "";
    autoUnlockError.value = null;
    autoUnlockModalShow.value = true;
  }

  /** 启用自动解锁确认：凭当前主口令建立缓存，成功才置偏好并提示；
   *  口令错误就地报错（弹窗保持打开可重试）、不启用——不存在中间态。 */
  async function confirmAutoUnlock() {
    if (autoUnlockSubmitting.value || !autoUnlockPass.value) return;
    await runAutoUnlock();
  }

  /** 关闭自动解锁：立即清缓存恢复手输并提示（清缓存幂等，无失败悬挂态）。 */
  async function disableAutoUnlock() {
    await clearRememberCache();
    message.success(t("settings.data.encryption.rememberDisabledToast"));
  }

  // 首刷内化（useBookSwitcher 先例）：状态探测 + 自动解锁平台能力懒加载。
  onMounted(() => {
    void refresh();
    void loadRememberSupport();
  });

  return {
    // 状态加载
    status,
    statusLoading,
    statusError,
    refresh,
    // 开启流
    passphrase,
    confirmPassphrase,
    enableRemember,
    passphraseStrength,
    passphraseTooShort,
    mismatch,
    enableConfirmShow,
    requestEnable,
    confirmEnable,
    submitting,
    // 修改流
    changeOld,
    changeNew,
    changeConfirm,
    changeRemember,
    changeNewStrength,
    changeMismatch,
    changeUnchanged,
    changeNewTooShort,
    changeReady,
    changeConfirmShow,
    requestChange,
    confirmChange,
    submittingChange,
    // 关闭流
    disablePassphrase,
    disableConfirmShow,
    requestDisable,
    confirmDisable,
    submittingDisable,
    // 自动解锁
    rememberSupport,
    autoUnlockOn,
    autoUnlockModalShow,
    autoUnlockPass,
    autoUnlockError,
    autoUnlockSubmitting,
    openAutoUnlockModal,
    confirmAutoUnlock,
    disableAutoUnlock,
  };
}

export type EncryptionTransitions = ReturnType<typeof useEncryptionTransitions>;
