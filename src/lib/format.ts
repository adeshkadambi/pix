// Display formatting shared by the companion screens.

const cap = (s: string) => s.charAt(0) + s.slice(1).toLowerCase();

/** "SILVER", "III" → "Silver III"; "MASTER", "" → "Master". */
export function tierLabel(tier: string, division: string): string {
  return division ? `${cap(tier)} ${division}` : cap(tier);
}

export function winRate(wins: number, games: number): string {
  return games > 0 ? `${Math.round((wins / games) * 100)}%` : "–";
}

export function kdaRatio(kills: number, deaths: number, assists: number): string {
  return deaths === 0 ? "Perfect" : ((kills + assists) / deaths).toFixed(2);
}

export function duration(secs: number): string {
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
}

export function perMinute(count: number, secs: number): string {
  return secs > 0 ? (count / (secs / 60)).toFixed(1) : "0.0";
}

/** Relative time like "just now", "5 min ago", "3h ago", "12d ago". */
export function timeAgo(ms: number, now: number): string {
  const s = Math.max(0, Math.floor((now - ms) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

/** Up to two letters, used when an icon is missing. */
export function initials(name: string): string {
  return name.replace(/[^A-Za-z ]/g, "").split(/\s+/).filter(Boolean).map((w) => w[0]).join("").slice(0, 2).toUpperCase() ||
    name.slice(0, 2).toUpperCase();
}
