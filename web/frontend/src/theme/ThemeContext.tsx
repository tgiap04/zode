import React, { createContext, useContext, useEffect, useState } from 'react';
import { theme as antdTheme, type ThemeConfig } from 'antd';

export type ThemeMode = 'light' | 'dark';

interface ThemeContextValue {
  theme: ThemeMode;
  toggleTheme: () => void;
  setTheme: (theme: ThemeMode) => void;
}

const THEME_STORAGE_KEY = 'zode_theme';

const ThemeContext = createContext<ThemeContextValue>({
  theme: 'light',
  toggleTheme: () => {},
  setTheme: () => {},
});

export const useTheme = () => useContext(ThemeContext);

export function getInitialTheme(): ThemeMode {
  if (typeof window === 'undefined') return 'light';
  try {
    const saved = localStorage.getItem(THEME_STORAGE_KEY) as ThemeMode | null;
    if (saved === 'light' || saved === 'dark') {
      return saved;
    }
    if (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches) {
      return 'dark';
    }
  } catch {
    // fallback
  }
  return 'light';
}

export const getAntdThemeConfig = (mode: ThemeMode): ThemeConfig => {
  const isDark = mode === 'dark';

  return {
    algorithm: isDark ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
    token: {
      colorPrimary: isDark ? '#FFFFFF' : '#111111',
      colorInfo: '#2563EB',
      colorSuccess: '#059669',
      colorWarning: '#D97706',
      colorError: '#DC2626',
      colorBgBase: isDark ? '#0A0A0C' : '#FFFFFF',
      colorBgContainer: isDark ? '#11141A' : '#FFFFFF',
      colorBgElevated: isDark ? '#161B22' : '#FFFFFF',
      colorBorder: isDark ? '#21262D' : '#E5E7EB',
      colorBorderSecondary: isDark ? '#1C2128' : '#F3F4F6',
      colorText: isDark ? '#F0F6FC' : '#111111',
      colorTextSecondary: isDark ? '#8B949E' : '#4B5563',
      colorTextTertiary: isDark ? '#6E7681' : '#6B7280',
      borderRadius: 9999, // Pill buttons everywhere
      borderRadiusLG: 16,
      borderRadiusSM: 9999,
      fontFamily: "'DM Sans', -apple-system, BlinkMacSystemFont, 'Inter', 'Segoe UI', sans-serif",
      fontSize: 14,
    },
    components: {
      Button: {
        controlHeight: 42,
        borderRadius: 9999,
        fontWeight: 600,
        primaryShadow: isDark ? '0 0 12px rgba(255, 255, 255, 0.15)' : '0 1px 2px rgba(0, 0, 0, 0.05)',
      },
      Card: {
        colorBgContainer: isDark ? '#11141A' : '#FFFFFF',
        colorBorderSecondary: isDark ? '#21262D' : '#E5E7EB',
        borderRadiusLG: 16,
      },
      Modal: {
        contentBg: isDark ? '#11141A' : '#FFFFFF',
        headerBg: isDark ? '#11141A' : '#FFFFFF',
        borderRadiusLG: 20,
      },
      Tag: {
        borderRadiusSM: 9999,
      },
      Tooltip: {
        colorBgSpotlight: isDark ? '#21262D' : '#111111',
        colorTextLightSolid: '#FFFFFF',
        borderRadius: 8,
        fontSize: 12,
      },
    },
  };
};

export const ThemeProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [theme, setThemeState] = useState<ThemeMode>(getInitialTheme);

  const setTheme = (newTheme: ThemeMode) => {
    setThemeState(newTheme);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, newTheme);
    } catch {
      // ignore
    }
  };

  const toggleTheme = () => {
    setTheme(theme === 'light' ? 'dark' : 'light');
  };

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
  }, [theme]);

  return (
    <ThemeContext.Provider value={{ theme, toggleTheme, setTheme }}>
      {children}
    </ThemeContext.Provider>
  );
};
