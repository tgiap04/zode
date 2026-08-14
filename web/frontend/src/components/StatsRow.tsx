import React from 'react';
import { useTranslation } from 'react-i18next';
import { Row, Col, Statistic, Card } from 'antd';

export const StatsRow: React.FC = () => {
  const { t } = useTranslation();

  const stats = [
    { value: t('stats.fps'), title: t('stats.fpsLabel') },
    { value: t('stats.startup'), title: t('stats.startupLabel') },
    { value: t('stats.memory'), title: t('stats.memoryLabel') },
    { value: t('stats.latency'), title: t('stats.latencyLabel') },
  ];

  return (
    <section className="stats-row-section">
      <div className="container-marketing">
        <Card
          variant="borderless"
          styles={{ body: { padding: '32px 16px', background: 'transparent' } }}
        >
          <Row gutter={[24, 32]} align="middle" justify="center">
            {stats.map((stat, idx) => (
              <Col key={idx} xs={24} sm={12} md={6}>
                <div className="stat-cell" style={{ borderRight: idx === 3 ? 'none' : undefined }}>
                  <Statistic
                    value={stat.value}
                    formatter={(val) => (
                      <span className="stat-number">{val}</span>
                    )}
                  />
                  <div className="stat-label" style={{ marginTop: 8 }}>{stat.title}</div>
                </div>
              </Col>
            ))}
          </Row>
        </Card>
      </div>
    </section>
  );
};
