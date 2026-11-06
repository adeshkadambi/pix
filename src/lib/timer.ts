/** Gold amount, compact: "850", "7.8K". */
export function goldK(n: number): string {
  return n < 1000 ? String(n) : `${(n / 1000).toFixed(1)}K`;
}

/** Signed gold difference, compact: "+850", "−1.0K", "±0". */
export function goldDiff(n: number): string {
  if (n === 0) return "±0";
  return `${n > 0 ? "+" : "−"}${goldK(Math.abs(n))}`;
}

/** Countdown text: "4:37" from a minute up, "42" below. */
export function countdown(secs: number): string {
  const s = Math.max(0, Math.ceil(secs));
  return s >= 60 ? `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}` : String(s);
}
