"""任务47 QEMU 先行验证 —— 引擎心跳模拟器（复用 47631 语义）。

等价 VARIX VM 内 agent（src-tauri/src/vm_agent.rs）在宿主侧的替身：
  - 监听 127.0.0.1:47631，收到以 b"PING" 开头的字节即回 "READY pid=<pid>\\n"
    （与 vm_agent::spawn 的握手语义一致）。
  - 收到 "MARK <n>" 将引擎「内存内容序号」写入持久化 sidecar 文件
    （模拟「内存快照写差分盘」的内容落盘；该文件位于差分盘镜像同目录）。
  - 收到 "GET" 回报当前内容序号。
  - 启动后若设 --callback host:port（或环境变量 VAR_HOST_ADDR），主动回连 host:47632
    发送 READY，模拟 vm_agent::notify_host（宿主可据此判定引擎已就绪）。
  - 进程被 kill = 引擎崩溃/断电；宿主编排器据此走超时/重启路径。

用法：python _attic/p47-engine-agent.py --port 47631 --content-file <path> [--callback 127.0.0.1:47632]
"""
import argparse
import os
import socket
import sys
import threading
import time

HEARTBEAT_PORT = 47631
HOST_CALLBACK_PORT = 47632


def load_content(path):
    try:
        with open(path, "r") as f:
            return int(f.read().strip() or "0")
    except (OSError, ValueError):
        return 0


def save_content(path, n):
    try:
        with open(path, "w") as f:
            f.write(str(n))
        return True
    except OSError:
        return False


def notify_host(host, msg):
    try:
        with socket.create_connection((host, HOST_CALLBACK_PORT), timeout=3) as s:
            s.sendall(msg.encode())
        return True
    except OSError:
        return False


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=HEARTBEAT_PORT)
    ap.add_argument("--content-file", required=True)
    ap.add_argument("--callback", default=os.environ.get("VAR_HOST_ADDR", ""))
    args = ap.parse_args()

    content = {"n": load_content(args.content_file)}
    lock = threading.Lock()
    pid = os.getpid()

    def handle(conn):
        try:
            conn.settimeout(2.0)
            data = conn.recv(64)
            if not data:
                return
            if data[:4] == b"PING":
                conn.sendall(("READY pid=%d\n" % pid).encode())
            elif data[:4] == b"MARK":
                try:
                    n = int(data[4:].strip())
                except ValueError:
                    return
                with lock:
                    content["n"] = n
                    save_content(args.content_file, n)
                conn.sendall(("OK %d\n" % n).encode())
            elif data[:3] == b"GET":
                with lock:
                    n = content["n"]
                conn.sendall(("SEQ %d\n" % n).encode())
        except (OSError, socket.timeout):
            pass
        finally:
            try:
                conn.close()
            except OSError:
                pass

    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", args.port))
    srv.listen(16)
    srv.settimeout(1.0)

    if args.callback:
        host = args.callback.split(":")[0]
        notify_host(host, "READY\n")

    print("[agent] listening on 127.0.0.1:%d pid=%d content=%d" %
          (args.port, pid, content["n"]), flush=True)
    while True:
        try:
            conn = srv.accept()[0]
        except socket.timeout:
            continue
        threading.Thread(target=handle, args=(conn,), daemon=True).start()


if __name__ == "__main__":
    sys.exit(main())
