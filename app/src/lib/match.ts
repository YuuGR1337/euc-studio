// Command palette matching: every typed word must start a word of the command's title or
// keywords. Title hits rank above keyword hits. Fuzzy letter matching ranked "Open component
// gallery" first for "accent green", so the palette uses this instead.

const words = (text: string) =>
  text
    .toLowerCase()
    .split(/[^\p{L}\p{N}%]+/u)
    .filter(Boolean);

/** 0 = no match; higher is better (at most 1). */
export function matchCommand(search: string, title: string, keywords: string[] = []): number {
  const tokens = words(search);
  if (tokens.length === 0) return 1;
  const titleWords = words(title);
  const keywordWords = keywords.flatMap(words);
  let score = 0;
  for (const t of tokens) {
    if (titleWords.some((w) => w.startsWith(t))) score += 2;
    else if (keywordWords.some((w) => w.startsWith(t))) score += 1;
    else return 0;
  }
  const bonus = titleWords[0]?.startsWith(tokens[0] ?? "") ? 0.5 : 0;
  return (score + bonus) / (tokens.length * 2 + 0.5);
}
