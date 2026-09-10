import type { NextConfig } from 'next';

// The default build remains a Cloudflare Worker for hosted deployments. The
// single-host Testnet deployment uses Vinext's self-contained Node artifact so
// the web process can stay on loopback behind the same Nginx origin as the Rust
// API.
const nextConfig: NextConfig =
  process.env.WCASH_EXPLORER_STANDALONE === 'true'
    ? { output: 'standalone' }
    : {};

export default nextConfig;
