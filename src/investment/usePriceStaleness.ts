import { onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import { usePricesChanged } from "@/investment/usePricesChanged";

/**
 * 价格过期检查（issue #1190）：打开投资页时的**本地水位检查**接缝——调用后端
 * 只读本地库的检查命令（零网络请求），产出过期标的总数与判定阈值，供页面提示
 * 「价格可能已过期」并导向既有「同步标的信息」入口。
 *
 * 口径不在这里：过期与否、阈值多少全归后端投资域单点（水位 = 现价缓存的行情
 * 采集时刻 / 净值日期），本接缝只搬运计数——前端不按类型与市场自行推断
 * （先例：价格通道判定单点在后端，ADR-0031 的信号消费方只重拉自身数据）。
 * 计数面是全库有通道标的（含已清仓，已清仓且水位陈旧超过一年的行退出，
 * #1663），故提示措辞只陈述标的自身、不宣称报表影响（报表与净资产只吃
 * 持仓）——见词汇表「价格过期提示」。
 *
 * 刷新时机两处：挂载一次（「打开投资页时做一次」），以及价格失效信号
 * `ledger:prices-changed` 后重查——同步实际写价后提示随价格变新自动消失，
 * 不需要调用方记得手动收提示（ADR-0031）。
 *
 * 失败静默降级为「不提示」：检查是背景提示，不是阻塞路径；发起经 Loadable
 * 静默实例（ADR-0040 决策 3 修订注），网络/命令异常既不弹 toast 也不留半态。
 */
export function usePriceStaleness() {
  /** 需要同步的标的数（0 = 不提示；失败降级亦为 0） */
  const staleCount = ref(0);
  /** 后端判定阈值（自然日），供提示文案引用——前端不另抄一份天数常量 */
  const thresholdDays = ref(0);

  const { run } = useLoadable(() => api.instrumentPriceStaleness(), { silent: true });

  async function refresh(): Promise<void> {
    const result = await run();
    // 失败（result 为 null）按「无过期」处置：宁可不提示，也不误报
    staleCount.value = result?.stale_count ?? 0;
    thresholdDays.value = result?.threshold_days ?? 0;
  }

  onMounted(() => {
    void refresh();
  });
  usePricesChanged(() => {
    void refresh();
  });

  return { staleCount, thresholdDays, refresh };
}
