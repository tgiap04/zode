import React from 'react';
import { useTranslation } from 'react-i18next';
import { Card, Row, Col, Typography } from 'antd';
import {
  AppstoreOutlined,
  DatabaseOutlined,
  CloudServerOutlined,
  RobotOutlined,
  ThunderboltOutlined,
  CodeOutlined,
} from '@ant-design/icons';

const { Title, Paragraph, Text } = Typography;

export const Features: React.FC = () => {
  const { t } = useTranslation();

  const featureItems = [
    {
      id: 'multi-project',
      title: t('features.multiProjectTitle'),
      description: t('features.multiProjectDesc'),
      icon: <AppstoreOutlined style={{ color: '#FF5B37' }} />,
    },
    {
      id: 'db-studio',
      title: t('features.dbStudioTitle'),
      description: t('features.dbStudioDesc'),
      icon: <DatabaseOutlined style={{ color: '#3B82F6' }} />,
    },
    {
      id: 'docker-pods',
      title: t('features.dockerPodsTitle'),
      description: t('features.dockerPodsDesc'),
      icon: <CloudServerOutlined style={{ color: '#8B5CF6' }} />,
    },
    {
      id: 'ai-agent',
      title: t('features.aiAgentTitle'),
      description: t('features.aiAgentDesc'),
      icon: <RobotOutlined style={{ color: '#E02475' }} />,
    },
    {
      id: 'gpu-engine',
      title: t('features.gpuEngineTitle'),
      description: t('features.gpuEngineDesc'),
      icon: <ThunderboltOutlined style={{ color: '#06B6D4' }} />,
    },
    {
      id: 'super-terminal',
      title: t('features.superTerminalTitle'),
      description: t('features.superTerminalDesc'),
      icon: <CodeOutlined style={{ color: '#10B981' }} />,
    },
  ];

  return (
    <section id="features" className="ai-product-matrix-section">
      <div className="container-marketing">
        <div className="section-header-editorial">
          <Text className="eyebrow">{t('features.eyebrow')}</Text>
          <Title level={2} className="display-lg" style={{ marginTop: 8, marginBottom: 16 }}>
            {t('features.title')}
          </Title>
          <Paragraph className="subtitle">
            {t('features.subtitle')}
          </Paragraph>
        </div>

        <Row gutter={[24, 24]}>
          {featureItems.map((item) => (
            <Col key={item.id} xs={24} sm={24} md={12} lg={8}>
              <Card
                className="ai-product-tile"
                styles={{ body: { padding: 0 } }}
              >
                <div className="ai-tile-icon-box">{item.icon}</div>
                <h3 className="ai-tile-title">{item.title}</h3>
                <p className="ai-tile-desc">{item.description}</p>
              </Card>
            </Col>
          ))}
        </Row>
      </div>
    </section>
  );
};
