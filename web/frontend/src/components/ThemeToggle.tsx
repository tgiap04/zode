import React from 'react';
import { Button, Tooltip } from 'antd';
import { useTheme } from '../theme/ThemeContext';
import { SunOutlined, MoonOutlined } from '@ant-design/icons';

export const ThemeToggle: React.FC = () => {
  const { theme, toggleTheme } = useTheme();
  const isDark = theme === 'dark';

  return (
    <Tooltip title={isDark ? 'Switch to Light Mode' : 'Switch to Dark Mode'} placement="bottom">
      <Button
        shape="round"
        onClick={toggleTheme}
        icon={isDark ? <MoonOutlined style={{ color: '#F0F6FC' }} /> : <SunOutlined style={{ color: '#FF5B37' }} />}
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 6,
          height: 34,
          fontSize: 12,
          fontWeight: 600,
        }}
      >
        <span className="theme-pill-label">{isDark ? 'Dark' : 'Light'}</span>
      </Button>
    </Tooltip>
  );
};
