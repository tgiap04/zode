import React from 'react';
import { AppleFilled } from '@ant-design/icons';

export const AppleEcosystem: React.FC = () => {
  return (
    <section className="ecosystem-section">
      <div className="container">
        <AppleFilled className="ecosystem-icon" />
        <h2 className="ecosystem-title">Tinh chỉnh tối đa cho Apple Ecosystem.</h2>
        <p className="ecosystem-desc">
          Hoạt động trơn tru với các tiêu chuẩn của macOS, giải quyết triệt để <br />
          các rắc rối về PluginKit hay môi trường ảo hóa.
        </p>
      </div>
    </section>
  );
};
