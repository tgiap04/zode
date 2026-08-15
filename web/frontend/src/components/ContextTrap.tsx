import React from 'react';

export const ContextTrap: React.FC = () => {
  return (
    <section className="context-trap-section section-padding">
      <div className="container trap-grid">
        <div className="trap-pain">
          <h3>The Context Switching Trap</h3>
          <ul>
            <li>RAM cạn kiệt vì Electron wrappers</li>
            <li>Luồng tư duy đứt đoạn khi đổi cửa sổ</li>
            <li>Workspace lộn xộn, plugin xung đột</li>
          </ul>
        </div>
        <div className="trap-solution">
          Zode giữ bạn lại với dòng code. <br />
          <span>Một không gian duy nhất</span> cho mọi thứ bạn cần.
        </div>
      </div>
    </section>
  );
};
