export interface LatestWinsToken {
  isStale(): boolean;
}

export interface LatestWins {
  begin(): LatestWinsToken;
  observe(): LatestWinsToken;
  invalidate(): void;
}

export function createLatestWins(): LatestWins {
  let epoch = 0;

  const bind = (): LatestWinsToken => {
    const mine = epoch;
    return { isStale: () => mine !== epoch };
  };

  return {
    begin(): LatestWinsToken {
      epoch += 1;
      return bind();
    },
    observe: bind,
    invalidate(): void {
      epoch += 1;
    },
  };
}
