import { reactive } from "vue";

interface OverlayToken {
  readonly name: string;
  open: boolean;
  readonly requestClose: (() => boolean) | null;
}

const openOverlays = reactive<OverlayToken[]>([]);

export type OverlayCloseRequest = () => boolean;

export interface OverlayTokenHandle {
  readonly name: string;
  set(open: boolean): void;
}

export function createOverlayToken(
  name: string,
  requestClose?: OverlayCloseRequest,
): OverlayTokenHandle {
  const token: OverlayToken = { name, open: false, requestClose: requestClose ?? null };
  return {
    name,
    set(next: boolean) {
      if (token.open === next) return;
      token.open = next;
      if (next) openOverlays.push(token);
      else openOverlays.splice(openOverlays.indexOf(token), 1);
    },
  };
}

export function hasOpenOverlay(): boolean {
  return openOverlays.length > 0;
}

export function openOverlayNames(): string[] {
  return openOverlays.map((t) => t.name);
}

export function closeTopOverlay(): boolean {
  const top = openOverlays[openOverlays.length - 1];
  if (!top || !top.requestClose) return false;
  return top.requestClose();
}

export function resetOverlays(): void {
  openOverlays.length = 0;
}
