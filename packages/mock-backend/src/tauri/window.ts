interface MockWindow {
  setTitle(title: string): Promise<void>;
  destroy(): Promise<void>;
}

export function getCurrentWindow(): MockWindow {
  return {
    setTitle: (_title: string) => Promise.resolve(),
    destroy: () => Promise.resolve(),
  };
}
