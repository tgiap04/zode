import { theme, type ThemeConfig } from 'antd';

export const antdThemeConfig: ThemeConfig = {
  algorithm: theme.defaultAlgorithm, // Light canvas per MiniMax design system
  token: {
    colorPrimary: '#111111',
    colorInfo: '#2563EB',
    colorSuccess: '#059669',
    colorWarning: '#D97706',
    colorError: '#DC2626',
    colorBgBase: '#FFFFFF',
    colorBgContainer: '#FFFFFF',
    colorBgElevated: '#FFFFFF',
    colorBorder: '#E5E7EB',
    colorBorderSecondary: '#F3F4F6',
    colorText: '#111111',
    colorTextSecondary: '#4B5563',
    colorTextTertiary: '#6B7280',
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
      primaryShadow: '0 1px 2px rgba(0, 0, 0, 0.05)',
    },
    Card: {
      colorBgContainer: '#FFFFFF',
      colorBorderSecondary: '#E5E7EB',
      borderRadiusLG: 16,
    },
    Modal: {
      contentBg: '#FFFFFF',
      headerBg: '#FFFFFF',
      borderRadiusLG: 20,
    },
    Tag: {
      borderRadiusSM: 9999,
    },
  },
};
