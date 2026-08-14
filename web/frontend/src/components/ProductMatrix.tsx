import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Tag, Row, Col, Typography, Tooltip } from 'antd';
import { useTheme } from '../theme/ThemeContext';
import {
  ApartmentOutlined,
  DisconnectOutlined,
  LineChartOutlined,
} from '@ant-design/icons';

const { Title, Paragraph, Text } = Typography;

export const ProductMatrix: React.FC = () => {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const isDark = theme === 'dark';
  const [hoveredNode, setHoveredNode] = useState<string | null>(null);

  // Satellite nodes mapped relative to 500x400 SVG viewBox (Center at 250, 200)
  const nodes = [
    {
      id: 'db',
      name: t('productMatrix.nodeDb'),
      x: 250,
      y: 65,
      iconSrc: '/icons/database.svg',
      color: '#0284C7',
    },
    {
      id: 'terminal',
      name: t('productMatrix.nodeTerminal'),
      x: 120,
      y: 105,
      iconSrc: '/icons/terminal.svg',
      color: '#8B5CF6',
    },
    {
      id: 'github',
      name: t('productMatrix.nodeGithub'),
      x: 380,
      y: 105,
      iconSrc: '/icons/github.svg',
      color: isDark ? '#F3F4F6' : '#1F2937',
    },
    {
      id: 'docker',
      name: t('productMatrix.nodeDocker'),
      x: 75,
      y: 220,
      iconSrc: '/icons/docker.svg',
      color: '#2563EB',
    },
    {
      id: 'mobile',
      name: t('productMatrix.nodeMobile'),
      x: 425,
      y: 220,
      iconSrc: '/icons/mobile.svg',
      color: '#DB2777',
    },
    {
      id: 'web',
      name: t('productMatrix.nodeWeb'),
      x: 140,
      y: 335,
      iconSrc: '/icons/web.svg',
      color: '#059669',
    },
    {
      id: 'ai',
      name: t('productMatrix.nodeAi'),
      x: 360,
      y: 335,
      iconSrc: '/icons/ai.svg',
      color: '#D97706',
    },
  ];

  const centerX = 250;
  const centerY = 200;

  return (
    <section id="models-matrix" className="context-switching-section">
      <div className="container-marketing">
        <Row gutter={[48, 48]} align="middle">
          {/* Left Column: Problem Statement */}
          <Col xs={24} lg={11}>
            <div className="problem-statement-pane">
              <Tag className="problem-pill-tag">
                {t('productMatrix.badge')}
              </Tag>

              <Title
                level={2}
                className="problem-display-title"
                style={{
                  fontSize: 'clamp(36px, 4.2vw, 54px)',
                  fontWeight: 700,
                  lineHeight: 1.12,
                  letterSpacing: '-1.5px',
                  marginTop: 0,
                  marginBottom: 20,
                }}
              >
                {t('productMatrix.titleLine1')}<br />
                {t('productMatrix.titleLine2')}<br />
                {t('productMatrix.titleLine3')}
              </Title>

              <Paragraph
                className="problem-lead-desc"
                style={{
                  fontSize: 16,
                  lineHeight: 1.65,
                  marginBottom: 32,
                }}
              >
                {t('productMatrix.subtitle')}
              </Paragraph>

              <div className="problem-bullet-list">
                <div className="problem-bullet-item">
                  <div className="problem-bullet-icon-box">
                    <ApartmentOutlined style={{ fontSize: 16, color: '#2563EB' }} />
                  </div>
                  <Text strong className="problem-bullet-text">
                    {t('productMatrix.bullet1')}
                  </Text>
                </div>

                <div className="problem-bullet-item">
                  <div className="problem-bullet-icon-box">
                    <DisconnectOutlined style={{ fontSize: 16, color: '#2563EB' }} />
                  </div>
                  <Text strong className="problem-bullet-text">
                    {t('productMatrix.bullet2')}
                  </Text>
                </div>

                <div className="problem-bullet-item">
                  <div className="problem-bullet-icon-box">
                    <LineChartOutlined style={{ fontSize: 16, color: '#2563EB' }} />
                  </div>
                  <Text strong className="problem-bullet-text">
                    {t('productMatrix.bullet3')}
                  </Text>
                </div>
              </div>
            </div>
          </Col>

          {/* Right Column: Context Switching Constellation */}
          <Col xs={24} lg={13}>
            <div className="constellation-card-container">
              <svg
                viewBox="0 0 500 400"
                className="constellation-svg"
                preserveAspectRatio="xMidYMid meet"
              >
                <defs>
                  {/* Adaptive Ambient Glow */}
                  <radialGradient id="centerRadialGlow" cx="50%" cy="50%" r="50%">
                    <stop offset="0%" stopColor="#2563EB" stopOpacity={isDark ? 0.22 : 0.12} />
                    <stop offset="60%" stopColor="#1D4ED8" stopOpacity={isDark ? 0.06 : 0.03} />
                    <stop offset="100%" stopColor={isDark ? '#1E293B' : '#FFFFFF'} stopOpacity="0" />
                  </radialGradient>
                </defs>

                {/* Ambient Soft Glow in Center */}
                <circle cx={centerX} cy={centerY} r="170" fill="url(#centerRadialGlow)" />

                {/* Concentric Orbital Radar Rings */}
                <circle
                  cx={centerX}
                  cy={centerY}
                  r="70"
                  fill="none"
                  stroke={isDark ? 'rgba(255, 255, 255, 0.06)' : 'rgba(0, 0, 0, 0.06)'}
                  strokeWidth="1"
                  strokeDasharray="3 6"
                />
                <circle
                  cx={centerX}
                  cy={centerY}
                  r="135"
                  fill="none"
                  stroke={isDark ? 'rgba(255, 255, 255, 0.045)' : 'rgba(0, 0, 0, 0.045)'}
                  strokeWidth="1"
                  strokeDasharray="4 8"
                />
                <circle
                  cx={centerX}
                  cy={centerY}
                  r="190"
                  fill="none"
                  stroke={isDark ? 'rgba(255, 255, 255, 0.03)' : 'rgba(0, 0, 0, 0.03)'}
                  strokeWidth="1"
                />

                {/* Curved Connection Lines */}
                {nodes.map((node) => {
                  const isHovered = hoveredNode === node.id;
                  const dx = node.x - centerX;
                  const dy = node.y - centerY;
                  // Control point with subtle natural curve
                  const cx1 = centerX + dx * 0.45 - dy * 0.15;
                  const cy1 = centerY + dy * 0.45 + dx * 0.15;

                  const pathD = `M ${centerX} ${centerY} Q ${cx1} ${cy1} ${node.x} ${node.y}`;

                  const defaultStroke = isDark ? 'rgba(255, 255, 255, 0.14)' : 'rgba(0, 0, 0, 0.12)';

                  return (
                    <g key={`link-${node.id}`}>
                      {/* Background Guide Line */}
                      <path
                        d={pathD}
                        fill="none"
                        stroke={isHovered ? node.color : defaultStroke}
                        strokeWidth={isHovered ? 2.5 : 1.2}
                        strokeDasharray={isHovered ? 'none' : '4 4'}
                        className="constellation-path"
                        style={{
                          transition: 'all 0.3s ease',
                          opacity: isHovered ? 1 : 0.75,
                        }}
                      />

                      {/* Animated Traveling Particle Pulse */}
                      <circle r={isHovered ? 3.5 : 2.5} fill={isHovered ? node.color : '#2563EB'}>
                        <animateMotion
                          path={pathD}
                          dur={isHovered ? '2.2s' : '3.8s'}
                          repeatCount="indefinite"
                        />
                      </circle>
                    </g>
                  );
                })}
              </svg>

              {/* Satellite Tool Nodes with Image Icons positioned over SVG */}
              {nodes.map((node) => {
                const isHovered = hoveredNode === node.id;
                return (
                  <Tooltip key={node.id} title={node.name} placement="top">
                    <div
                      className={`constellation-node ${isHovered ? 'active' : ''}`}
                      style={{
                        left: `${(node.x / 500) * 100}%`,
                        top: `${(node.y / 400) * 100}%`,
                        borderColor: isHovered ? node.color : undefined,
                      }}
                      onMouseEnter={() => setHoveredNode(node.id)}
                      onMouseLeave={() => setHoveredNode(null)}
                    >
                      <img
                        src={node.iconSrc}
                        alt={node.name}
                        className="constellation-node-img"
                      />
                    </div>
                  </Tooltip>
                );
              })}

              {/* Center "You" Node with Developer Avatar Image */}
              <div
                className="constellation-center-node"
                style={{
                  left: `${(centerX / 500) * 100}%`,
                  top: `${(centerY / 400) * 100}%`,
                }}
              >
                <div className="center-node-avatar">
                  <img
                    src="/icons/user.svg"
                    alt="You"
                    className="center-node-img"
                  />
                </div>
                <span className="center-node-label">{t('productMatrix.youNode')}</span>
              </div>
            </div>
          </Col>
        </Row>
      </div>
    </section>
  );
};
