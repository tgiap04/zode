import React from 'react';
import { ConfigProvider } from 'antd';
import { ThemeProvider, useTheme, getAntdThemeConfig } from './theme/ThemeContext';
import { useLocaleSync } from './i18n/localeManager';
import { PromoBanner } from './components/PromoBanner';
import { Navbar } from './components/Navbar';
import { Hero } from './components/Hero';
import { ProductMatrix } from './components/ProductMatrix';
import { StatsRow } from './components/StatsRow';
import { IdePreview } from './components/IdePreview';
import { Features } from './components/Features';
import { Benchmarks } from './components/Benchmarks';
import { KitShowcase } from './components/KitShowcase';
import { DownloadSection } from './components/DownloadSection';
import { Footer } from './components/Footer';

const AppContent: React.FC = () => {
  const { theme } = useTheme();

  // Synchronizes URL pathname, localStorage, and i18next language
  useLocaleSync();

  return (
    <ConfigProvider theme={getAntdThemeConfig(theme)}>
      <div className="landing-wrapper">
        {/* Promo banner above nav */}
        <PromoBanner />

        {/* Sticky top navigation */}
        <Navbar />

        <main className="main-content">
          {/* Hero marketing band */}
          <Hero />

          {/* Solid Product Matrix (4 Pillar Cards) */}
          <ProductMatrix />

          {/* Editorial Stats Row */}
          <StatsRow />

          {/* Interactive IDE Preview */}
          <IdePreview />

          {/* Architectural Features Matrix */}
          <Features />

          {/* Empirical Benchmarks */}
          <Benchmarks />

          {/* All-In-One Infra Showcase */}
          <KitShowcase />

          {/* All Platforms Download Section */}
          <DownloadSection id="downloads" />
        </main>

        {/* Dense Solid Black Canvas Footer */}
        <Footer />
      </div>
    </ConfigProvider>
  );
};

export const App: React.FC = () => {
  return (
    <ThemeProvider>
      <AppContent />
    </ThemeProvider>
  );
};

export default App;
