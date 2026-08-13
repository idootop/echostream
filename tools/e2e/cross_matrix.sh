#!/usr/bin/env bash
# EchoStream 跨端 E2E 矩阵：6 个 Rust / Node / Python 交叉组合
#
# 每个组合：启动服务端 -> 等待 E2E_SERVER_READY -> 运行客户端 ->
#           检查客户端 add(10, 20) = 30 且退出码 0 ->
#           检查服务端收到事件(E2E_EVENT_RECEIVED)与 3 帧流(E2E_STREAM_FRAMES=3) ->
#           终止服务端。
#
# 端口：5110-5115，每个组合独立。任一失败记录并继续，最后汇总 PASS/FAIL 表。
# 用法：bash tools/e2e/cross_matrix.sh

set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RUST_PEER="$ROOT/target/release/examples/e2e_peer"
NODE_SERVER="$ROOT/bindings/node/test/cross_server.cjs"
NODE_CLIENT="$ROOT/bindings/node/test/cross_client.cjs"
PY_SERVER="$ROOT/bindings/python/tests/cross_server.py"
PY_CLIENT="$ROOT/bindings/python/tests/cross_client.py"
LOG_DIR="$ROOT/tools/e2e/.matrix-logs"

# ---------- 前置准备：编译 Rust 通用对端（首次或缺失时） ----------
if [ ! -x "$RUST_PEER" ]; then
  echo "== 编译 Rust 通用对端 (e2e_peer) =="
  (cd "$ROOT" && cargo build -p echostream --release --example e2e_peer) || {
    echo "❌ Rust e2e_peer 编译失败"
    exit 1
  }
fi

mkdir -p "$LOG_DIR"

PASS=0
FAIL=0
FAILED=""

# 轮询日志直到出现目标行，超时返回 1
wait_log() { # 日志文件 超时秒 关键字
  local log="$1" timeout="$2" key="$3"
  for _ in $(seq 1 $((timeout * 10))); do
    grep -qF "$key" "$log" 2>/dev/null && return 0
    sleep 0.1
  done
  return 1
}

# 运行单个组合
run_combo() { # 名称 端口 服务端命令 客户端命令
  local name="$1" port="$2" server_cmd="$3" client_cmd="$4"
  local slog="$LOG_DIR/$name.server.log" clog="$LOG_DIR/$name.client.log"
  : > "$slog"
  echo "== [$name] 启动服务端（端口 ${port}）=="
  # exec 使子 shell 被服务端进程替换，$! 即为服务端 PID，可精确 kill
  ( eval "exec $server_cmd" ) > "$slog" 2>&1 &
  local spid=$!

  if ! wait_log "$slog" 15 "E2E_SERVER_READY"; then
    echo "❌ FAIL [$name] 服务端未就绪，日志如下："
    cat "$slog"
    kill "$spid" 2>/dev/null
    wait "$spid" 2>/dev/null
    FAIL=$((FAIL + 1))
    FAILED="$FAILED $name"
    return
  fi

  echo "== [$name] 运行客户端 =="
  eval "$client_cmd" > "$clog" 2>&1
  local cexit=$?

  # 判定：客户端退出码 + 输出 + 服务端标记
  local ok="PASS"
  [ $cexit -ne 0 ] && ok="FAIL"
  grep -qF "add(10, 20) = 30" "$clog" || ok="FAIL"
  if ! wait_log "$slog" 5 "E2E_EVENT_RECEIVED"; then ok="FAIL"; fi
  if ! wait_log "$slog" 5 "E2E_STREAM_FRAMES=3"; then ok="FAIL"; fi

  # 终止服务端（先 SIGTERM，超时则 SIGKILL）
  kill "$spid" 2>/dev/null
  for _ in 1 2 3 4 5; do
    kill -0 "$spid" 2>/dev/null || break
    sleep 0.2
  done
  kill -0 "$spid" 2>/dev/null && kill -9 "$spid" 2>/dev/null
  wait "$spid" 2>/dev/null

  if [ "$ok" = "PASS" ]; then
    PASS=$((PASS + 1))
    echo "✅ PASS [$name]"
  else
    FAIL=$((FAIL + 1))
    FAILED="$FAILED $name"
    echo "❌ FAIL [$name]（client exit=${cexit}）"
    echo "---- 客户端日志 ----"
    cat "$clog"
    echo "---- 服务端日志 ----"
    cat "$slog"
  fi
}

# ---------- 6 个交叉组合（端口 5110-5115） ----------
run_combo "rust-server_node-client" 5110 \
  "$RUST_PEER --server --addr 127.0.0.1:5110" \
  "node $NODE_CLIENT 127.0.0.1:5110"

run_combo "node-server_rust-client" 5111 \
  "node $NODE_SERVER 5111" \
  "$RUST_PEER --client --addr 127.0.0.1:5111"

run_combo "rust-server_python-client" 5112 \
  "$RUST_PEER --server --addr 127.0.0.1:5112" \
  "python3 $PY_CLIENT 127.0.0.1:5112"

run_combo "python-server_rust-client" 5113 \
  "python3 $PY_SERVER 5113" \
  "$RUST_PEER --client --addr 127.0.0.1:5113"

run_combo "node-server_python-client" 5114 \
  "node $NODE_SERVER 5114" \
  "python3 $PY_CLIENT 127.0.0.1:5114"

run_combo "python-server_node-client" 5115 \
  "python3 $PY_SERVER 5115" \
  "node $NODE_CLIENT 127.0.0.1:5115"

# ---------- 汇总 ----------
echo ""
echo "========== 跨端 E2E 矩阵结果 =========="
echo "PASS: $PASS / 6    FAIL: $FAIL / 6"
if [ $FAIL -gt 0 ]; then
  echo "失败组合:$FAILED"
  echo "详细日志见: $LOG_DIR"
  exit 1
fi
echo "🎉 全部 6 个跨端组合通过，线缆格式跨端一致"
