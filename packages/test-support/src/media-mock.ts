export interface FakeMediaState {
  width: number;
  hover: "hover" | "none";
  pointer: "fine" | "coarse";
}

export const DEFAULT_MEDIA_STATE: FakeMediaState = {
  width: 1280,
  hover: "hover",
  pointer: "fine",
};

type MediaListener = (event: MediaQueryListEvent) => void;

interface RegisteredMql {
  media: string;
  matches: boolean;
  onchange: MediaListener | null;
  listeners: Set<MediaListener>;
}

let state: FakeMediaState = { ...DEFAULT_MEDIA_STATE };
let registry: RegisteredMql[] = [];

function evaluateFeature(feature: string, s: FakeMediaState): boolean | undefined {
  const m = /^\(\s*([\w-]+)\s*:\s*([^)]+?)\s*\)$/.exec(feature);
  if (!m) return undefined;
  const name = m[1];
  const value = m[2];
  switch (name) {
    case "min-width":
      return s.width >= Number.parseFloat(value);
    case "max-width":
      return s.width <= Number.parseFloat(value);
    case "hover":
      return value === s.hover;
    case "pointer":
      return value === s.pointer;
    default:
      return undefined;
  }
}

function evaluateQuery(query: string, s: FakeMediaState): boolean | undefined {
  const q = query.trim();
  if (q.startsWith("not ")) {
    const inner = evaluateQuery(q.slice(4), s);
    return inner === undefined ? undefined : !inner;
  }
  let known = false;
  for (const part of q.split(/\s+and\s+/)) {
    const result = evaluateFeature(part.trim(), s);
    if (result === false) return false;
    if (result === true) known = true;
  }
  return known;
}

function toListener(listener: EventListenerOrEventListenerObject | null): MediaListener | null {
  if (!listener) return null;
  if (typeof listener === "function") return listener;
  return (e: MediaQueryListEvent) => listener.handleEvent(e);
}

export function fakeMatchMedia(query: string): MediaQueryList {
  const listeners = new Set<MediaListener>();
  const entry: RegisteredMql = {
    media: query,
    matches: evaluateQuery(query, state) === true,
    onchange: null,
    listeners,
  };
  const mql = {
    get matches() {
      return entry.matches;
    },
    media: query,
    get onchange() {
      return entry.onchange;
    },
    set onchange(fn: MediaListener | null) {
      entry.onchange = fn;
    },
    addEventListener: (_type: string, listener: EventListenerOrEventListenerObject | null) => {
      const fn = toListener(listener);
      if (fn) listeners.add(fn);
    },
    removeEventListener: (_type: string, listener: EventListenerOrEventListenerObject | null) => {
      const fn = toListener(listener);
      if (fn) listeners.delete(fn);
    },
    addListener: (listener: MediaListener) => {
      listeners.add(listener);
    },
    removeListener: (listener: MediaListener) => {
      listeners.delete(listener);
    },
    dispatchEvent: () => false,
  };
  registry.push(entry);
  return mql as unknown as MediaQueryList;
}

export function setFakeMedia(patch: Partial<FakeMediaState>): void {
  state = { ...state, ...patch };
  for (const entry of registry) {
    const next = evaluateQuery(entry.media, state) === true;
    if (next === entry.matches) continue;
    entry.matches = next;
    const event = { matches: next, media: entry.media } as MediaQueryListEvent;
    if (entry.onchange) entry.onchange(event);
    for (const listener of entry.listeners) listener(event);
  }
}

export function resetFakeMedia(): void {
  state = { ...DEFAULT_MEDIA_STATE };
  registry = [];
}

export function fakeMediaListenerCount(query: string): number {
  return registry.reduce((n, entry) => (entry.media === query ? n + entry.listeners.size : n), 0);
}
