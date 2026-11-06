/** Countdown text: "4:37" from a minute up, "42" below. */
export function countdown(secs: number): string {
  const s = Math.max(0, Math.ceil(secs));
  return s >= 60 ? `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}` : String(s);
}
