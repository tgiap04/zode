import { useState, useEffect } from 'react';
import type { OSPlatform, DownloadOption } from '../types';
import { CURRENT_RELEASE } from '../data/releases';

export function useOS() {
  const [platform, setPlatform] = useState<OSPlatform>('macos-arm64');
  const [recommendedDownload, setRecommendedDownload] = useState<DownloadOption>(
    CURRENT_RELEASE.downloads[0]
  );

  useEffect(() => {
    if (typeof window === 'undefined') return;

    const userAgent = window.navigator.userAgent.toLowerCase();
    const platformStr = window.navigator.platform?.toLowerCase() || '';

    let detected: OSPlatform = 'macos-arm64';

    if (platformStr.includes('mac') || userAgent.includes('macintosh') || userAgent.includes('mac os')) {
      const isIntel = userAgent.includes('intel');
      detected = isIntel ? 'macos-x64' : 'macos-arm64';
    } else if (platformStr.includes('win') || userAgent.includes('windows')) {
      detected = 'windows-x64';
    } else if (platformStr.includes('linux') || userAgent.includes('linux') || userAgent.includes('x11')) {
      detected = userAgent.includes('aarch64') || userAgent.includes('arm') ? 'linux-arm64' : 'linux-x64';
    } else {
      detected = 'macos-arm64';
    }

    setPlatform(detected);

    const match = CURRENT_RELEASE.downloads.find((d) => d.platform === detected) || CURRENT_RELEASE.downloads[0];
    setRecommendedDownload(match);
  }, []);

  return {
    platform,
    recommendedDownload,
    allDownloads: CURRENT_RELEASE.downloads,
    cliCommands: CURRENT_RELEASE.cliInstallCommands,
    version: CURRENT_RELEASE.version,
  };
}
