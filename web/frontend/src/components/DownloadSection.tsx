import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Card, Segmented, Button, Tag, Tooltip, Row, Col, Space, Typography } from 'antd';
import {
  AppleFilled,
  WindowsFilled,
  CodeOutlined,
  DownloadOutlined,
  SafetyCertificateOutlined,
  ClockCircleOutlined,
} from '@ant-design/icons';
import { CURRENT_RELEASE } from '../data/releases';
import type { DownloadOption } from '../types';

const { Title, Paragraph, Text } = Typography;

interface DownloadSectionProps {
  id?: string;
}

export const DownloadSection: React.FC<DownloadSectionProps> = ({ id = 'downloads' }) => {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<'macos' | 'windows' | 'linux'>('macos');

  const macDownloads = CURRENT_RELEASE.downloads.filter((d) => d.platform.startsWith('macos'));
  const winDownloads = CURRENT_RELEASE.downloads.filter((d) => d.platform.startsWith('windows'));
  const linuxDownloads = CURRENT_RELEASE.downloads.filter((d) => d.platform.startsWith('linux'));

  const getFilteredList = () => {
    if (activeTab === 'macos') return macDownloads;
    if (activeTab === 'windows') return winDownloads;
    return linuxDownloads;
  };

  const getIcon = (icon: string) => {
    if (icon === 'apple') return <AppleFilled style={{ fontSize: 22, color: 'var(--color-ink-strong)' }} />;
    if (icon === 'windows') return <WindowsFilled style={{ fontSize: 22, color: '#2563EB' }} />;
    return <CodeOutlined style={{ fontSize: 22, color: '#D97706' }} />;
  };

  return (
    <section id={id} className="promo-cta-section">
      <div className="container-marketing">
        {/* Solid Obsidian Promo Card */}
        <div className="promo-cta-card-wrapper">
          <Tag color="#FF5B37" style={{ borderRadius: 9999, fontWeight: 700, fontSize: 12, padding: '4px 12px', marginBottom: 20 }}>
            {t('downloads.promoBadge')}
          </Tag>
          <Title level={2} className="promo-cta-title" style={{ color: '#FFFFFF', marginTop: 0 }}>
            {t('downloads.promoTitle')}
          </Title>
          <Paragraph className="promo-cta-desc">
            {t('downloads.promoDesc')}
          </Paragraph>

          <Tooltip title={t('common.comingSoon')} placement="top">
            <span style={{ display: 'inline-block', cursor: 'not-allowed' }}>
              <Button
                type="default"
                shape="round"
                size="large"
                icon={<AppleFilled />}
                className="promo-cta-btn-white"
                style={{
                  opacity: 0.75,
                  pointerEvents: 'none',
                  height: 48,
                  fontSize: 16,
                  fontWeight: 600,
                }}
              >
                <span>{t('downloads.promoBtn')}</span>
                <ClockCircleOutlined style={{ fontSize: 13, marginLeft: 4 }} />
              </Button>
            </span>
          </Tooltip>
        </div>

        {/* All Platforms Download Matrix */}
        <div style={{ marginTop: 80 }}>
          <div className="section-header-editorial">
            <Text className="eyebrow">{t('downloads.eyebrow')}</Text>
            <Title level={2} className="display-lg" style={{ marginTop: 8, marginBottom: 16 }}>
              {t('downloads.title')}
            </Title>
            <Paragraph className="subtitle">
              {t('downloads.subtitle')}
            </Paragraph>
          </div>

          {/* Ant Design Segmented OS Platform Switcher */}
          <div style={{ display: 'flex', justifyContent: 'center', marginBottom: 40 }}>
            <Segmented<'macos' | 'windows' | 'linux'>
              size="large"
              value={activeTab}
              onChange={(val) => setActiveTab(val)}
              options={[
                { label: <span><AppleFilled style={{ marginRight: 6 }} />{t('downloads.macosTab')}</span>, value: 'macos' },
                { label: <span><WindowsFilled style={{ marginRight: 6 }} />{t('downloads.windowsTab')}</span>, value: 'windows' },
                { label: <span><CodeOutlined style={{ marginRight: 6 }} />{t('downloads.linuxTab')}</span>, value: 'linux' },
              ]}
              style={{
                fontWeight: 600,
                padding: 4,
                borderRadius: 9999,
              }}
            />
          </div>

          {/* Download Grid */}
          <Row gutter={[24, 24]}>
            {getFilteredList().map((item: DownloadOption) => (
              <Col key={item.id} xs={24} sm={24} md={12} lg={8}>
                <Card
                  className="ai-product-tile"
                  styles={{ body: { padding: 0, height: '100%', display: 'flex', flexDirection: 'column', justifyContent: 'space-between' } }}
                >
                  <div>
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: 16 }}>
                      <div className="ai-tile-icon-box" style={{ marginBottom: 0 }}>
                        {getIcon(item.icon)}
                      </div>
                      {item.recommended && (
                        <Tag color="success" style={{ borderRadius: 9999, fontWeight: 600 }}>
                          {t('downloads.recommended')}
                        </Tag>
                      )}
                    </div>

                    <h3 className="card-title">{item.arch}</h3>
                    <p className="body-sm" style={{ marginBottom: 24 }}>
                      {t('downloads.packageLabel')}: <strong>{item.filename}</strong> ({item.size})
                    </p>
                  </div>

                  <Tooltip title={t('common.comingSoon')} placement="top">
                    <div style={{ width: '100%', cursor: 'not-allowed' }}>
                      <Button
                        type="primary"
                        shape="round"
                        icon={<DownloadOutlined />}
                        block
                        size="large"
                        style={{
                          opacity: 0.65,
                          pointerEvents: 'none',
                          fontWeight: 600,
                        }}
                      >
                        <span>{t('downloads.downloadBtn', { format: item.format })}</span>
                      </Button>
                    </div>
                  </Tooltip>
                </Card>
              </Col>
            ))}
          </Row>

          <Space size={8} style={{ display: 'flex', justifyContent: 'center', marginTop: 40 }}>
            <SafetyCertificateOutlined style={{ color: '#059669', fontSize: 16 }} />
            <Text type="secondary" style={{ fontSize: 13 }}>
              {t('downloads.signedNotice')}
            </Text>
          </Space>
        </div>
      </div>
    </section>
  );
};
