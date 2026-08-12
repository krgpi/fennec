import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));

execFileSync("cargo", ["build", "--release", "-p", "fennec-cli"], {
  cwd: root,
  stdio: "inherit",
});

const rustcInfo = execFileSync("rustc", ["-vV"], { cwd: root }).toString();
const triple = rustcInfo.match(/host: (\S+)/)[1];
const ext = process.platform === "win32" ? ".exe" : "";

const src = join(root, "target", "release", `fennec${ext}`);
const destDir = join(root, "src-tauri", "binaries");
mkdirSync(destDir, { recursive: true });
const dest = join(destDir, `fennec-${triple}${ext}`);
copyFileSync(src, dest);
console.log(`built ${dest}`);
