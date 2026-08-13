// WebSocket 场景浏览器 E2E（ws:// 无证书，应直接打通）
const { chromium } = require('/Users/del/X/App/Rust/echostream/tools/e2e/node_modules/playwright');
const { spawn } = require('node:child_process');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
(async () => {
  const root = '/Users/del/X/App/Rust/echostream';
  // 启动 WS 服务端
  const server = spawn('cargo', ['run', '-p', 'echostream-ws', '--example', 'ws_chat_server', '--release'], { cwd: root, stdio: 'pipe' });
  server.stdout.on('data', (d) => process.stdout.write('[server] ' + d));
  await sleep(3000);
  // 静态页面服务
  const http = spawn('python3', ['-m', 'http.server', '8080', '--directory', `${root}/sdk/web`], { stdio: 'ignore' });
  await sleep(1000);
  try {
    const browser = await chromium.launch({ headless: true });
    const page = await browser.newPage();
    page.on('pageerror', (e) => console.log('[page-error]', e.message));
    await page.goto('http://127.0.0.1:8080/e2e.html', { timeout: 15000 });
    await page.waitForFunction(() => typeof window.__e2e === 'function', null, { timeout: 10000 });
    const results = await page.evaluate((u) => window.__e2e(u), 'ws://127.0.0.1:8081');
    console.log('E2E 结果:', JSON.stringify(results));
    if (!results.connected || results.sum !== 30 || !results.done) {
      throw new Error('断言失败: ' + JSON.stringify(results));
    }
    console.log('🎉 WebSocket 浏览器端到端测试通过（RPC/事件/流，零证书）');
    await browser.close();
  } finally {
    server.kill();
    http.kill();
  }
})().catch((e) => { console.error('❌ 失败:', e.message); process.exit(1); });
