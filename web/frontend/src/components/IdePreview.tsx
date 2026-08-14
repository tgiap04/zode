import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Tag, Input, Button, Badge, Space, Typography } from 'antd';
import {
  RobotOutlined,
  SendOutlined,
  ThunderboltFilled,
  DatabaseOutlined,
  CloudServerOutlined,
  AppstoreOutlined,
} from '@ant-design/icons';

const { Title, Paragraph, Text } = Typography;

export const IdePreview: React.FC = () => {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<'rust' | 'react' | 'db' | 'docker'>('rust');

  return (
    <section className="ide-showcase-section">
      <div className="container-marketing">
        <div className="section-header-editorial">
          <Text className="eyebrow">{t('idePreview.eyebrow')}</Text>
          <Title level={2} className="display-lg" style={{ marginTop: 8, marginBottom: 16 }}>
            {t('idePreview.title')}
          </Title>
          <Paragraph className="subtitle">
            {t('idePreview.subtitle')}
          </Paragraph>
        </div>

        <div className="ide-window-wrapper">
          {/* Top Window Bar */}
          <div className="ide-top-bar">
            <div style={{ display: 'flex', alignItems: 'center' }}>
              <div className="ide-window-controls">
                <div className="ide-window-dot dot-red"></div>
                <div className="ide-window-dot dot-yellow"></div>
                <div className="ide-window-dot dot-green"></div>
              </div>

              <div className="ide-tab-bar">
                {/* Tab 1: Rust Backend Repo */}
                <div
                  className={`ide-tab-item ${activeTab === 'rust' ? 'active' : ''}`}
                  onClick={() => setActiveTab('rust')}
                >
                  <span>🦀</span>
                  <span>auth_service.rs</span>
                  <Tag color="#FF5B37" style={{ fontSize: 10, padding: '0 4px', margin: 0, borderRadius: 4 }}>
                    Backend
                  </Tag>
                </div>

                {/* Tab 2: React Frontend Repo */}
                <div
                  className={`ide-tab-item ${activeTab === 'react' ? 'active' : ''}`}
                  onClick={() => setActiveTab('react')}
                >
                  <span>⚛️</span>
                  <span>Dashboard.tsx</span>
                  <Tag color="#3B82F6" style={{ fontSize: 10, padding: '0 4px', margin: 0, borderRadius: 4 }}>
                    Frontend
                  </Tag>
                </div>

                {/* Tab 3: Native Database Studio */}
                <div
                  className={`ide-tab-item ${activeTab === 'db' ? 'active' : ''}`}
                  onClick={() => setActiveTab('db')}
                >
                  <DatabaseOutlined style={{ color: '#06B6D4' }} />
                  <span>PostgreSQL (users)</span>
                  <Tag color="#059669" style={{ fontSize: 10, padding: '0 4px', margin: 0, borderRadius: 4 }}>
                    Connected
                  </Tag>
                </div>

                {/* Tab 4: Docker Pod Manager */}
                <div
                  className={`ide-tab-item ${activeTab === 'docker' ? 'active' : ''}`}
                  onClick={() => setActiveTab('docker')}
                >
                  <CloudServerOutlined style={{ color: '#8B5CF6' }} />
                  <span>Docker (api-gw)</span>
                  <Tag color="#8B5CF6" style={{ fontSize: 10, padding: '0 4px', margin: 0, borderRadius: 4 }}>
                    Running
                  </Tag>
                </div>
              </div>
            </div>

            <Space size={8}>
              <Tag color="#10B981" style={{ borderRadius: 9999, fontSize: 11 }}>
                <ThunderboltFilled /> 120 FPS
              </Tag>
              <Tag color="#FF5B37" style={{ borderRadius: 9999, fontSize: 11 }}>
                <AppstoreOutlined /> 3 Repos Open
              </Tag>
            </Space>
          </div>

          {/* Workspace Body */}
          <div className="ide-workspace-grid">
            {/* Sidebar File Tree */}
            <div className="ide-sidebar-pane">
              <div className="ide-tree-title">{t('idePreview.explorer')}</div>

              {/* Repo 1 */}
              <div className="ide-tree-node" style={{ color: '#FF5B37', fontWeight: 600 }}>
                📁 repo: auth-backend
              </div>
              <div className="ide-tree-node active" style={{ paddingLeft: 20 }}>
                <span>🦀</span> auth_service.rs
              </div>
              <div className="ide-tree-node" style={{ paddingLeft: 20 }}>
                <span>🦀</span> jwt_validator.rs
              </div>

              {/* Repo 2 */}
              <div className="ide-tree-node" style={{ color: '#3B82F6', fontWeight: 600, marginTop: 8 }}>
                📁 repo: web-frontend
              </div>
              <div className="ide-tree-node" style={{ paddingLeft: 20 }}>
                <span>⚛️</span> Dashboard.tsx
              </div>

              {/* Native Database Integration */}
              <div className="ide-tree-node" style={{ color: '#06B6D4', fontWeight: 600, marginTop: 8 }}>
                <DatabaseOutlined /> DB: postgres-prod
              </div>
              <div className="ide-tree-node" style={{ paddingLeft: 20 }}>
                <span>📊</span> public.users (1.2M rows)
              </div>
              <div className="ide-tree-node" style={{ paddingLeft: 20 }}>
                <span>📊</span> public.sessions
              </div>

              {/* Docker & Pods */}
              <div className="ide-tree-node" style={{ color: '#8B5CF6', fontWeight: 600, marginTop: 8 }}>
                <CloudServerOutlined /> Docker: 4 containers
              </div>
              <div className="ide-tree-node" style={{ paddingLeft: 20 }}>
                <span>🟢</span> api-gateway:8080
              </div>
              <div className="ide-tree-node" style={{ paddingLeft: 20 }}>
                <span>🟢</span> redis-cache:6379
              </div>
            </div>

            {/* Editor Code / DB / Docker View Pane */}
            <div className="ide-code-pane">
              {activeTab === 'rust' && (
                <div>
                  <p><span style={{ color: '#F43F5E' }}>use</span> zode_db::&#123;PgPool, QueryBuilder&#125;;</p>
                  <p><span style={{ color: '#F43F5E' }}>use</span> zode_docker::ContainerRuntime;</p>
                  <p><br /></p>
                  <p><span style={{ color: '#6B7280' }}>// ⚡ Native high-throughput async handler</span></p>
                  <p><span style={{ color: '#F43F5E' }}>pub async fn</span> <span style={{ color: '#8B5CF6' }}>authenticate_user</span>(pool: &amp;<span style={{ color: '#06B6D4' }}>PgPool</span>, token: &amp;<span style={{ color: '#06B6D4' }}>str</span>) -&gt; <span style={{ color: '#06B6D4' }}>Result</span>&lt;<span style={{ color: '#06B6D4' }}>User</span>&gt; &#123;</p>
                  <p style={{ paddingLeft: 20 }}>let user = sqlx::<span style={{ color: '#8B5CF6' }}>query_as!</span>(User, <span style={{ color: '#10B981' }}>"SELECT * FROM users WHERE auth_token = $1"</span>, token)</p>
                  <p style={{ paddingLeft: 40 }}>.<span style={{ color: '#8B5CF6' }}>fetch_one</span>(pool).<span style={{ color: '#F43F5E' }}>await</span>?;</p>
                  <p style={{ paddingLeft: 20 }}><span style={{ color: '#059669' }}>Ok</span>(user)</p>
                  <p>&#125;</p>
                </div>
              )}

              {activeTab === 'react' && (
                <div>
                  <p><span style={{ color: '#F43F5E' }}>import</span> React, &#123; useEffect, useState &#125; <span style={{ color: '#F43F5E' }}>from</span> <span style={{ color: '#10B981' }}>'react'</span>;</p>
                  <p><span style={{ color: '#F43F5E' }}>import</span> &#123; <span style={{ color: '#06B6D4' }}>useLiveMetrics</span> &#125; <span style={{ color: '#F43F5E' }}>from</span> <span style={{ color: '#10B981' }}>'@zode/hooks'</span>;</p>
                  <p><br /></p>
                  <p><span style={{ color: '#F43F5E' }}>export const</span> <span style={{ color: '#8B5CF6' }}>Dashboard</span> = () =&gt; &#123;</p>
                  <p style={{ paddingLeft: 20 }}>const &#123; fps, activeRepos, dbStatus &#125; = <span style={{ color: '#8B5CF6' }}>useLiveMetrics</span>();</p>
                  <p style={{ paddingLeft: 20 }}>return &lt;<span style={{ color: '#06B6D4' }}>SuperIdeView</span> repos=&#123;activeRepos&#125; db=&#123;dbStatus&#125; /&gt;;</p>
                  <p>&#125;</p>
                </div>
              )}

              {activeTab === 'db' && (
                <div>
                  <p style={{ color: '#06B6D4', fontWeight: 600 }}>-- 🗄️ Native PostgreSQL Query Studio</p>
                  <p><span style={{ color: '#F43F5E' }}>SELECT</span> id, email, role, created_at <span style={{ color: '#F43F5E' }}>FROM</span> users <span style={{ color: '#F43F5E' }}>WHERE</span> status = <span style={{ color: '#10B981' }}>'active'</span> <span style={{ color: '#F43F5E' }}>LIMIT</span> 3;</p>
                  <p><br /></p>
                  <div style={{ background: '#161B22', border: '1px solid #30363D', borderRadius: 6, padding: '8px 12px', fontSize: 12, fontFamily: 'monospace' }}>
                    <div style={{ color: '#58A6FF', borderBottom: '1px solid #30363D', paddingBottom: 4 }}>
                      | id | email | role | created_at |
                    </div>
                    <div style={{ paddingTop: 4, color: '#C9D1D9' }}>
                      | 1042 | dev@zode.build | admin | 2026-08-15 01:20:00 |<br />
                      | 1043 | alex@rust.org | developer | 2026-08-15 01:21:40 |<br />
                      | 1044 | sarah@scale.io | lead | 2026-08-15 01:22:15 |
                    </div>
                  </div>
                  <p style={{ marginTop: 8, color: '#10B981', fontSize: 11 }}>✓ Query executed in 0.42ms (3 rows retrieved)</p>
                </div>
              )}

              {activeTab === 'docker' && (
                <div>
                  <p style={{ color: '#8B5CF6', fontWeight: 600 }}>-- 🐳 Docker Container: api-gateway (Port 8080)</p>
                  <p style={{ color: '#6B7280' }}>[2026-08-15T01:24:12Z INFO api_gateway] Listening on 0.0.0.0:8080</p>
                  <p style={{ color: '#10B981' }}>[2026-08-15T01:24:15Z DEBUG route] GET /api/v1/auth/session -&gt; 200 OK (0.8ms)</p>
                  <p style={{ color: '#10B981' }}>[2026-08-15T01:24:18Z DEBUG route] POST /api/v1/query/db -&gt; 200 OK (1.2ms)</p>
                  <p style={{ color: '#58A6FF' }}>[2026-08-15T01:24:22Z INFO pod_health] Health check passed: CPU 2.1% | RAM 28MB</p>
                </div>
              )}
            </div>

            {/* AI Assistant Pane */}
            <div className="ide-ai-panel-pane">
              <div className="ai-header-bar">
                <span>
                  <RobotOutlined style={{ marginRight: 6, color: '#FF5B37' }} />
                  {t('idePreview.aiAssistant')}
                </span>
                <Badge
                  count={t('idePreview.acpLive')}
                  style={{ backgroundColor: '#FF5B37', fontSize: 10 }}
                />
              </div>

              <div className="ai-bubble-box">
                <div style={{ fontWeight: 600, color: '#58A6FF', marginBottom: 4 }}>
                  {t('idePreview.aiContextTitle')}
                </div>
                {t('idePreview.aiContextDesc')}
              </div>

              <div style={{ marginTop: 'auto', display: 'flex', gap: 8 }}>
                <Input
                  placeholder={t('idePreview.aiPlaceholder')}
                  disabled
                  style={{
                    borderRadius: 9999,
                    background: '#161B22',
                    borderColor: '#30363D',
                    color: '#FFF',
                    fontSize: 12,
                  }}
                />
                <Button
                  type="primary"
                  shape="circle"
                  icon={<SendOutlined />}
                  style={{
                    backgroundColor: '#FF5B37',
                    borderColor: '#FF5B37',
                  }}
                />
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
};
