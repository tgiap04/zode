import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip, Space, Typography } from 'antd';
import { ThunderboltFilled, ClockCircleOutlined } from '@ant-design/icons';

const { Text } = Typography;

export const PromoBanner: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div className="promo-banner">
      <Space size="middle" wrap align="center" style={{ justifyContent: 'center' }}>
        <span>
          <ThunderboltFilled style={{ color: '#FF5B37', marginRight: 6 }} />
          <Text strong style={{ color: '#FFFFFF' }}>{t('promo.announcement')}</Text>
          <span style={{ color: 'rgba(255, 255, 255, 0.85)', marginLeft: 6 }}>— {t('promo.desc')}</span>
        </span>

        <Tooltip title={t('common.comingSoon')} placement="bottom">
          <span
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: 4,
              cursor: 'not-allowed',
              textDecoration: 'underline',
              fontWeight: 600,
              color: '#FFFFFF',
              opacity: 0.9,
            }}
          >
            <span style={{ pointerEvents: 'none' }}>{t('promo.cta')}</span>
            <ClockCircleOutlined style={{ fontSize: 11, pointerEvents: 'none' }} />
          </span>
        </Tooltip>
      </Space>
    </div>
  );
};
