export type PassphraseStrengthTier = "weak" | "medium" | "strong" | "very-strong";

export interface PassphraseStrengthAssessment {
  score: 0 | 1 | 2 | 3 | 4;
  tier: PassphraseStrengthTier;
  percent: number;
}

type ZxcvbnScore = PassphraseStrengthAssessment["score"];

export function strengthForScore(score: ZxcvbnScore): PassphraseStrengthAssessment {
  const tier: PassphraseStrengthTier =
    score <= 1 ? "weak" : score === 2 ? "medium" : score === 3 ? "strong" : "very-strong";
  return { score, tier, percent: (score + 1) * 20 };
}

let checkerPromise: Promise<(password: string) => ZxcvbnScore> | null = null;

function loadChecker(): Promise<(password: string) => ZxcvbnScore> {
  checkerPromise ??= Promise.all([
    import("@zxcvbn-ts/core"),
    import("@zxcvbn-ts/language-common"),
  ]).then(([{ ZxcvbnFactory }, { dictionary, adjacencyGraphs }]) => {
    const factory = new ZxcvbnFactory({
      dictionary: { ...dictionary },
      graphs: adjacencyGraphs,
    });
    return (password: string) => factory.check(password).score;
  });
  return checkerPromise;
}

export async function assessPassphraseStrength(
  input: string,
): Promise<PassphraseStrengthAssessment | null> {
  if (!input) return null;
  const checker = await loadChecker();
  return strengthForScore(checker(input));
}
