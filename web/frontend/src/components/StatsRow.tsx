import React from 'react';
import { useTranslation } from 'react-i18next';

export const StatsRow: React.FC = () => {
  const { t } = useTranslation();

  const metrics = [
    {
      id: 'fps',
      value: '120',
      unit: 'FPS',
      title: 'Native Rendering',
      desc: t('stats.fpsLabel'),
    },
    {
      id: 'startup',
      value: '< 5',
      unit: 'ms',
      title: 'Cold Launch',
      desc: t('stats.startupLabel'),
    },
    {
      id: 'memory',
      value: '68',
      unit: 'MB',
      title: 'RAM Usage',
      desc: t('stats.memoryLabel'),
    },
    {
      id: 'latency',
      value: '0.9',
      unit: 'ms',
      title: 'Key Latency',
      desc: t('stats.latencyLabel'),
    },
  ];

  return (
    <section className="telemetry-strip-section">
      <div className="container-marketing">
        <div className="telemetry-strip-dock">
          {metrics.map((item) => (
            <div key={item.id} className="telemetry-cell">
              <div className="telemetry-value-row">
                <span className="telemetry-number">{item.value}</span>
                <span className="telemetry-unit">{item.unit}</span>
              </div>
              <div className="telemetry-title">{item.title}</div>
              <div className="telemetry-desc">{item.desc}</div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
};
