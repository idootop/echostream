import { defineConfig } from "tsdown";

// tsdown 构建：仅产出 ESM 包（dist/），源码布局与产物布局同构（相对导入一致）
export default defineConfig({
  entry: {
    index: "index.ts",
    postcard: "postcard.ts",
    "examples/client": "examples/client.ts",
    "examples/server": "examples/server.ts",
    "test/codec.test": "test/codec.test.ts",
    "test/cross_client": "test/cross_client.ts",
    "test/cross_server": "test/cross_server.ts",
    "test/server.test": "test/server.test.ts",
  },
  format: ["esm"],
  target: "node20",
  // 包为 "type": "module"，统一 .js 扩展名
  outExtension: () => ({ js: ".js" }),
  // 原生二进制与 WASM 胶水不打包（运行时按相对路径加载）
  deps: { neverBundle: [/echostream-node\.node$/, /echostream_wasm/] },
  sourcemap: false,
  clean: true,
});
