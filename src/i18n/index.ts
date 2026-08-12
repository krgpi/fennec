import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import ja from "./ja.json";
import en from "./en.json";

const stored = localStorage.getItem("appLanguage");
const systemLang = navigator.language.startsWith("ja") ? "ja" : "en";
const lng = stored === "ja" || stored === "en" ? stored : systemLang;

i18n.use(initReactI18next).init({
  resources: {
    ja: { translation: ja },
    en: { translation: en },
  },
  lng,
  fallbackLng: "ja",
  interpolation: { escapeValue: false },
  keySeparator: false,
  nsSeparator: false,
});

export function setAppLanguage(lang: "system" | "ja" | "en") {
  if (lang === "system") {
    localStorage.removeItem("appLanguage");
    i18n.changeLanguage(systemLang);
  } else {
    localStorage.setItem("appLanguage", lang);
    i18n.changeLanguage(lang);
  }
}

export default i18n;
