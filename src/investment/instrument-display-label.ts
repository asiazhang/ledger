/**
 * 标的展示名拼法单点（空格形，issue #1839）：代码 + 名称空格连接，无名称
 * （含空串）退化为裸代码——与后端 InstrumentSourceDisplay::display_label
 * 同一口径（InstrumentSourceDisplay::display_label，crates/investment/src/model.rs；
 * 主列表来源列标的目标反查为同款拼法的另一站点，read/source.rs ④）。
 *
 * 消费面：明细页签来源列、交易弹窗族行投影（source.display_name）、
 * 走势页签下拉候选与曲线标签。
 * 「代码 · 名称」下拉候选拼法（useInstrumentOptions.instrumentOptionLabel）
 * 是另一口径，各有单点、不互串。
 */
export function instrumentDisplayLabel(symbol: string, name: string | null): string {
  return name ? `${symbol} ${name}` : symbol;
}
