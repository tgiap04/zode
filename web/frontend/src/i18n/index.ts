import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import LanguageDetector from 'i18next-browser-languagedetector';

import en from './locales/en.json';
import vi from './locales/vi.json';
import { getLocaleFromPath, getStoredLocale, DEFAULT_LOCALE } from './localeManager';

// Determine initial locale from URL path first, then localStorage, then fallback
const initialLocale =
  (typeof window !== 'undefined' && getLocaleFromPath(window.location.pathname)) ||
  (typeof window !== 'undefined' && getStoredLocale()) ||
  DEFAULT_LOCALE;

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      vi: { translation: vi },
    },
    lng: initialLocale,
    fallbackLng: DEFAULT_LOCALE,
    interpolation: {
      escapeValue: false,
    },
    detection: {
      order: ['path', 'localStorage', 'navigator', 'htmlTag'],
      lookupFromPathIndex: 0,
      caches: ['localStorage'],
    },
  });

export default i18n;
