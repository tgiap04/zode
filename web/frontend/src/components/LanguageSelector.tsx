import React from 'react';
import { Segmented } from 'antd';
import { useLocaleSync, type SupportedLocale } from '../i18n/localeManager';

export const LanguageSelector: React.FC = () => {
  const { currentLocale, changeLocale } = useLocaleSync();

  return (
    <Segmented<SupportedLocale>
      value={currentLocale}
      onChange={(val) => changeLocale(val as SupportedLocale)}
      options={[
        { label: '🇬🇧 EN', value: 'en' },
        { label: '🇻🇳 VI', value: 'vi' },
      ]}
      className="ant-segmented-custom"
      style={{
        fontWeight: 600,
        fontSize: 12,
      }}
    />
  );
};
