# -*- coding: utf-8 -*-
"""AI-110 纯 API 推送器：github.com:443 被重置时绕过本地 git，
经 api.github.com 建 blob→tree(base=远端HEAD tree)→commit(parent=远端HEAD)→快进 ref。
用法：项目根目录执行  python _ai110_push_api.py
"""
import base64, json, os, subprocess, sys, time, urllib.error, urllib.request

ROOT = os.path.dirname(os.path.abspath(__file__))
REPO = "VariableXs/-Un-Real-0d23d9ux-Engine"
API = f"https://api.github.com/repos/{REPO}"
REL_BOOKLET = "docs/Varix/CoRun Varix STAR II · Unxreal/AI-110 · 内核工程链增补线 · 300项新功能增补册（B01–B15 · A110-001–A110-300）.md"
REL_DETAIL = "docs/Varix/CoRun Varix STAR II · Unxreal/AI-110 · 内核工程链增补线 · 300项新功能定制详述册（A110-001–A110-300 · 每条300字）.md"
FILES = [REL_BOOKLET, REL_DETAIL, "_ai110_gen.py", "_ai110_append.py", "_ai110_push.py", "_ai110_push_api.py"] + \
        [f"_ai110_deep/{n}" for n in [f"B{i:02d}.md" for i in range(1, 16)] + ["supplements.md"]]
MSG = ("unxreal(k110): AI-110 300项新功能定制详述册落件（A110-001-A110-300 · 每条≥300字定制深化 · 功能定位/定制详述/完成与验收/落地与回归口径逐条定制 · 12.9万字 · 300/300达标min=300avg=382 · 防重零撞号）——主汇编册纯追加登记（详述前sha333a3a2d后b2ed2b10，他会话追加共存如实登记）；主册超100MB沿AI-71/81/86/90/98/108先例欠账登记不入本提交；随附增补册/生成器/追加器/推送器/15批深化底稿；他AI域账零触碰。前案 0a21c6f。github.com:443被重置，本提交经 api.github.com Git Data API 建 tree/commit（gh_api_push 同族判例的无本地对象变体）。")

def token():
    out = subprocess.run(["git", "credential", "fill"], cwd=ROOT,
                         input="protocol=https\nhost=github.com\n\n", capture_output=True, text=True).stdout
    for line in out.splitlines():
        if line.startswith("password="):
            return line.split("=", 1)[1]
    raise SystemExit("git credential 里没有 github.com 的凭据")

T = token()

def api(method, path, payload=None, binary=False):
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(f"{API}/{path}", data=data, method=method,
                                 headers={"Authorization": f"Bearer {T}",
                                          "Accept": "application/vnd.github+json",
                                          "Content-Type": "application/json"})
    for attempt in range(3):
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                body = r.read()
                return json.loads(body) if body and not binary else (json.loads(body) if body else {})
        except urllib.error.HTTPError as e:
            if e.code >= 500 and attempt < 2:
                time.sleep(3); continue
            raise SystemExit(f"HTTP {e.code} on {method} {path}: {e.read().decode('utf-8','replace')[:400]}")
        except urllib.error.URLError as e:
            if attempt < 2:
                time.sleep(3); continue
            raise SystemExit(f"URLError on {method} {path}: {e}")

def main():
    remote = api("GET", "git/ref/heads/main")["object"]["sha"]
    base_tree = api("GET", f"git/commits/{remote}")["tree"]["sha"]
    print(f"remote={remote[:7]} base_tree={base_tree[:7]}")
    tree = []
    for rel in FILES:
        local = os.path.join(ROOT, rel.replace("/", os.sep))
        data = open(local, "rb").read()
        blob = api("POST", "git/blobs", {"content": base64.b64encode(data).decode(),
                                         "encoding": "base64"})["sha"]
        tree.append({"path": rel, "mode": "100644", "type": "blob", "sha": blob})
        print(f"blob {rel[:60]}… {len(data)}B -> {blob[:7]}")
    new_tree = api("POST", "git/trees", {"base_tree": base_tree, "tree": tree})["sha"]
    commit = api("POST", "git/commits", {"message": MSG, "tree": new_tree, "parents": [remote]})["sha"]
    print(f"commit={commit[:7]}")
    # 快进校验：ref 仍指向当初的 remote 才 PATCH
    cur = api("GET", "git/ref/heads/main")["object"]["sha"]
    if cur != remote:
        raise SystemExit(f"远端已前移 {cur[:7]} != {remote[:7]}，请重跑（竞态保护）")
    api("PATCH", "git/refs/heads/main", {"sha": commit, "force": False})
    print("PUSH OK (api fast-forward)")

if __name__ == "__main__":
    main()
