import { defineConfig } from "tsdown";

// tsdown 构建：仅产出 ESM 包（dist/）
// - 浏览器 SDK：dist/echostream.js（HTML 页面直接引用）
// - WASM 胶水外部化并复制到 dist/wasm（运行时相对路径加载不变）
export default defineConfig({
  entry: {
    echostream: "echostream.ts",
    "test/echostream.test": "echostream.test.ts",
    "test/client_core.test": "client_core.test.ts",
    "test/e2e.sdk.test": "e2e.sdk.test.ts",
  },
  format: ["esm"],
  target: "es2022",
  // 包为 "type": "module"，统一 .js 扩展名
  outExtension: () => ({ js: ".js" }),
  deps: { neverBundle: [/echostream_wasm/] },
  sourcemap: false,
  clean: true,
});
