export type Platform = "macos" | "windows" | "linux";

function detect(): Platform {
  const ua = navigator.userAgent;
  if (ua.includes("Windows")) return "windows";
  if (ua.includes("Mac")) return "macos";
  return "linux";
}

export const platform = detect();
export const isMac = platform === "macos";
export const isWindows = platform === "windows";
export const isLinux = platform === "linux";

export const revealFolderKey = isMac
  ? "Finderで開く"
  : isWindows
    ? "エクスプローラーで開く"
    : "フォルダーを開く";
