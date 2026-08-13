// EchoStream 真实浏览器端到端测试（Playwright + Chromium）
// 验证：WebTransport 连接 → RPC / 事件 / 流 全链路
import { chromium } from "playwright";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const hexToBytes = (hex) => new Uint8Array(hex.match(/.{2}/g).map((b) => parseInt(b, 16)));

async function main() {
  // 启动 WebTransport 服务端
  const server = spawn(
    "cargo",
    ["run", "-p", "echostream-web", "--example", "web_chat_server", "--release"],
    { cwd: root, stdio: "pipe", env: { ...process.env, RUST_LOG: "wtransport=debug,quinn=debug" } }
  );
  let certHashHex = null;
  server.stdout.on("data", (d) => {
    const text = d.toString();
    const m = text.match(/\[cert-hash\] ([0-9a-f]{64})/);
    if (m) certHashHex = m[1];
    process.stdout.write(`[server] ${text}`);
  });
  server.stderr.on("data", (d) => process.stderr.write(`[server-err] ${d}`));
  await sleep(3000);
  if (!certHashHex) throw new Error("未获取到服务端证书 hash");

  // 本地静态服务提供页面（file:// 的 ES module 受 CORS 限制）
  const http = spawn("python3", ["-m", "http.server", "8080", "--directory", `${root}/sdk/web`], { stdio: "ignore" });
  await sleep(1000);

  try {
    // 生成 CA 并签发服务器证书（Chrome 信任 CA 公钥 → 常规 PKI 验证通过）
  const { execSync } = await import("node:child_process");
  const caDir = "target/e2e-ca";
  execSync(
    `mkdir -p ${caDir} && ` +
    `openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -keyout ${caDir}/ca.key -out ${caDir}/ca.pem -days 30 -nodes -subj "/CN=EchoStream E2E CA" 2>/dev/null && ` +
    `openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -keyout ${caDir}/server.key -out ${caDir}/server.csr -nodes -subj "/CN=localhost" 2>/dev/null && ` +
    `printf 'subjectAltName = DNS:localhost, IP:127.0.0.1\nextendedKeyUsage = serverAuth\nkeyUsage = digitalSignature\n' > ${caDir}/server.ext && ` +
    `openssl x509 -req -in ${caDir}/server.csr -CA ${caDir}/ca.pem -CAkey ${caDir}/ca.key -CAcreateserial -out ${caDir}/server.pem -days 7 -extfile ${caDir}/server.ext 2>/dev/null`,
    { cwd: root }
  );
  console.log("CA 证书已生成:", caDir);
  // 服务端加载 CA 签发的证书（通过环境变量传给 web_chat_server）
  process.env.ECHO_CERT = `${root}/target/e2e-ca/server.pem`;
  process.env.ECHO_KEY = `${root}/target/e2e-ca/server.key`;

  // 三件套（videocall-rs 实践）：忽略证书错误 + 强制 QUIC + 干净 profile
  const browser = await chromium.launchPersistentContext("/tmp/echostream-chrome-profile", {
    headless: true,
    ignoreHTTPSErrors: true,
    channel: "chrome",
    args: [
      "--ignore-certificate-errors",
      "--allow-insecure-localhost",
      "--origin-to-force-quic-on=localhost:4433,127.0.0.1:4433",
    ],
  });
    const page = await browser.newPage();
    page.on("pageerror", (e) => console.log("[page-error]", e.message));
    page.on("console", (m) => console.log("[page-log]", m.text()));
    await page.goto("http://127.0.0.1:8080/e2e.html");
    await page.waitForFunction(() => typeof window.__e2e === "function", null, { timeout: 10000 });

    await page.evaluate((hex) => { window.__certHash = { hex }; }, certHashHex);
    const results = await page.evaluate(() => window.__e2e());
    console.log("E2E 结果:", JSON.stringify(results));

    if (!results.connected || results.sum !== 30 || !results.done) {
      throw new Error(`E2E 断言失败: ${JSON.stringify(results)}`);
    }
    console.log("🎉 真实浏览器端到端测试通过（RPC/事件/流）");
    await browser.close();
  } finally {
    server.kill();
    http.kill();
  }
}

main().catch((e) => {
  console.error("❌ E2E 失败:", e);
  process.exit(1);
});
