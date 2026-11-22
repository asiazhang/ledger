export const CONCEPT_KEYS = [
  "marketValue",
  "unrealizedPnl",
  "cumulativePnl",
  "cost",
  "price",
  "mwr",
  "mwrCumulative",
  "realizedPnl",
  "realizedGain",
  "portfolioTrend",
  "investableAssets",
] as const;

export type ConceptKey = (typeof CONCEPT_KEYS)[number];

const CONCEPT_SCOPES = ["filtered", "wholeLedger", "overview", "crossBook"] as const;

export type ConceptScope = (typeof CONCEPT_SCOPES)[number];

export const CONCEPT_SCOPE_KEY_SUFFIX: Record<ConceptScope, string> = {
  filtered: "Filtered",
  wholeLedger: "WholeLedger",
  overview: "Overview",
  crossBook: "CrossBook",
};
