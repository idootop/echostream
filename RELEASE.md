# EchoStream v0.1 发布手册

## 发布前检查清单

- [ ] `cargo clippy --all-features` 零警告
- [ ] `cargo build` 通过；CI 全绿（fmt / clippy / 示例冒烟 / WASM 验证 / Node E2E）
- [ ] 回归：`cargo run -p echostream --example simple_rpc` 输出"全部完成"
- [ ] `node bindings/web/echostream.test.mjs`、`node bindings/web/client_core.test.mjs` 通过
- [ ] `node bindings/node/test/server.test.mjs`、`node bindings/python/tests/test_e2e.py` 通过
- [ ] 跨端矩阵 `node scripts/cross_e2e.mjs` 6 组合全 PASS
- [ ] 版本号统一：workspace `Cargo.toml`（0.1.0）、`bindings/node/package.json`、`bindings/python/pyproject.toml`

## 1. crates.io 发布（依赖顺序严格执行）

```bash
# 需要 crates.io API token：cargo login
cargo publish -p echostream-proto
cargo publish -p echostream-core            # 框架 + 无 I/O 状态机
cargo publish -p echostream-transport       # QUIC/WS/WebTransport 传输
cargo publish -p echostream-derive
cargo publish -p echostream-discovery
cargo publish -p echostream                 # 最后发布统一入口
```

每个 crate 发布前先验证：`cargo package -p <crate> --allow-dirty --no-verify`

## 2. npm 发布（echostream-node）

```bash
cd bindings/node
cargo build -p echostream-node --release
# 按平台产出原生模块（macOS/Linux/Windows 分别构建）：
#   macOS:   cp target/release/libechostream_node.dylib echostream-node.node
#   Linux:   cp target/release/libechostream_node.so echostream-node.node
#   Windows: cp target/release/echostream_node.dll echostream-node.node
npm publish
```

跨平台产物建议后续引入 napi-rs 的多平台构建（`napi build --platform`）。

## 3. PyPI 发布（echostream）

```bash
cd bindings/python
pip install maturin
maturin build --release            # 本机平台 wheel
maturin publish                    # 或 maturin build --release --out dist 后 twine upload
```

多平台 wheel：`maturin build --release --target <triple>`（Linux/macOS/Windows，或 CI 矩阵）。

## 4. WASM 产物（Web SDK 依赖）

```bash
cargo build -p echostream-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir bindings/web/wasm target/wasm32-unknown-unknown/release/echostream_wasm.wasm
wasm-bindgen --target nodejs --out-dir bindings/wasm/node target/wasm32-unknown-unknown/release/echostream_wasm.wasm
```

Web SDK（`bindings/web/`）随仓库分发或单独 npm 包。

## 5. 发布后验证

- [ ] `cargo add echostream` 新项目跑通 simple_rpc
- [ ] `npm install echostream-node` 跑通 server.test.cjs
- [ ] `pip install echostream` 跑通 test_e2e.py
- [ ] docs.rs 文档生成正常（`cargo doc --all-features --no-deps`）

## 版本策略

- 0.1.x：协议层（proto）冻结后，上层可独立迭代
- 协议变更（Message 线缆格式）必须升 minor 并同步四端
- 每端版本独立（crates.io / npm / PyPI），但功能对齐
