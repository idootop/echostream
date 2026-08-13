// EchoStream Node.js binding 入口
// 原生模块 + 友好命名（Client / Server / ServerBuilder / Session / Stream）
const native = require("./echostream-node.node");

module.exports = {
  ...native,
  Client: native.JsClient,
  Server: native.JsServer,
  ServerBuilder: native.JsServerBuilder,
  Session: native.JsSession,
  Stream: native.JsStream,
};
