"""真发自检：等空闲 → 真发 → 核验 → 还原。

★ 为什么必须「等空闲」★
WorkBuddy 生成中时 `sending=true`，发送键的 class 是
`cr-send-button--stop`——此时点击是「停止」，不是「发送」。
本工具的忙闲保护会**故意拒绝**（这是安全设计，不是缺陷）。
所以要测真发，必须等到 `sending=false`。

★ 安全约定（本脚本严格遵守）★
1. 探针文本唯一可识别（`VAPROBE_<随机>`），便于精确回收。
2. 发送后**必定**把输入框还原，不留孤儿文字。
3. 勾了 --yes 才真发；否则只干跑。
4. 全程打印每一步，不黑盒。

用法：
  python send_selftest.py            # 只干跑，安全
  python send_selftest.py --yes      # 真发一轮（需空闲）
  python send_selftest.py --yes --keep  # 真发且不清理（人工确认用）
"""

import argparse
import json
import os
import random
import sys
import time
import urllib.request

PORT = 9222
URL = f"http://127.0.0.1:{PORT}"


def devtools_alive() -> bool:
    try:
        with urllib.request.urlopen(f"{URL}/json/version", timeout=2) as r:
            return b"Browser" in r.read(400)
    except Exception:
        return False


def wait_idle(timeout: int = 300, poll: float = 2.0) -> bool:
    """等WorkBuddy 空闲。超时返回 False。"""
    print("等待 WorkBuddy 空闲（生成中时不能发）...", end="", flush=True)
    t0 = time.time()
    while time.time() - t0 < timeout:
        s = probe_state()
        if s is None:
            print(" [读不到状态]")
            return False
        if not s["sending"]:
            print(f" 空闲（等了 {int(time.time()-t0)}s）")
            return True
        time.sleep(poll)
    print(f" 超时（{timeout}s 仍在生成）")
    return False


def probe_state():
    """读忙闲 + 输入框。走页面内脚本。

    注意 `chars`：空编辑器 innerText 是占位提示语（"今天帮你做些什么？@ 添加上下文…"，
    约 25 字），所以 `chars == 0` **不能**当"输入框已清空"用。
    需要真实字数请读 `charsReal`（已剔除占位提示语）。2026-10-06 实测修正。
    """
    js = r"""(() => {
      const ed = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
      const btn = document.querySelector('button.cr-send-button');
      const cls = btn ? String(btn.className) : '';
      const raw = ed ? (ed.innerText||'').trim() : '';
      let real = 0;
      if (ed) {
        if (/今天帮你做些什么|有什么我可以帮|How can I help/i.test(raw)) {
          real = 0;
        } else {
          let sum = 0;
          ed.querySelectorAll('[data-slate-string]').forEach(
            s => { sum += (s.textContent || '').length; });
          real = sum || raw.length;
        }
      } else {
        real = -1;
      }
      return {
        sending: /--sending|--stop/.test(cls),
        label: (btn && btn.getAttribute('aria-label')) || (btn && btn.title) || '',
        chars: raw.length,
        charsReal: real,
        hasEditor: !!ed,
      };
    })()"""
    r = call_js(js)
    return r


