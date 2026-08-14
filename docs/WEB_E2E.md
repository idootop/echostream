# Web 端浏览器联调（E2E）

## 验证覆盖现状

- ✅ WebTransport 服务端逻辑：wtransport 客户端端到端（RPC/事件/流全链路）
- ✅ 协议编解码：WASM 交叉验证（与 Rust 基准字节一致）
- ✅ 客户端状态机：RPC 匹配 / 事件路由 / 主动调用 / 流序号（6 项断言）
- ⚠️ 真实浏览器自动化：受各浏览器证书/协议策略限制（详见下）

## 浏览器兼容性调研结论（2026-08，本机 macOS 15.7）

| 浏览器 | WebTransport | 私有 CA 证书 | 结论 |
|--------|-------------|-------------|------|
| Chrome/Chromium | ✅ | ❌ QUIC 证书校验不遵循任何自动化信任机制（`--ignore-certificate-errors`、`spki-list`、`allow-insecure-localhost`、`serverCertificateHashes`、CA 方案 8 种组合逐一失败；服务端确认收到 QUIC 连接，拒绝发生在 TLS 证书层） | 需公网可信证书 |
| Firefox | ✅ | ❌ HTTP/3 证书验证强制要求 CA/Browser Forum 公网 CA；导入私有 CA + `disable_when_third_party_roots_found` 两个取值均失败（TCP HTTPS 同证书验证通过，确认是 HTTP/3 特有策略） | 需公网可信证书 |
| Safari | 26.4+ | ✅ 系统钥匙串信任 | 本机 26.3 未支持（需升级系统） |
| WebKit（Playwright） | ❌ 构建无 WebTransport | — | 不可用 |

服务端侧关键证据：Firefox 连接时 quinn TRACE 显示握手正常推进至 Handshake 阶段
（收到 ClientHello → 发送 ServerHello+证书 → 收到客户端 Handshake 包），随后客户端
发送 TLS alert 42（handshake_failure）中止 —— 证书验证在 QUIC 路径被拒。

## 推荐路径

### 方案 A：公网穿透 + 公网证书（完整自动化，需 VPS 或付费穿透）

1. 准备：公网服务器（或支持 UDP 的穿透服务如 ngrok 付费层 / frp）+ 域名
2. 域名解析到公网 IP，Let's Encrypt 签发证书（`certbot certonly --standalone`）
3. 本地启动 `web_chat_server` 加载公网证书（`ECHO_CERT`/`ECHO_KEY` 环境变量已支持）
4. UDP 端口转发到本地（frp UDP 模式 / ngrok TCP+UDP 等）
5. 浏览器打开 `https://公网域名/e2e.html`（本地静态服务见方案 B 第 4 步）—— 此时浏览器走标准公网 PKI，应直接通过

### 方案 B：Safari 手动验证（免费，需 macOS 26.4+）

1. 生成 CA 并签发证书（openssl 步骤见下）
2. 导入 CA 到钥匙串：`security add-trusted-cert -d -r trustRoot -k ~/Library/Keychains/login.keychain-db target/e2e-ca/ca.pem`
3. 启动服务端：`cargo run -p echostream-transport --example web_chat_server --release --features web`（默认自签）或加载 CA 证书
4. Safari 打开 `http://127.0.0.1:8080/e2e.html`（`python3 -m http.server 8080 --directory bindings/web`）
5. 页面自动执行 RPC / 事件 / 流，控制台可见结果

### 方案 C：Chrome 手动验证（任何版本）

1. 启动服务端，Chrome 访问 `https://127.0.0.1:4433` 点击"高级 → 继续访问"信任证书
2. 打开 `http://127.0.0.1:8080/e2e.html`（同上）
3. 页面自动执行并输出结果

## 自动化脚本

> 旧版 Playwright 自动化脚本（tools/e2e）已随仓库清理：受浏览器证书策略限制（下表），

## 排查记录（避免重复踩坑）

| 方案 | 结果 |
|------|------|
| Chrome：`--ignore-certificate-errors` / `ignoreHTTPSErrors` / `spki-list`（叶子/CA）/ `allow-insecure-localhost` / `origin-to-force-quic-on` + 新 profile / 三件套组合 | ✗ |
| Chrome：W3C `serverCertificateHashes`（DER hash 与 openssl 一致，Uint8Array/ArrayBuffer，合规证书 7 天 + serverAuth EKU） | ✗ |
| Firefox：导入私有 CA（certutil，TCP HTTPS 验证通过）+ `disable_when_third_party_roots_found` 设 false/true | ✗（HTTP/3 强制公网 CA） |
| WebKit（Playwright 26.5）：`typeof WebTransport` 为 undefined | ✗ |
| 系统 Safari 26.3：WebTransport 需 26.4 | ✗（版本不足） |
| curl --http3：libcurl 构建不支持 | ✗（本机） |
| 公网对照（webtransport.day）：本机代理环境 UDP 被阻断 | ✗（网络环境） |
