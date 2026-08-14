import React from 'react';
import { CodeOutlined, BugOutlined } from '@ant-design/icons';

export const ZigZagFeatures: React.FC = () => {
  return (
    <section className="zigzag-section container">
      {/* Block 1 */}
      <div className="zigzag-row">
        <div className="zigzag-text">
          <h3>Chuyển đổi dự án nhanh như chớp.</h3>
          <p>
            Quản lý song song hàng chục dự án. Khôi phục trạng thái làm việc 
            ngay lập tức mà không cần mở lại thư mục. Global Activity Bar 
            siêu mỏng giữ mọi thứ trong tầm tay.
          </p>
        </div>
        <div className="zigzag-visual">
          <div className="glass-card-inner">
            <div style={{ display: 'flex', gap: 16, alignItems: 'center' }}>
              <div style={{ padding: 12, background: 'rgba(255,255,255,0.05)', borderRadius: 12 }}><CodeOutlined style={{ fontSize: 24, color: '#38bdf8' }} /></div>
              <div>
                <h4 style={{ margin: 0, fontSize: 16 }}>Global Activity Bar</h4>
                <p style={{ margin: 0, color: '#a1a1aa', fontSize: 13 }}>Switch contexts instantly</p>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Block 2 */}
      <div className="zigzag-row reverse">
        <div className="zigzag-text">
          <h3>Code và Test trên cùng một màn hình.</h3>
          <p>
            Khởi chạy trực tiếp iPhone virtual machines ngay trong IDE. 
            Không cần cửa sổ phụ, luồng dữ liệu liên tục và liền mạch.
          </p>
        </div>
        <div className="zigzag-visual">
          <div className="glass-card-inner" style={{ width: '200px', height: '350px', borderRadius: 30, border: '4px solid #333' }}>
            <div style={{ height: 20, width: 100, background: '#000', margin: '0 auto', borderBottomLeftRadius: 10, borderBottomRightRadius: 10 }}></div>
            <div style={{ padding: 20, textAlign: 'center', marginTop: 40 }}>
              <p style={{ color: '#10b981' }}>Preview App</p>
              <div style={{ fontSize: 40, marginTop: 20 }}>📱</div>
            </div>
          </div>
        </div>
      </div>

      {/* Block 3 */}
      <div className="zigzag-row">
        <div className="zigzag-text">
          <h3>Terminal sinh ra để chịu tải.</h3>
          <p>
            Giữ luồng log của bạn luôn mượt mà và không bao giờ treo ứng dụng, 
            bất chấp lượng dữ liệu trả về lớn đến đâu. Tích hợp AI CLI và Docker Logs.
          </p>
        </div>
        <div className="zigzag-visual">
          <div className="glass-card-inner" style={{ width: '80%', fontFamily: 'var(--font-mono)', fontSize: 13 }}>
            <div style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
              <BugOutlined style={{ color: '#f43f5e' }} />
              <span style={{ color: '#a1a1aa' }}>docker logs api-gateway</span>
            </div>
            <div style={{ color: '#38bdf8' }}>[INFO] Worker process started</div>
            <div style={{ color: '#10b981' }}>[OK] Database connected in 2ms</div>
            <div style={{ color: '#f59e0b' }}>[WARN] Rate limit threshold near</div>
          </div>
        </div>
      </div>
    </section>
  );
};
