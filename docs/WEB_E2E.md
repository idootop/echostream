# Web 端浏览器联调（E2E）

## 状态

- ✅ 已验证：WebTransport 服务端（wtransport 客户端端到端）、WASM 编解码与状态机（交叉验证）、
  JS SDK 网络层逻辑
- ⚠️ 浏览器自动化（Playwright）证书信任：**环境限制** —— Chrome/Chromium 的 WebTransport（QUIC）
  证书校验不遵循 `--ignore-certificate-errors` / `ignoreHTTPSErrors` / `--ignore-certificate-errors-spki-list`
  等常规信任机制（已逐一实验确认，含 CA 签发 + serverAuth EKU + 7 天有效期证书 +
  W3C `serverCertificateHashes` API）。服务端确认收到 Chrome 的 QUIC 连接，拒绝发生在 TLS 证书层。

## 手动验证步骤（推荐）

1. 启动服务端：`cargo run -p echostream-web --example web_chat_server --release`
2. Chrome 访问 `https://127.0.0.1:4433`，点击"高级 → 继续访问"信任自签名证书
3. 打开 `http://127.0.0.1:8080/e2e.html`（或 `sdk/web/index.html`，需静态服务）：
   `python3 -m http.server 8080 --directory sdk/web`
4. 页面自动执行 RPC / 事件 / 流，控制台可见结果

## 自动化脚本

`tools/e2e/e2e.mjs`（Playwright + Chromium）已实现完整流程（启动服务端 → 生成 CA 证书 →
注入 hash → 页面执行断言），在证书信任机制生效的环境中可直接运行：

```bash
node tools/e2e/e2e.mjs
```

## 排查记录（避免重复踩坑）

| 方案 | 结果 |
|------|------|
| `--ignore-certificate-errors` | ✗ |
| `ignoreHTTPSErrors: true`（Playwright） | ✗（不覆盖 QUIC） |
| `--ignore-certificate-errors-spki-list`（叶子/CA） | ✗ |
| `--allow-insecure-localhost` | ✗ |
| `--origin-to-force-quic-on` + 新 profile | ✗ |
| CA 签发证书（serverAuth EKU，7 天） | ✗ |
| W3C `serverCertificateHashes`（Uint8Array/ArrayBuffer，DER hash 与 openssl 一致） | ✗ |
| 系统 Chrome（非 Playwright 构建） | ✗ |

关键证据：服务端日志 `New incoming QUIC connection` 后 Chrome 主动中止
（`certificate unknown, CERTIFICATE_VERIFY_FAILED`）—— 传输正常，证书策略拒绝。
