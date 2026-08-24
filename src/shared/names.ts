export type NameIssue = "length" | "duplicate";

export function validateEntityName(
  value: string,
  existing: ReadonlyArray<{ id: string; name: string }>,
  currentId?: string,
): NameIssue | undefined {
  const trimmed = value.trim();
  if ([...trimmed].length < 1 || [...trimmed].length > 64) return "length";
  const normalized = trimmed.toLowerCase();
  if (
    existing.some((item) => item.id !== currentId && item.name.trim().toLowerCase() === normalized)
  )
    return "duplicate";
  return undefined;
}

export function uniqueCopyName(
  source: string,
  suffix: string,
  existing: ReadonlyArray<{ name: string }>,
): string {
  const names = new Set(existing.map((item) => item.name.trim().toLowerCase()));
  const base = `${source.trim()} ${suffix}`.trim();
  if (!names.has(base.toLowerCase()) && [...base].length <= 64) return base;
  for (let index = 2; index < 10_000; index += 1) {
    const ending = ` ${suffix} ${index}`;
    const available = Math.max(1, 64 - [...ending].length);
    const candidate = `${[...source.trim()].slice(0, available).join("")}${ending}`;
    if (!names.has(candidate.toLowerCase())) return candidate;
  }
  return [...crypto.randomUUID()].slice(0, 64).join("");
}
