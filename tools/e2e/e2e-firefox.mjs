// EchoStream 真实浏览器端到端测试（Playwright + Firefox）
// 方案：本地 CA 签发服务端证书 → CA 导入 Firefox profile（Firefox 支持自定义 CA）
//       → WebTransport 走标准证书验证即可通过（Chrome 的 QUIC 证书忽略机制不可用）
import { firefox } from "playwright";
import { spawn, execSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  // 1. 生成 CA 并签发服务器证书
  const caDir = "target/e2e-ca";
  execSync(
    `mkdir -p ${caDir} && ` +
    `openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -keyout ${caDir}/ca.key -out ${caDir}/ca.pem -days 30 -nodes -subj "/CN=EchoStream E2E CA" 2>/dev/null && ` +
    `openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -keyout ${caDir}/server.key -out ${caDir}/server.csr -nodes -subj "/CN=localhost" 2>/dev/null && ` +
    `printf 'subjectAltName = DNS:localhost, IP:127.0.0.1\\nextendedKeyUsage = serverAuth\\nkeyUsage = digitalSignature\\n' > ${caDir}/server.ext && ` +
    `openssl x509 -req -in ${caDir}/server.csr -CA ${caDir}/ca.pem -CAkey ${caDir}/ca.key -CAcreateserial -out ${caDir}/server.pem -days 7 -extfile ${caDir}/server.ext 2>/dev/null`,
    { cwd: root }
  );

  // 2. 启动 WebTransport 服务端（加载 CA 签发的证书）
  const server = spawn(
    "cargo",
    ["run", "-p", "echostream-web", "--example", "web_chat_server", "--release"],
    { cwd: root, stdio: "pipe", env: { ...process.env, ECHO_CERT: `${root}/${caDir}/server.pem`, ECHO_KEY: `${root}/${caDir}/server.key`, NSPR_LOG_MODULES: "nsHttp3:5,nsNSSComponent:5,nsNSSErrors:5" } }
  );
  server.stdout.on("data", (d) => process.stdout.write(`[server] ${d}`));
  server.stderr.on("data", (d) => process.stderr.write(`[server-err] ${d}`));
  await sleep(3000);

  // 3. 本地静态服务提供测试页面
  const http = spawn("python3", ["-m", "http.server", "8080", "--directory", `${root}/sdk/web`], { stdio: "ignore" });
  await sleep(1000);

  const profileDir = "/tmp/echostream-firefox-profile";
  fs.rmSync(profileDir, { recursive: true, force: true });

  try {
    // 4. 首启生成 cert9.db，再导入 CA
    const first = await firefox.launchPersistentContext(profileDir, { headless: true });
    await first.close();
    execSync(`certutil -A -n "EchoStream E2E CA" -t "TCu,," -i ${root}/${caDir}/ca.pem -d sql:${profileDir}`, { stdio: "pipe" });
    console.log("CA 已导入 Firefox profile");

    // 5. 正式启动并执行测试（禁用 HTTP/3 第三方根证书限制）
    const browser = await firefox.launchPersistentContext(profileDir, {
      headless: true,
      firefoxUserPrefs: {
        "network.http.http3.disable_when_third_party_roots_found": true,
      },
      env: { NSPR_LOG_MODULES: "nsHttp3:5,nsNSSComponent:5,nsNSSErrors:5" },
    });
    const page = await browser.newPage();
    page.on("pageerror", (e) => console.log("[page-error]", e.message));
    await page.goto("http://127.0.0.1:8080/e2e.html", { timeout: 15000 });
    await page.waitForFunction(() => typeof window.__e2e === "function", null, { timeout: 10000 });

    const results = await page.evaluate(() => window.__e2e());
    console.log("E2E 结果:", JSON.stringify(results));

    if (!results.connected || results.sum !== 30 || !results.done) {
      throw new Error(`E2E 断言失败: ${JSON.stringify(results)}`);
    }
    console.log("🎉 真实浏览器（Firefox）端到端测试通过（RPC/事件/流）");
    await browser.close();
  } finally {
    server.kill();
    http.kill();
  }
}

main().catch((e) => {
  console.error("❌ E2E 失败:", e.message);
  process.exit(1);
});
