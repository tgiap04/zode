import React from 'react';
import { ThunderboltOutlined, ApiOutlined, SafetyCertificateOutlined } from '@ant-design/icons';

export const TechCore: React.FC = () => {
  return (
    <section className="tech-section section-padding container">
      <div className="tech-grid">
        <div className="tech-card">
          <ThunderboltOutlined style={{ fontSize: 32, color: '#f59e0b', marginBottom: 20 }} />
          <h4>Rust Core.</h4>
          <p>
            Render trực tiếp qua GPU, loại bỏ hoàn toàn giới hạn 
            của các IDE dựa trên Electron. Tốc độ khung hình 120 FPS.
          </p>
        </div>
        
        <div className="tech-card" style={{ borderColor: 'rgba(56, 189, 248, 0.2)' }}>
          <ApiOutlined style={{ fontSize: 32, color: '#38bdf8', marginBottom: 20 }} />
          <h4>O(1) Complexity.</h4>
          <p>
            Cấu trúc indexing siêu tốc. Chuyển đổi ngữ cảnh và 
            tìm kiếm đạt độ phức tạp O(1). Không độ trễ.
          </p>
        </div>
        
        <div className="tech-card">
          <SafetyCertificateOutlined style={{ fontSize: 32, color: '#10b981', marginBottom: 20 }} />
          <h4>Zero Memory Leaks.</h4>
          <p>
            Quản lý tiến trình ngầm và log bằng cấu trúc circular buffers, 
            đảm bảo RAM luôn ở mức tối ưu nhất dù mở file Gigabyte.
          </p>
        </div>
      </div>
    </section>
  );
};
