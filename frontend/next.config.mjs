import { createRequire } from 'module';
const require = createRequire(import.meta.url);

/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'standalone',

  cacheHandler: require.resolve('./cache-handler.js'),
  cacheMaxMemorySize: 0,

  images: {
    unoptimized: true,
  },

  reactStrictMode: true,
  devIndicators: false,

  logging: false,
};

export default nextConfig;
