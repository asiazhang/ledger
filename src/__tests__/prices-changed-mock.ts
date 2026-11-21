const state = {
  handlers: [] as (() => void)[],
};

export function capturePricesChangedHandler(cb: () => void): void {
  state.handlers.push(cb);
}

export function firePricesChanged(): void {
  if (state.handlers.length === 0) {
    throw new Error(
      "usePricesChanged 未被订阅：先在 vi.mock 工厂中接好 capturePricesChangedHandler",
    );
  }
  for (const handler of state.handlers) {
    handler();
  }
}

export function resetPricesChangedHandler(): void {
  state.handlers = [];
}
