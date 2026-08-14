import React from 'react';
import { Modal } from 'antd';
import { DownloadSection } from './DownloadSection';

interface DownloadModalProps {
  open: boolean;
  onClose: () => void;
}

export const DownloadModal: React.FC<DownloadModalProps> = ({ open, onClose }) => {
  return (
    <Modal
      open={open}
      onCancel={onClose}
      footer={null}
      width={880}
      className="download-full-modal"
      centered
    >
      <DownloadSection id="modal-downloads" />
    </Modal>
  );
};
