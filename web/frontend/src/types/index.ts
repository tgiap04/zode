export type OSPlatform = 'macos-arm64' | 'macos-x64' | 'windows-x64' | 'linux-x64' | 'linux-arm64' | 'unknown';

export interface DownloadOption {
  id: string;
  osName: string;
  arch: string;
  platform: OSPlatform;
  format: string;
  filename: string;
  downloadUrl: string;
  size: string;
  icon: string;
  badge?: string;
  recommended?: boolean;
}

export interface ReleaseInfo {
  version: string;
  releaseDate: string;
  channel: 'stable' | 'preview' | 'nightly';
  downloads: DownloadOption[];
  cliInstallCommands: {
    macos: string;
    linux: string;
    windows: string;
    cargo: string;
  };
}

export interface FeatureItem {
  id: string;
  title: string;
  subtitle: string;
  description: string;
  icon: string;
  tag: string;
  color?: string;
}

export interface BenchmarkItem {
  metric: string;
  unit: string;
  zode: number;
  vscode: number;
  cursor: number;
  description: string;
  isLowerBetter: boolean;
}
