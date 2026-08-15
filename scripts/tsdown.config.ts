import { defineConfig } from "tsdown";

// 跨端 E2E 脚本：仅产出 ESM（dist/cross_e2e.js）
export default defineConfig({
  entry: { "cross_e2e": "cross_e2e.ts" },
  format: ["esm"],
  target: "node20",
  outExtension: () => ({ js: ".js" }),
  sourcemap: false,
  clean: true,
});
