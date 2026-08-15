import React from 'react';
import { useTranslation } from 'react-i18next';
import { Row, Col, Typography, Space, Divider } from 'antd';
import { GithubOutlined, TwitterOutlined } from '@ant-design/icons';
import { CURRENT_RELEASE } from '../data/releases';

const { Text, Link } = Typography;

export const Footer: React.FC = () => {
  const { t } = useTranslation();

  return (
    <footer className="footer-region">
      <div className="container-marketing">
        <Row gutter={[48, 48]} className="footer-grid-row" style={{ marginBottom: 64 }}>
          {/* Col 1: Brand & Wordmark */}
          <Col xs={24} sm={24} md={8} lg={8} className="footer-brand-col">
            <div className="footer-logo">
              <img src="/logo.png" alt="Zode Logo" className="footer-logo-img" />
              <span>Zode</span>
            </div>
            <p className="footer-tagline">
              {t('footer.tagline')}
            </p>
            <Space size={16} style={{ marginTop: 'auto' }}>
              <Link
                href="https://github.com/zode/zode"
                target="_blank"
                rel="noreferrer"
                style={{ color: '#9CA3AF', fontSize: 18 }}
              >
                <GithubOutlined />
              </Link>
              <Link
                href="https://twitter.com"
                target="_blank"
                rel="noreferrer"
                style={{ color: '#9CA3AF', fontSize: 18 }}
              >
                <TwitterOutlined />
              </Link>
            </Space>
          </Col>

          {/* Col 2: Super IDE */}
          <Col xs={12} sm={12} md={4} lg={4}>
            <div className="footer-col-title">{t('footer.productCol')}</div>
            <ul className="footer-links-list">
              <li className="footer-link-item"><a href="#models-matrix">Multi-Project Hub</a></li>
              <li className="footer-link-item"><a href="#models-matrix">Database Studio</a></li>
              <li className="footer-link-item"><a href="#models-matrix">Docker & Pods</a></li>
              <li className="footer-link-item"><a href="#models-matrix">Zode AI Agent</a></li>
              <li className="footer-link-item"><a href="#benchmarks">{t('nav.benchmarks')}</a></li>
            </ul>
          </Col>

          {/* Col 3: Workspaces & Infra */}
          <Col xs={12} sm={12} md={4} lg={4}>
            <div className="footer-col-title">{t('footer.ecosystemCol')}</div>
            <ul className="footer-links-list">
              <li className="footer-link-item"><a href="#infra">PostgreSQL & Redis</a></li>
              <li className="footer-link-item"><a href="#infra">Docker Compose</a></li>
              <li className="footer-link-item"><a href="#infra">Kubernetes Pods</a></li>
              <li className="footer-link-item"><a href="#infra">ACP & MCP Protocols</a></li>
            </ul>
          </Col>

          {/* Col 4: Resources */}
          <Col xs={12} sm={12} md={4} lg={4}>
            <div className="footer-col-title">{t('footer.resourcesCol')}</div>
            <ul className="footer-links-list">
              <li className="footer-link-item"><a href="https://github.com/zode/zode/releases">Changelog</a></li>
              <li className="footer-link-item"><a href="#downloads">{t('nav.downloads')}</a></li>
              <li className="footer-link-item"><a href="https://github.com/zode/zode">Documentation</a></li>
              <li className="footer-link-item"><a href="https://github.com/zode/zode/issues">Issue Tracker</a></li>
            </ul>
          </Col>

          {/* Col 5: Community */}
          <Col xs={12} sm={12} md={4} lg={4}>
            <div className="footer-col-title">{t('footer.communityCol')}</div>
            <ul className="footer-links-list">
              <li className="footer-link-item"><a href="https://github.com/zode/zode">GitHub Discussions</a></li>
              <li className="footer-link-item"><a href="https://discord.gg">Discord Community</a></li>
              <li className="footer-link-item"><a href="https://twitter.com">Twitter / X</a></li>
            </ul>
          </Col>
        </Row>

        <Divider style={{ borderColor: 'rgba(255, 255, 255, 0.08)', margin: '24px 0' }} />

        <div className="footer-bottom-bar" style={{ borderTop: 'none', paddingTop: 0 }}>
          <Text style={{ color: 'var(--color-muted)', fontSize: 13 }}>
            &copy; {new Date().getFullYear()} Zode Technologies Inc. {t('footer.rights')}
          </Text>
          <Space size={16} wrap>
            <Text style={{ color: 'var(--color-muted)', fontSize: 13 }}>Version {CURRENT_RELEASE.version} ({CURRENT_RELEASE.channel})</Text>
            <Text style={{ color: 'var(--color-muted)', fontSize: 13 }}>•</Text>
            <Text style={{ color: 'var(--color-muted)', fontSize: 13 }}>{t('footer.crafted')}</Text>
          </Space>
        </div>
      </div>
    </footer>
  );
};
