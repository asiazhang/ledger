// Chart.js 统一注册模块（issue #926）：全仓图表所需的 controller / element /
// scale 在此一处注册。背景：Chart.js 的 register 是全局副作用，历史实现由各
// 图表组件自行注册各自子集、靠「其他组件恰好补齐」互相罩住——走势折线图缺
// LineElement/PointElement，图表带数据渲染时在构造/动画帧中抛
// "point" is not a registered element，错误随帧循环打断 Vue 调度器，
// 整个界面冻结（词汇表「Loadable」错误通道只覆盖 IPC 错误，渲染层错误此前
// 只进 console，用户不可见）。
//
// 消费约定：图表组件一律 `import '@/utils/chart-registration'`（或显式导入
// 本模块）后即用，不再各自 register；新增图表类型时在此扩注册，注册表由
// portfolio-trend-chart-registration.test.ts 按真实 registry 守门。
import {
  BarController,
  BarElement,
  CategoryScale,
  Chart as ChartJS,
  LineController,
  LineElement,
  LinearScale,
  PointElement,
  Tooltip,
  Legend,
  Title,
} from 'chart.js'

ChartJS.register(
  // 图表类型（controller）
  LineController,
  BarController,
  // 数据元素
  LineElement,
  PointElement,
  BarElement,
  // 坐标轴
  CategoryScale,
  LinearScale,
  // 插件
  Tooltip,
  Legend,
  Title,
)
