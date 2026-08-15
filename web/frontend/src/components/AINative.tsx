import React from 'react';
import { ArrowRightOutlined, BugOutlined, RobotOutlined, CodeOutlined } from '@ant-design/icons';

export const AINative: React.FC = () => {
  return (
    <section className="ai-section section-padding container">
      <div className="ai-header">
        <h2 className="ai-title">AI hiểu toàn bộ <span style={{ color: 'var(--brand-coral)' }}>hệ thống</span> của bạn.</h2>
        <p className="ai-desc">
          Đừng copy-paste thủ công nữa. AI của Zode đọc hiểu file bạn đang mở, 
          log đang chạy và schema của database để tự động đưa ra giải pháp chính xác nhất.
        </p>
      </div>

      <div className="ai-flowchart">
        <div className="ai-node">
          <BugOutlined style={{ marginRight: 8, color: '#f43f5e' }} />
          Terminal Logs
        </div>
        <ArrowRightOutlined className="ai-arrow" />
        <div className="ai-node highlight">
          <RobotOutlined style={{ marginRight: 8 }} />
          Zode Context Engine
        </div>
        <ArrowRightOutlined className="ai-arrow" />
        <div className="ai-node">
          <CodeOutlined style={{ marginRight: 8, color: '#10b981' }} />
          Auto Fix & Refactor
        </div>
      </div>
    </section>
  );
};
