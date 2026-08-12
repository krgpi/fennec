import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const source = join(root, "..", "code", "Resources", "Localizable.xcstrings");
const outDir = join(root, "src", "i18n");

const catalog = JSON.parse(readFileSync(source, "utf8"));
const ja = {};
const en = {};

for (const [key, entry] of Object.entries(catalog.strings)) {
  const jaValue = entry.localizations?.ja?.stringUnit?.value ?? key;
  const enValue = entry.localizations?.en?.stringUnit?.value ?? jaValue;
  ja[key] = jaValue;
  en[key] = enValue;
}

const extraPath = join(outDir, "extra.json");
try {
  const extra = JSON.parse(readFileSync(extraPath, "utf8"));
  for (const [key, values] of Object.entries(extra)) {
    ja[key] = values.ja ?? key;
    en[key] = values.en ?? values.ja ?? key;
  }
} catch {
  // extra.json が無ければ変換元カタログのみ
}

mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, "ja.json"), JSON.stringify(ja, null, 2) + "\n");
writeFileSync(join(outDir, "en.json"), JSON.stringify(en, null, 2) + "\n");
console.log(`converted ${Object.keys(ja).length} keys`);
