import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { en } from "./en";
import { zhCN } from "./zh-CN";

void i18n.use(initReactI18next).init({
  resources: { en, "zh-CN": zhCN },
  lng: "zh-CN",
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

export default i18n;
