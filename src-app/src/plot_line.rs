//! line 图封装件（`App*` 封装族，issue #2045 走势卡首建；ADR-0145 决策 5：
//! line 用 NSH 内建 plot——本件是封装层落点，视图不直连框架基元，ADR-0118
//! 纪律的 NSH 延续）。
//!
//! 形态（按本票首建消费面建形，消费面外扩再行开放——just-in-time 纪律）：
//! - **时间序列单线**：x 轴点式刻度（采样点序均匀放置，约 6 处均匀抽点标周
//!   一日期），y 轴线性自量域（刻度文本经调用方给的金额格式化闭包——口径
//!   单源 [`crate::assembly`]）；
//! - **缺值点跨越**（旧壳 chart.js `spanGaps` 同语义）：值为 `None` 的采样点
//!   不出点、连线跨过（NSH plot「无值的行被跳过」的内置语义直消费）。
//!
//! 输入纪律：规格面（[`PlotLineSeries`]）由调用方落闭包（视图装载产物），
//! 本件只消费——本件零统计语义、零金额换算（y 轴文本归调用方闭包）。旧壳
//! 走势图的 pointer 逐点 tooltip 在本壳无对等物：桌面引擎无 hover 步骤
//! （nsh-testing 能力盘点 §1.4），本件不带 tooltip（缺口回写父 spec，人工
//! 对表子集记录）。
//!
//! 色彩（issue #2100 收编）：走势主色走主题令牌 `line`（亮暗同值，表内
//! 单值档），网格线走 `border` 令牌降透明——本模块零本地色常量，读取经
//! [`crate::theme::color`] 单点。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::plot::{AxisPlacement, Curve, TickFormat, area, grid, line, plot, plot_axis};
use nsh_core::element::{RowAccessor, Rows};
use nsh_core::prelude::*;

/// 点式 x 轴的抽点数（约数；采样点数不超抽点数时逐点标注——旧壳
/// maxTicksLimit 12 的量级收紧，观感归人工对表子集）。
const X_TICK_COUNT: usize = 6;

/// line 图规格面（调用方装载产物）：每采样点的 x 标签与 y 值；等长两列，
/// `values[i]` 为 `None` = 该周缺价（缺值点跨越语义，见模块头注）。y 值以
/// 本位币「分」为量纲（f64 承载——|分| < 2⁵³ 内精确，与旧壳 chart.js 同
/// 浮点呈现面）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlotLineSeries {
    /// 每采样点的 x 刻度文本（ISO 周一日期；调用方持词表格式）。
    pub labels: Vec<String>,
    /// 每采样点的值（分）；`None` = 缺值点。
    pub values: Vec<Option<f64>>,
}

impl PlotLineSeries {
    /// 采样点数（两列等长以少者计——装配层保证等长，此为越界兜底）。
    pub fn len(&self) -> usize {
        self.labels.len().min(self.values.len())
    }

    /// 无采样点（空态判定的 wrapper 入参面）。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 线图（单线 + 底部日期刻度 + 左侧金额刻度 + 水平网格线）：
/// `series` 是规格面来源（调用方装载产物信号/闭包），`height` 是图区高
/// （pt），`format_y` 是 y 轴刻度的文本化闭包（金额口径单源调用方）。
///
/// 空序列不出图（调用方空态门控在场；本件兜底呈现空图分板，不出 marks 与
/// 旧壳空 series 渲染同果）。
pub fn plot_line(
    series: impl Fn() -> PlotLineSeries + Copy + 'static,
    height: f32,
    format_y: impl Fn(f64) -> String + Copy + 'static,
) -> impl IntoElement {
    // 行集构造（渲染期零格式化纪律的图形面例外：行集即装载产物逐点搬运，
    // 无文本格式化——格式化只在刻度闭包渲染期执行一次 per tick）。
    let rows = move || {
        let series = series();
        let len = series.len();
        let mut columns: Vec<f64> = Vec::with_capacity(len);
        for i in 0..len {
            columns.push(series.values[i].unwrap_or(f64::NAN));
        }
        Rows::from_columns(len, [("value".into(), columns)])
    };

    // x 轴抽点值（点式刻度：约 6 处均匀抽点，首尾必有）与刻度文本（row
    // 上的周日期）。采样点少时逐点标注。
    let x_label_of = move |row: f64| {
        series()
            .labels
            .get(row as usize)
            .cloned()
            .unwrap_or_default()
    };
    let x_ticks = move || {
        let len = series().len();
        if len == 0 {
            return Vec::<f64>::new();
        }
        if len <= X_TICK_COUNT {
            return (0..len).map(|i| i as f64).collect();
        }
        let last = len - 1;
        (0..X_TICK_COUNT)
            .map(|i| (i * last / (X_TICK_COUNT - 1)) as f64)
            .collect::<Vec<_>>()
    };

    // 走势主色（主题令牌 `line`，亮暗同值）：线实色、底面积同色 15% 透明
    // （旧壳 rgba(32,128,240,0.15) 的透明度同口径）；网格线走 `border` 令牌
    // 降透明（旧壳 20% 灰的视觉重量同量级）。令牌解析在 UI 构建期（引擎
    // 线程）进行，缺失统一落中灰可读兜底。
    let line_color = crate::theme::color("line");
    let grid_color = crate::theme::color("border").opacity(0.6);

    view().w_full().h(px(height)).child(
        plot()
            .data(rows)
            .y(RowAccessor::key("value"))
            .accessibility_label("组合市值走势")
            .cross_line_class("w-px bg-border")
            .dot_class("size-1 rounded-full bg-foreground")
            .child(grid().stroke_color(grid_color))
            .child(
                area()
                    .curve(Curve::Natural)
                    .fill_color(line_color.opacity(0.15)),
            )
            .child(
                line()
                    .curve(Curve::Natural)
                    .stroke_width(px(2.0))
                    .stroke_color(line_color),
            )
            .child(
                plot_axis()
                    .placement(AxisPlacement::Bottom)
                    .tick_values(x_ticks)
                    .format(Some(TickFormat::from_fn(move |row, _| {
                        x_label_of(row).into()
                    })))
                    .label_class("text-muted-foreground"),
            )
            .child(
                plot_axis()
                    .placement(AxisPlacement::Left)
                    .ticks(Some(5))
                    .format(Some(TickFormat::from_fn(move |value, _| {
                        format_y(value).into()
                    })))
                    .label_class("text-muted-foreground"),
            ),
    )
}
