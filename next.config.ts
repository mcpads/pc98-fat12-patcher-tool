import type { NextConfig } from 'next';

const sourceRevision = process.env.RETROGAME_PATCHER_SOURCE_REVISION;
if (sourceRevision !== undefined && !/^[0-9a-f]{40}$/.test(sourceRevision)) {
  throw new Error('RETROGAME_PATCHER_SOURCE_REVISION must be a full lowercase Git commit SHA');
}

const nextConfig: NextConfig = {
  generateBuildId: async () => sourceRevision ?? null,
  output: 'export',
};

export default nextConfig;
