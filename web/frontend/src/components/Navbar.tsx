import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Drawer, Tooltip, Space } from 'antd';
import {
  DownloadOutlined,
  GithubOutlined,
  MenuOutlined,
  CloseOutlined,
  AppstoreOutlined,
  ThunderboltOutlined,
  DashboardOutlined,
  CloudServerOutlined,
  ArrowRightOutlined,
} from '@ant-design/icons';
import { LanguageSelector } from './LanguageSelector';
import { ThemeToggle } from './ThemeToggle';

export const Navbar: React.FC = () => {
  const { t } = useTranslation();
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);

  const navLinks = [
    { href: '#models-matrix', label: t('nav.products'), icon: <AppstoreOutlined /> },
    { href: '#features', label: t('nav.architecture'), icon: <ThunderboltOutlined /> },
    { href: '#benchmarks', label: t('nav.benchmarks'), icon: <DashboardOutlined /> },
    { href: '#infra', label: t('nav.allInOne'), icon: <CloudServerOutlined /> },
    { href: '#downloads', label: t('nav.downloads'), icon: <DownloadOutlined /> },
  ];

  const handleLinkClick = () => {
    setMobileMenuOpen(false);
  };

  return (
    <>
      <header className="top-navbar">
        <div className="container-marketing nav-container">
          <div className="nav-brand-group">
            <a href="#" className="nav-logo">
              <img src="/logo.png" alt="Zode Logo" className="nav-logo-img" />
              <span>Zode</span>
            </a>

            {/* Desktop Navigation Links */}
            <nav className="desktop-nav">
              <ul className="nav-menu-links">
                {navLinks.map((link) => (
                  <li key={link.href}>
                    <a href={link.href} className="nav-menu-link">
                      {link.label}
                    </a>
                  </li>
                ))}
              </ul>
            </nav>
          </div>

          <Space size={10} className="nav-actions">
            {/* Theme Toggle (Light / Dark) */}
            <ThemeToggle />

            {/* Language Selector (EN / VI) */}
            <LanguageSelector />

            {/* Desktop GitHub Button */}
            <Button
              href="https://github.com/zode/zode"
              target="_blank"
              rel="noreferrer"
              shape="round"
              icon={<GithubOutlined />}
              className="desktop-only-btn"
              style={{
                fontSize: 13,
                fontWeight: 600,
                height: 34,
              }}
            >
              <span>{t('nav.github')}</span>
            </Button>

            {/* Disabled Get Zode button with Antd Tooltip */}
            <Tooltip title={t('common.comingSoon')} placement="bottom">
              <span
                className="desktop-only-btn"
                style={{ display: 'inline-flex', cursor: 'not-allowed' }}
              >
                <Button
                  type="primary"
                  shape="round"
                  icon={<DownloadOutlined />}
                  style={{
                    fontSize: 13,
                    fontWeight: 600,
                    height: 34,
                    opacity: 0.65,
                    pointerEvents: 'none',
                  }}
                >
                  <span>{t('nav.getZode')}</span>
                </Button>
              </span>
            </Tooltip>

            {/* Mobile Hamburger Toggle Button */}
            <Button
              shape="circle"
              icon={mobileMenuOpen ? <CloseOutlined /> : <MenuOutlined />}
              className="mobile-hamburger-btn"
              onClick={() => setMobileMenuOpen(!mobileMenuOpen)}
              aria-label="Toggle navigation menu"
              style={{ width: 36, height: 36 }}
            />
          </Space>
        </div>
      </header>

      {/* Mobile Navigation Drawer */}
      <Drawer
        placement="right"
        open={mobileMenuOpen}
        onClose={() => setMobileMenuOpen(false)}
        rootClassName="mobile-nav-drawer"
        title={
          <div className="nav-logo" style={{ fontSize: 18 }}>
            <img src="/logo.png" alt="Zode Logo" className="nav-logo-img" style={{ width: 28, height: 28 }} />
            <span>Zode Super IDE</span>
          </div>
        }
        closeIcon={<CloseOutlined style={{ fontSize: 16, color: 'var(--color-ink)' }} />}
      >
        <div className="mobile-drawer-content">
          <nav className="mobile-drawer-nav">
            <ul className="mobile-nav-list">
              {navLinks.map((link) => (
                <li key={link.href}>
                  <a
                    href={link.href}
                    className="mobile-nav-item"
                    onClick={handleLinkClick}
                  >
                    <div className="mobile-nav-item-left">
                      <span className="mobile-nav-icon">{link.icon}</span>
                      <span className="mobile-nav-text">{link.label}</span>
                    </div>
                    <ArrowRightOutlined className="mobile-nav-arrow" />
                  </a>
                </li>
              ))}
            </ul>
          </nav>

          <Space direction="vertical" size={12} style={{ width: '100%', paddingTop: 16 }}>
            <Tooltip title={t('common.comingSoon')} placement="top">
              <span style={{ display: 'block', width: '100%', cursor: 'not-allowed' }}>
                <Button
                  type="primary"
                  shape="round"
                  size="large"
                  icon={<DownloadOutlined />}
                  block
                  style={{ opacity: 0.65, pointerEvents: 'none', fontWeight: 600 }}
                >
                  <span>{t('nav.getZode')}</span>
                </Button>
              </span>
            </Tooltip>

            <Button
              href="https://github.com/zode/zode"
              target="_blank"
              rel="noreferrer"
              shape="round"
              size="large"
              icon={<GithubOutlined />}
              block
              style={{ fontWeight: 600 }}
              onClick={handleLinkClick}
            >
              <span>{t('nav.github')}</span>
            </Button>
          </Space>
        </div>
      </Drawer>
    </>
  );
};
