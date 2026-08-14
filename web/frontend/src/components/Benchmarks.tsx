import React from 'react';
import { useTranslation } from 'react-i18next';
import { Card, Tag, Progress, Row, Col, Typography } from 'antd';

const { Title, Paragraph, Text } = Typography;

export const Benchmarks: React.FC = () => {
  const { t } = useTranslation();

  const benchmarkItems = [
    {
      metric: t('benchmarks.startupTitle'),
      unit: 'ms',
      zode: 18,
      vscode: 480,
      cursor: 720,
      description: t('benchmarks.startupDesc'),
    },
    {
      metric: t('benchmarks.memoryTitle'),
      unit: 'MB',
      zode: 68,
      vscode: 1450,
      cursor: 1820,
      description: t('benchmarks.memoryDesc'),
    },
    {
      metric: t('benchmarks.latencyTitle'),
      unit: 'ms',
      zode: 0.9,
      vscode: 14.5,
      cursor: 16.2,
      description: t('benchmarks.latencyDesc'),
    },
  ];

  return (
    <section id="benchmarks" className="benchmarks-matrix-section">
      <div className="container-marketing">
        <div className="section-header-editorial">
          <Text className="eyebrow">{t('benchmarks.eyebrow')}</Text>
          <Title level={2} className="display-lg" style={{ marginTop: 8, marginBottom: 16 }}>
            {t('benchmarks.title')}
          </Title>
          <Paragraph className="subtitle">
            {t('benchmarks.subtitle')}
          </Paragraph>
        </div>

        <Row gutter={[24, 24]}>
          {benchmarkItems.map((item, index) => {
            const maxVal = Math.max(item.zode, item.vscode, item.cursor);
            const zodePercent = Math.max(6, (item.zode / maxVal) * 100);
            const vscodePercent = (item.vscode / maxVal) * 100;
            const cursorPercent = (item.cursor / maxVal) * 100;

            return (
              <Col key={index} xs={24} sm={24} md={12} lg={8}>
                <Card
                  className="benchmark-stat-card"
                  styles={{ body: { padding: 0 } }}
                >
                  <div className="benchmark-card-header">
                    <h3 className="card-title">{item.metric}</h3>
                    <Tag color="#FF5B37" style={{ borderRadius: 9999, fontSize: 11, fontWeight: 600 }}>
                      {t('benchmarks.lowerIsBetter')}
                    </Tag>
                  </div>

                  <p className="body-sm" style={{ marginBottom: 20 }}>
                    {item.description}
                  </p>

                  {/* Zode Bar */}
                  <div className="benchmark-bar-row">
                    <div className="benchmark-bar-meta">
                      <span style={{ color: '#FF5B37', fontWeight: 700 }}>⚡ Zode</span>
                      <span style={{ color: '#FF5B37' }}>
                        {item.zode} {item.unit}
                      </span>
                    </div>
                    <Progress
                      percent={zodePercent}
                      showInfo={false}
                      strokeColor="#FF5B37"
                      size={['100%', 8]}
                    />
                  </div>

                  {/* VS Code Bar */}
                  <div className="benchmark-bar-row" style={{ marginTop: 12 }}>
                    <div className="benchmark-bar-meta">
                      <span>VS Code</span>
                      <span>
                        {item.vscode} {item.unit}
                      </span>
                    </div>
                    <Progress
                      percent={vscodePercent}
                      showInfo={false}
                      strokeColor="var(--color-stone)"
                      size={['100%', 8]}
                    />
                  </div>

                  {/* Cursor Bar */}
                  <div className="benchmark-bar-row" style={{ marginTop: 12 }}>
                    <div className="benchmark-bar-meta">
                      <span>Cursor</span>
                      <span>
                        {item.cursor} {item.unit}
                      </span>
                    </div>
                    <Progress
                      percent={cursorPercent}
                      showInfo={false}
                      strokeColor="var(--color-stone)"
                      size={['100%', 8]}
                    />
                  </div>
                </Card>
              </Col>
            );
          })}
        </Row>
      </div>
    </section>
  );
};
