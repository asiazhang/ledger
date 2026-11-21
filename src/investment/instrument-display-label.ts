export function instrumentDisplayLabel(symbol: string, name: string | null): string {
  return name ? `${symbol} ${name}` : symbol;
}
