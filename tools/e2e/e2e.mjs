// EchoStream 浏览器端到端测试（Playwright + Chromium）
//
// 两种模式：
//  1. 本地模式（默认）：启动本地 WebTransport 服务端 + 静态页面服务，
//     需要浏览器信任自签证书（受限，见 docs/WEB_E2E.md）
//  2. 公网模式：ECHO_URL=https://test.xbox.work —— 连接已部署的公网服务
//     （Let's Encrypt 证书，浏览器标准 PKI 验证直接通过）
import { chromium } from "playwright";
import { spawn, execSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  const externalUrl = process.env.ECHO_URL;
  let server = null;
  let http = null;

  if (!externalUrl) {
    // ===== 本地模式：启动 WebTransport 服务端 =====
    server = spawn(
      "cargo",
      ["run", "-p", "echostream-web", "--example", "web_chat_server", "--release"],
      { cwd: root, stdio: "pipe" }
    );
    server.stdout.on("data", (d) => process.stdout.write(`[server] ${d}`));
    server.stderr.on("data", (d) => process.stderr.write(`[server-err] ${d}`));
    await sleep(3000);

    // 生成 CA 并签发服务器证书
    const caDir = "target/e2e-ca";
    execSync(
      `mkdir -p ${caDir} && ` +
      `openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -keyout ${caDir}/ca.key -out ${caDir}/ca.pem -days 30 -nodes -subj "/CN=EchoStream E2E CA" 2>/dev/null && ` +
      `openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -keyout ${caDir}/server.key -out ${caDir}/server.csr -nodes -subj "/CN=localhost" 2>/dev/null && ` +
      `printf 'subjectAltName = DNS:localhost, IP:127.0.0.1\\nextendedKeyUsage = serverAuth\\nkeyUsage = digitalSignature\\n' > ${caDir}/server.ext && ` +
      `openssl x509 -req -in ${caDir}/server.csr -CA ${caDir}/ca.pem -CAkey ${caDir}/ca.key -CAcreateserial -out ${caDir}/server.pem -days 7 -extfile ${caDir}/server.ext 2>/dev/null`,
      { cwd: root }
    );
  }

  // 本地静态服务提供测试页面
  http = spawn("python3", ["-m", "http.server", "8080", "--directory", `${root}/bindings/web`], { stdio: "ignore" });
  await sleep(1000);

  try {
    const browser = await chromium.launch({ headless: true });
    const page = await browser.newPage();
    page.on("pageerror", (e) => console.log("[page-error]", e.message));
    await page.goto("http://127.0.0.1:8080/e2e.html", { timeout: 15000 });
    await page.waitForFunction(() => typeof window.__e2e === "function", null, { timeout: 10000 });

    const wtUrl = externalUrl || "https://127.0.0.1:4433";
    const results = await page.evaluate((u) => window.__e2e(u), wtUrl);
    console.log("E2E 结果:", JSON.stringify(results));

    if (!results.connected || results.sum !== 30 || !results.done) {
      throw new Error(`E2E 断言失败: ${JSON.stringify(results)}`);
    }
    console.log(`🎉 真实浏览器端到端测试通过（${wtUrl}）：RPC/事件/流`);
    await browser.close();
  } finally {
    if (server) server.kill();
    if (http) http.kill();
  }
}

main().catch((e) => {
  console.error("❌ E2E 失败:", e.message);
  process.exit(1);
});
