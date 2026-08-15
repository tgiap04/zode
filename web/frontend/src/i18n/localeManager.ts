import { useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';

export const SUPPORTED_LOCALES = ['en', 'vi'] as const;
export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];
export const DEFAULT_LOCALE: SupportedLocale = 'en';
export const STORAGE_KEY = 'i18nextLng';

/**
 * Extracts a valid locale from a given pathname (e.g. /vi/docs -> 'vi', /en -> 'en')
 */
export function getLocaleFromPath(pathname: string): SupportedLocale | null {
  const segments = pathname.split('/').filter(Boolean);
  const firstSegment = segments[0]?.toLowerCase();

  if (firstSegment && (SUPPORTED_LOCALES as readonly string[]).includes(firstSegment)) {
    return firstSegment as SupportedLocale;
  }
  return null;
}

/**
 * Retrieves the stored locale from localStorage if valid
 */
export function getStoredLocale(): SupportedLocale {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)?.toLowerCase();
    if (stored) {
      if (stored.startsWith('vi')) return 'vi';
      if (stored.startsWith('en')) return 'en';
    }
  } catch (e) {
    console.warn('Unable to access localStorage', e);
  }
  return DEFAULT_LOCALE;
}

/**
 * Saves the locale to localStorage
 */
export function setStoredLocale(locale: SupportedLocale): void {
  try {
    localStorage.setItem(STORAGE_KEY, locale);
    localStorage.setItem('zode_locale', locale);
  } catch (e) {
    console.warn('Unable to write to localStorage', e);
  }
}

/**
 * Builds a new URL pathname with the target locale while preserving remaining path segments, search, and hash
 */
export function buildLocaleUrl(targetLocale: SupportedLocale): string {
  const { pathname, search, hash } = window.location;
  const segments = pathname.split('/').filter(Boolean);

  let remainingSegments: string[] = [];
  if (segments.length > 0 && (SUPPORTED_LOCALES as readonly string[]).includes(segments[0].toLowerCase())) {
    remainingSegments = segments.slice(1);
  } else {
    remainingSegments = segments;
  }

  const newPath = `/${targetLocale}${remainingSegments.length > 0 ? '/' + remainingSegments.join('/') : ''}`;
  return `${newPath}${search}${hash}`;
}

/**
 * Hook to synchronize URL path, localStorage, and i18next language
 */
export function useLocaleSync() {
  const { i18n } = useTranslation();

  // Function to switch locale: updates i18n, localStorage, and URL path
  const changeLocale = useCallback(
    (newLocale: SupportedLocale) => {
      setStoredLocale(newLocale);
      if (i18n.language !== newLocale) {
        i18n.changeLanguage(newLocale);
      }

      const currentPathLocale = getLocaleFromPath(window.location.pathname);
      if (currentPathLocale !== newLocale) {
        const newUrl = buildLocaleUrl(newLocale);
        window.history.pushState({ locale: newLocale }, '', newUrl);
      }
    },
    [i18n],
  );

  useEffect(() => {
    // 1. Check path locale
    const pathLocale = getLocaleFromPath(window.location.pathname);

    if (pathLocale) {
      // Path has valid locale: sync to i18n & localStorage
      setStoredLocale(pathLocale);
      if (i18n.language !== pathLocale) {
        i18n.changeLanguage(pathLocale);
      }
    } else {
      // Path has no locale prefix (e.g. /): use stored locale and rewrite path
      const storedLocale = getStoredLocale();
      setStoredLocale(storedLocale);
      if (i18n.language !== storedLocale) {
        i18n.changeLanguage(storedLocale);
      }

      const newUrl = buildLocaleUrl(storedLocale);
      window.history.replaceState({ locale: storedLocale }, '', newUrl);
    }

    // 2. Listen to browser back/forward navigation (popstate)
    const handlePopState = () => {
      const activePathLocale = getLocaleFromPath(window.location.pathname) || getStoredLocale();
      setStoredLocale(activePathLocale);
      if (i18n.language !== activePathLocale) {
        i18n.changeLanguage(activePathLocale);
      }
    };

    window.addEventListener('popstate', handlePopState);
    return () => window.removeEventListener('popstate', handlePopState);
  }, [i18n]);

  const currentLocale: SupportedLocale =
    i18n.language && i18n.language.startsWith('vi') ? 'vi' : 'en';

  return {
    currentLocale,
    changeLocale,
    supportedLocales: SUPPORTED_LOCALES,
  };
}