def call_js(expr: str):
    """通过 CDP WebSocket 执行一段 JS。最小实现，只用标准库。"""
    import struct
    import socket
    import base64
    import os

    # 1) 找 target
    with urllib.request.urlopen(f"{URL}/json/list", timeout=5) as r:
        lst = json.loads(r.read().decode("utf-8"))
    pages = [t for t in lst if t.get("type") == "page"
             and not t.get("url", "").startswith("devtools://")]
    if not pages:
        return None
    pages.sort(key=lambda t: len(t.get("url", "")))
    ws = pages[0]["webSocketDebuggerUrl"]

    # 2) 极简 WebSocket 客户端（只要 text frame，够用）
    m = re_ws(ws)
    host, port, path = m.group(1), int(m.group(2)), m.group(3)
    key = base64.b64encode(os.urandom(16)).decode()
    s = socket.create_connection((host, port), timeout=20)
    req = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {host}:{port}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n\r\n"
    )
    s.sendall(req.encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        d = s.recv(4096)
        if not d:
            raise RuntimeError("握手失败")
        buf += d

    def send_text(payload: bytes):
        hdr = bytearray([0x81])
        n = len(payload)
        if n < 126:
            hdr.append(0x80 | n)
        elif n < 65536:
            hdr.append(0x80 | 126)
            hdr += struct.pack(">H", n)
        else:
            hdr.append(0x80 | 127)
            hdr += struct.pack(">Q", n)
        mask = os.urandom(4)
        hdr += mask
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        s.sendall(bytes(hdr) + masked)

    def recv_text():
        def rd(n):
            out = b""
            while len(out) < n:
                d = s.recv(n - len(out))
                if not d:
                    raise RuntimeError("连接关闭")
                out += d
            return out
        b0, b1 = rd(2)
        op = b0 & 0x0F
        ln = b1 & 0x7F
        if ln == 126:
            ln = struct.unpack(">H", rd(2))[0]
        elif ln == 127:
            ln = struct.unpack(">Q", rd(8))[0]
        data = rd(ln) if ln else b""
        return op, data

    msg = json.dumps({
        "id": 1, "method": "Runtime.evaluate",
        "params": {"expression": expr, "returnByValue": True, "awaitPromise": True},
    })
    send_text(msg.encode())
    deadline = time.time() + 20
    while time.time() < deadline:
        op, data = recv_text()
        if op == 1:
            try:
                obj = json.loads(data.decode("utf-8", "replace"))
            except Exception:
                continue
            if obj.get("id") == 1:
                s.close()
                return obj.get("result", {}).get("result", {}).get("value")
    s.close()
    return None


import re


def re_ws(url: str):
    m = re.match(r"ws://([^:/]+):(\d+)(/.*)", url)
    if not m:
        raise RuntimeError(f"无法解析 ws 地址: {url}")
    return m


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--yes", action="store_true", help="真发（默认只干跑）")
    ap.add_argument("--keep", action="store_true", help="发完不清理")
    ap.add_argument("--wait", type=int, default=300, help="等空闲超时秒数")
    a = ap.parse_args()

    if not devtools_alive():
        print(f"[ERR] 9222 端口不通。先双击桌面「VarixAutoPilot 开端口.bat --kill」")
        return 1
    print("[OK] CDP 端点可达")

    st = probe_state()
    if not st:
        print("[ERR] 读不到页面状态")
        return 1
    print(f"初始状态: {json.dumps(st, ensure_ascii=False)}")

    if st["sending"]:
        if not wait_idle(a.wait):
            print("[ERR] 一直忙，未测。稍后再试。")
            return 3

    # 备份原内容（大概率是占位提示）
    before = call_js(
        """(()=>{const e=document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
        return e?(e.innerText||'').trim():'';})()"""
    ) or ""
    print(f"发前输入框: {len(before)} 字符 {before[:30]!r}")

    tag = "VAPROBE_" + "".join(random.choice("abcdef0123456789") for _ in range(6))
    text = f"{tag} 发送链路自检探针"
    print(f"探针: {text!r}")

    # 填入（CDP Input.insertText——execCommand 版不进 Slate 状态，D11）
    ok_fill, ev = fill_cdp(text)
    print(f"填入: {ev}")
    if not ok_fill:
        restore(before)
        print("[ERR] 填入失败，已还原输入框")
        return 4
    ch = probe_state()

    if not a.yes:
        # 干跑：还原后退出
        restore(before)
        print("[OK] 干跑通过（未发送），输入框已还原。加 --yes 可真发。")
        return 0

    print("★ 真发（点击发送键）...")
    rst = call_js(click_send_js())
    print(f"  点击返回: {rst}")

    # 等入流（body 全文查 tag——[data-message-author-role] 在本版 DOM 恒 0 个）
    ok = False
    tag_js = json.dumps(tag)
    for i in range(20):
        time.sleep(0.5)
        inb = call_js(
            f"(document.body.innerText||'').includes({tag_js})"
        )
        ch2 = probe_state() or {}
        print(f"  [{i+1}] 入流={inb} 编辑器={ch2.get('chars')} 忙={ch2.get('sending')}")
        if inb:
            ok = True
            break
        if ch2.get("chars", 99) == 0:
            ok = True
            break

    if not ok:
        print("[WARN] 10 秒内没确认入流。也可能已发出但消息节点选择器变了——")
        print("       手工看一眼 WorkBuddy 窗口确认。")

    if not a.keep:
        cur = probe_state() or {}
        if (cur.get("chars") or 0) > 0:
            restore(before)
            print(f"已清理：输入框还原为 {len(before)} 字符")
        else:
            print("输入框已是空的，无需清理")
    else:
        print("--keep 已指定：保留输入框现状，请手工处理")

    return 0 if ok else 5


JS_FOCUS_EDIT = r"""(() => {
  const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
  if (!e) return false;
  e.focus();
  return !!(document.activeElement === e || e.contains(document.activeElement));
})()"""


def call_cdp_method(method: str, params: dict, timeout: float = 20.0):
    """在 page target 的同一条 WS 通道上发任意 CDP 域方法。

    与 call_js 的区别：call_js 执行页面 JS（Runtime.evaluate）；
    本函数调用 CDP 域方法（如 Input.insertText——模拟真实 IME 输入，
    走浏览器可信输入管线，Slate/React 一定注册）。
    """
    import struct as _struct
    import socket as _socket
    import base64 as _base64
    import re as _re

    with urllib.request.urlopen(f"{URL}/json/list", timeout=5) as r:
        lst = json.loads(r.read().decode("utf-8"))
    pages = [t for t in lst if t.get("type") == "page"
             and not t.get("url", "").startswith("devtools://")]
    if not pages:
        raise RuntimeError("no page target")
    pages.sort(key=lambda t: len(t.get("url", "")))
    m = _re.match(r"ws://([^:/]+):(\d+)(/.*)", pages[0]["webSocketDebuggerUrl"])
    host, port, path = m.group(1), int(m.group(2)), m.group(3)
    key = _base64.b64encode(os.urandom(16)).decode()
    s = _socket.create_connection((host, port), timeout=timeout)
    req = (f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\n"
           "Upgrade: websocket\r\nConnection: Upgrade\r\n"
           f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n")
    s.sendall(req.encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        d = s.recv(4096)
        if not d:
            raise RuntimeError("握手失败")
        buf += d

    def send_text(payload: bytes):
        hdr = bytearray([0x81])
        n = len(payload)
        if n < 126:
            hdr.append(0x80 | n)
        elif n < 65536:
            hdr.append(0x80 | 126)
            hdr += _struct.pack(">H", n)
        else:
            hdr.append(0x80 | 127)
            hdr += _struct.pack(">Q", n)
        mask = os.urandom(4)
        hdr += mask
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        s.sendall(bytes(hdr) + masked)

    def recv_text():
        def rd(n):
            out = b""
            while len(out) < n:
                d = s.recv(n - len(out))
                if not d:
                    raise RuntimeError("连接关闭")
                out += d
            return out
        b0, b1 = rd(2)
        op = b0 & 0x0F
        ln = b1 & 0x7F
        if ln == 126:
            ln = _struct.unpack(">H", rd(2))[0]
        elif ln == 127:
            ln = _struct.unpack(">Q", rd(8))[0]
        data = rd(ln) if ln else b""
        return op, data

    msg = json.dumps({"id": 1, "method": method, "params": params})
    send_text(msg.encode())
    deadline = time.time() + timeout
    result = None
    while time.time() < deadline:
        op, data = recv_text()
        if op == 1:
            try:
                obj = json.loads(data.decode("utf-8", "replace"))
            except Exception:
                continue
            if obj.get("id") == 1:
                result = obj
                break
    s.close()
    if result is None:
        raise RuntimeError(f"{method} 无响应")
    if "error" in result:
        raise RuntimeError(f"{method} 错误: {result['error']}")
    return result.get("result")


def fill_cdp(text: str) -> tuple:
    """CDP Input.insertText 填充编辑器。返回 (ok, 证据描述)。

    ★ 这是唯一可用的填充方法（D11，2026-10-06 实测定案）★
    旧 execCommand('insertText') 版：文字只进 DOM 不进 Slate 内部状态，
    React 认为编辑器为空 → 发送键呈「黑色」视觉、点击被空内容逻辑吞掉
    ——即用户看到的「有字但发送不了」。Input.insertText 是浏览器可信
    输入事件，端到端实弹已验证：消息入流 + AI 开始生成 + 新会话创建。
    """
    if not call_js(JS_FOCUS_EDIT):
        return False, "编辑器无法聚焦"
    try:
        call_cdp_method("Input.insertText", {"text": text})
    except Exception as e:
        return False, f"Input.insertText 失败：{e}"
    time.sleep(0.4)
    s = probe_state() or {}
    n = s.get("charsReal", s.get("chars", -1))
    if not isinstance(n, int) or n < 10:
        return False, f"填充后真实字数={n}（Slate 未注册）"
    return True, f"已填入（真实字数={n}）"


def click_send_js() -> str:
    return """(() => {
      const b = document.querySelector('button.cr-send-button:not(.cr-send-button--sending):not(.cr-send-button--stop)');
      if (!b) return { err: '找不到可点的发送键' };
      b.click();
      return { ok: true, cls: String(b.className) };
    })()"""


def clear_editor_cdp() -> bool:
    """CDP trusted 按键清空编辑器（Ctrl+A + Backspace）。

    实测（2026-10-06）：对 Slate 已注册的内容，execCommand delete 清不干净；
    trusted 按键必清。返回是否清空成功（charsReal==0）。
    """
    if not call_js(JS_FOCUS_EDIT):
        return False
    try:
        call_cdp_method("Input.dispatchKeyEvent", {
            "type": "rawKeyDown", "key": "a", "code": "KeyA",
            "windowsVirtualKeyCode": 65, "modifiers": 2})
        call_cdp_method("Input.dispatchKeyEvent", {
            "type": "keyUp", "key": "a", "code": "KeyA",
            "windowsVirtualKeyCode": 65, "modifiers": 2})
        time.sleep(0.2)
        call_cdp_method("Input.dispatchKeyEvent", {
            "type": "rawKeyDown", "key": "Backspace", "code": "Backspace",
            "windowsVirtualKeyCode": 8})
        call_cdp_method("Input.dispatchKeyEvent", {
            "type": "keyUp", "key": "Backspace", "code": "Backspace",
            "windowsVirtualKeyCode": 8})
        time.sleep(0.4)
    except Exception:
        return False
    s = probe_state() or {}
    return s.get("charsReal", -1) == 0


def restore(prev: str) -> None:
    """清空编辑器到 Slate 空状态（占位语自然显示，无需真的填回）。

    旧版用 execCommand insertText 把 prev「填回」——那正是 D11 孤儿文字
    的制造机（DOM 有字、Slate 空 → 发送键黑）。prev 参数保留仅为兼容。
    """
    if clear_editor_cdp():
        return
    # 兜底：execCommand DOM 清理（对 DOM 孤儿文字有效）
    call_js("""(() => {
      const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
      if (!e) return 0;
      e.focus();
      const sel = window.getSelection();
      const r = document.createRange();
      r.selectNodeContents(e);
      sel.removeAllRanges(); sel.addRange(r);
      document.execCommand('delete', false, null);
      return (e.innerText||'').trim().length;
    })()""")


if __name__ == "__main__":
    sys.exit(main())
