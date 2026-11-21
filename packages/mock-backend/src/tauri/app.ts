interface BackPressPayload {
  canGoBack: boolean;
}

interface MockPluginListener {
  unregister(): Promise<void>;
}

export function onBackButtonPress(
  _handler: (payload: BackPressPayload) => void,
): Promise<MockPluginListener> {
  return Promise.resolve({ unregister: () => Promise.resolve() });
}
