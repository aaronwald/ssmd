import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",
  // Perspective uses WASM — Turbopack handles this natively,
  // but we need webpack for the asyncWebAssembly experiment.
  webpack: (config) => {
    config.experiments = { ...config.experiments, asyncWebAssembly: true };
    return config;
  },
  turbopack: {},
};

export default nextConfig;
