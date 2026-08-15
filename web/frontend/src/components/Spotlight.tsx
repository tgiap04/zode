import React from 'react';
import { SearchOutlined } from '@ant-design/icons';

export const Spotlight: React.FC = () => {
  return (
    <section className="spotlight-section section-padding">
      <div className="container spotlight-content">
        <h2 className="spotlight-title">Mọi thao tác,<br />chỉ cần một <span style={{ color: 'var(--brand-coral)' }}>tổ hợp phím</span>.</h2>
        <p className="spotlight-desc">
          Quên đi những menu thả xuống cồng kềnh. Tìm kiếm file, gọi AI, hay chạy script — 
          tất cả đều tức thì và chuẩn xác với tốc độ phản hồi cực đoan.
        </p>
        
        <div className="spotlight-search-mock">
          <SearchOutlined style={{ fontSize: '28px', color: 'rgba(255, 255, 255, 0.6)' }} />
          <input type="text" placeholder="Gõ lệnh hoặc tìm kiếm file..." disabled />
        </div>
      </div>
    </section>
  );
};
