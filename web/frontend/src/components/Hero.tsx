import React from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Tag, Tooltip, Space, Typography } from 'antd';
import { AppleFilled, WindowsFilled, CodeOutlined, ThunderboltFilled } from '@ant-design/icons';
import { useOS } from '../hooks/useOS';

const { Title, Paragraph } = Typography;

export const Hero: React.FC = () => {
  const { t } = useTranslation();
  const { recommendedDownload } = useOS();

  const getOsIcon = () => {
    if (recommendedDownload.osName === 'macOS') return <AppleFilled />;
    if (recommendedDownload.osName === 'Windows') return <WindowsFilled />;
    return <CodeOutlined />;
  };

  return (
    <section className="hero-marketing">
      {/* Background Soft Center Halo */}
      <div className="hero-subtle-glow" aria-hidden="true" />

      {/* Non-intrusive Background Floating Code Symbols */}
      <div className="hero-floating-elements" aria-hidden="true">
        <span className="hero-code-glyph glyph-1">&#123; &#125;</span>
        <span className="hero-code-glyph glyph-2">&lt; /&gt;</span>
        <span className="hero-code-glyph glyph-3">fn main()</span>
        <span className="hero-code-glyph glyph-4">// 120 FPS</span>
      </div>

      {/* Two Subtle Outer Ambient Badges (Far Left & Far Right) */}
      <div className="hero-ambient-badge badge-left">
        <span className="hero-pulse-dot" style={{ backgroundColor: '#FF5B37' }} />
        <span>{t('hero.floatingLeft')}</span>
      </div>

      <div className="hero-ambient-badge badge-right">
        <span className="hero-pulse-dot" style={{ backgroundColor: '#10B981' }} />
        <span>{t('hero.floatingRight')}</span>
      </div>

      <div className="container-marketing hero-marketing-content">
        {/* Clean Announcement Badge with Live Pulse Dot */}
        <div className="hero-announcement-badge">
          <Tag
            color="default"
            className="hero-badge-tag"
            style={{
              padding: '6px 14px',
              fontSize: 13,
              borderRadius: 9999,
              display: 'inline-flex',
              alignItems: 'center',
              gap: 8,
            }}
          >
            <span className="hero-pulse-dot" style={{ backgroundColor: '#FF5B37' }} />
            <ThunderboltFilled style={{ color: '#FF5B37' }} />
            <span>{t('hero.badge')}</span>
            <Tag color="#FF5B37" style={{ margin: 0, padding: '0 6px', fontSize: 11, borderRadius: 9999 }}>
              {t('hero.version')}
            </Tag>
          </Tag>
        </div>

        <Title
          level={1}
          className="hero-display hero-display-title"
          style={{ marginBottom: 24 }}
        >
          {t('hero.titleLine1')}<br />
          {t('hero.titleLine2')}
        </Title>

        <Paragraph
          className="hero-lead-subtitle"
          style={{ marginBottom: 36 }}
        >
          {t('hero.subtitle')}
        </Paragraph>

        <Space size={16} wrap className="hero-cta-group" style={{ marginBottom: 32, justifyContent: 'center' }}>
          {/* Ant Design Tooltip on Disabled Download Button */}
          <Tooltip title={t('common.comingSoon')} placement="top">
            <span style={{ display: 'inline-block', cursor: 'not-allowed' }}>
              <Button
                type="primary"
                shape="round"
                size="large"
                icon={getOsIcon()}
                style={{
                  height: 48,
                  padding: '0 32px',
                  fontSize: 15,
                  fontWeight: 600,
                  opacity: 0.65,
                  pointerEvents: 'none',
                }}
              >
                <span>{t('hero.downloadFor', { os: recommendedDownload.osName })}</span>
              </Button>
            </span>
          </Tooltip>

          <Button
            href="#downloads"
            shape="round"
            size="large"
            style={{
              height: 48,
              padding: '0 28px',
              fontSize: 15,
              fontWeight: 600,
            }}
          >
            <span>{t('hero.allPlatforms')}</span>
          </Button>
        </Space>

        <Space size={20} wrap className="hero-meta-strip" style={{ justifyContent: 'center' }}>
          <span className="hero-meta-item">✓ {t('hero.metaSilicon')}</span>
          <span>•</span>
          <span className="hero-meta-item">✓ {t('hero.metaZeroBloat')}</span>
          <span>•</span>
          <span className="hero-meta-item">✓ {t('hero.metaStartup')}</span>
        </Space>
      </div>
    </section>
  );
};
