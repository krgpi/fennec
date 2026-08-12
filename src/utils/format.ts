export function formatTime(t: number): string {
  const total = Math.floor(t);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) {
    return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

export function formatSessionDate(iso: string): string {
  const d = new Date(iso);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}/${pad(d.getMonth() + 1)}/${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function relativeDateLabel(iso: string, lang: string): string {
  const d = new Date(iso);
  const now = new Date();
  const startOfDay = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate());
  const days = Math.round(
    (startOfDay(now).getTime() - startOfDay(d).getTime()) / 86400000,
  );
  const pad = (n: number) => String(n).padStart(2, "0");
  if (days === 0) return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  if (days === 1) return lang === "en" ? "Yesterday" : "昨日";
  if (days < 7) {
    return d.toLocaleDateString(lang === "en" ? "en-US" : "ja-JP", { weekday: "short" });
  }
  return `${d.getFullYear()}/${pad(d.getMonth() + 1)}/${pad(d.getDate())}`;
}

export function sectionLabel(iso: string, lang: string): string {
  const d = new Date(iso);
  const now = Date.now();
  const age = now - d.getTime();
  if (age < 7 * 86400000) return lang === "en" ? "Last 7 days" : "過去7日間";
  if (age < 30 * 86400000) return lang === "en" ? "Last 30 days" : "過去30日間";
  return lang === "en"
    ? d.toLocaleDateString("en-US", { year: "numeric", month: "long" })
    : `${d.getFullYear()}年${d.getMonth() + 1}月`;
}
