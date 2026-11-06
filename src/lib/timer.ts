/** Signed gold, compact: "+850", "−1.2k", "±0". */
export function goldDiff(n: number): string {
  if (n === 0) return "±0";
  const sign = n > 0 ? "+" : "−";
  const a = Math.abs(n);
  return a < 1000 ? `${sign}${a}` : `${sign}${(a / 1000).toFixed(1)}k`;
}

/** Countdown text: "4:37" from a minute up, "42" below. */
export function countdown(secs: number): string {
  const s = Math.max(0, Math.ceil(secs));
  return s >= 60 ? `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}` : String(s);
}
