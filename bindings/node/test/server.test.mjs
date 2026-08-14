// EchoStream Node.js server + client 端到端测试（新 DX：自动编解码）
import assert from "node:assert/strict";
import { connect, ServerBuilder } from "../index.js";

async function main() {
  // ===== Node 侧服务端（新 DX：参数自动解码展开、返回值自动编码） =====
  const builder = new ServerBuilder();
  builder.bind("127.0.0.1:5001");

  const events = [];
  const clientEvents = [];
  const streamFrames = [];

  builder.addRpc("add", async (a, b) => {
    console.log(`[node-server] add(${a}, ${b})`);
    return a + b;
  });
  builder.addRpc("echo", async (text) => text);
  builder.addRpc("getUser", async (id) => ({ id, name: "alice" }));
  builder.addEvent("hello", (data) => {
    events.push(data);
    console.log(`[node-server] 收到事件: ${data}`);
  });
  builder.addStream("chat", async (receiver) => {
    while (true) {
      const frame = await receiver.recv();
      if (frame === null) break;
      streamFrames.push(frame);
      console.log(`[node-server] 流帧: ${frame}`);
    }
    console.log(`[node-server] 流 chat 结束，共 ${streamFrames.length} 帧`);
  });

  const server = await builder.build();
  console.log(`[node-server] 监听 ${server.addr()}`);
  const runPromise = server.run(); // 后台运行
  await new Promise((r) => setTimeout(r, 200));

  // ===== Node 侧客户端（新 DX：request("add", 10, 20) -> 30） =====
  const client = await connect("127.0.0.1:5001");
  console.log("[node-client] 已连接");

  // RPC：多参数自动元组 + 响应自动解码
  const sum = await client.request("add", 10, 20);
  assert.equal(sum, 30);
  console.log(`[node-client] add(10, 20) = ${sum}`);

  // RPC：字符串往返
  assert.equal(await client.request("echo", "hello node"), "hello node");
  console.log("[node-client] echo 往返一致");

  // RPC：结构体载荷与响应
  const user = await client.request("getUser", 7, { decode: { id: "number", name: "string" } });
  assert.deepEqual(user, { id: 7, name: "alice" });
  console.log("[node-client] getUser 结构体往返一致");

  // 事件：自动编码/解码
  await client.emit("hello", "from node client");

  // 客户端监听事件（验证服务端广播）
  client.onEvent("hello", (data) => clientEvents.push(data));

  // 客户端也注册处理器（双向通信）
  client.onRpc("ping", async () => "pong");
  const sessions = server.sessions();
  assert.ok(sessions.length >= 1, "应存在在线会话");
  for (const s of sessions) {
    const reply = await s.request("ping");
    assert.equal(reply, "pong");
    console.log("[node-server] 主动调用客户端 ping ->", reply);
  }

  // 流：帧自动编码
  const stream = await client.createStream("chat");
  for (let i = 0; i < 3; i++) await stream.send(`node stream ${i}`);
  await stream.finish();

  // 服务端广播
  await server.broadcast("hello", "broadcast!");

  await new Promise((r) => setTimeout(r, 300));
  client.close();
  server.shutdown();
  await runPromise;

  assert.deepEqual(events, ["from node client"], "服务端事件未全部收到");
  assert.deepEqual(clientEvents, ["broadcast!"], "客户端未收到广播");
  assert.deepEqual(streamFrames, ["node stream 0", "node stream 1", "node stream 2"], "流帧不符");
  console.log("🎉 Node.js server + client 端到端测试通过（自动编解码 + 双向通信 + 流）");
  process.exit(0); // napi runtime 线程会阻止自然退出
}

main().catch((e) => {
  console.error("❌ 测试失败:", e);
  process.exit(1);
});
