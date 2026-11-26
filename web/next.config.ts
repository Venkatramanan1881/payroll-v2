import type { NextConfig } from "next";

// The browser calls /v1/* on this origin; Next forwards to the Rust API, so no CORS setup is needed.
const api = process.env.API_URL ?? "http://localhost:8080";

const config: NextConfig = {
  async rewrites() {
    return [{ source: "/v1/:path*", destination: `${api}/v1/:path*` }];
  },
};

export default config;
