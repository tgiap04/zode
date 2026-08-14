import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Card, Tag, Button, Row, Col, Typography, message } from 'antd';
import {
  AppstoreOutlined,
  DatabaseOutlined,
  CloudServerOutlined,
  CopyOutlined,
  CheckOutlined,
} from '@ant-design/icons';

const { Title, Paragraph, Text } = Typography;

export const KitShowcase: React.FC = () => {
  const { t } = useTranslation();
  const [copiedIndex, setCopiedIndex] = useState<number | null>(null);

  const infraCards = [
    {
      name: t('allInOneShowcase.multiRepoTitle'),
      pkg: 'workspace-engine',
      description: t('allInOneShowcase.multiRepoDesc'),
      command: 'zode workspace attach ./backend ./frontend ./infra --sync',
      icon: <AppstoreOutlined style={{ color: '#FF5B37' }} />,
    },
    {
      name: t('allInOneShowcase.dbTitle'),
      pkg: 'native-db-driver',
      description: t('allInOneShowcase.dbDesc'),
      command: 'zode db query --uri postgres://dev@localhost:5432/main "SELECT * FROM users"',
      icon: <DatabaseOutlined style={{ color: '#3B82F6' }} />,
    },
    {
      name: t('allInOneShowcase.dockerTitle'),
      pkg: 'container-runtime',
      description: t('allInOneShowcase.dockerDesc'),
      command: 'zode docker attach api-gateway --logs --metrics --shell',
      icon: <CloudServerOutlined style={{ color: '#8B5CF6' }} />,
    },
  ];

  const handleCopy = (cmd: string, index: number) => {
    navigator.clipboard.writeText(cmd);
    setCopiedIndex(index);
    message.success(t('allInOneShowcase.copied'));
    setTimeout(() => setCopiedIndex(null), 2000);
  };

  return (
    <section id="infra" className="kit-section">
      <div className="container-marketing">
        <div className="section-header-editorial">
          <Text className="eyebrow">{t('allInOneShowcase.eyebrow')}</Text>
          <Title level={2} className="display-lg" style={{ marginTop: 8, marginBottom: 16 }}>
            {t('allInOneShowcase.title')}
          </Title>
          <Paragraph className="subtitle">
            {t('allInOneShowcase.subtitle')}
          </Paragraph>
        </div>

        <Row gutter={[24, 24]}>
          {infraCards.map((tool, idx) => (
            <Col key={idx} xs={24} sm={24} md={12} lg={8}>
              <Card
                className="kit-tool-card"
                styles={{ body: { padding: 0, height: '100%', display: 'flex', flexDirection: 'column', justifyContent: 'space-between' } }}
              >
                <div>
                  <div className="kit-card-header">
                    <div className="kit-icon-badge">{tool.icon}</div>
                    <Tag className="badge-code" style={{ margin: 0 }}>{tool.pkg}</Tag>
                  </div>

                  <h3 className="card-title" style={{ marginBottom: 12 }}>
                    {tool.name}
                  </h3>

                  <p className="body-sm" style={{ lineHeight: 1.6 }}>
                    {tool.description}
                  </p>
                </div>

                {/* Terminal Snippet Box with Ant Design Button */}
                <div className="kit-terminal-snippet">
                  <div className="kit-terminal-code">
                    <span className="kit-terminal-prompt">$</span>
                    <code>{tool.command}</code>
                  </div>

                  <Button
                    size="small"
                    shape="round"
                    onClick={() => handleCopy(tool.command, idx)}
                    icon={copiedIndex === idx ? <CheckOutlined /> : <CopyOutlined />}
                    className={`kit-copy-action-btn ${copiedIndex === idx ? 'copied' : ''}`}
                  >
                    <span>{copiedIndex === idx ? t('allInOneShowcase.copied') : t('allInOneShowcase.copy')}</span>
                  </Button>
                </div>
              </Card>
            </Col>
          ))}
        </Row>
      </div>
    </section>
  );
};
