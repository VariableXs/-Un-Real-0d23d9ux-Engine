# -*- coding: utf-8 -*-
"""链式快进推送：从远端 main 起，按序用 Git Data API（raw utf-8 blob）重建并快进到本地 HEAD。
逐提交：blob（>20MB 文本走 raw）→ tree（sha 对账）→ commit（原样 author/committer）→ PATCH ref。
任一环节失败即停（远端停留在最后一个成功提交，可重跑续推）。
"""
import base64, json, os, subprocess, sys, urllib.request, urllib.error

REPO = "VariableXs/-Un-Real-0d23d9ux-Engine"
API = f"https://api.github.com/repos/{REPO}"
RAW_THRESHOLD = 20 * 1024 * 1024

def sh(args, binary=False):
    r = subprocess.run(args, capture_output=True, check=True)
    return r.stdout if binary else r.stdout.decode("utf-8", "replace")

def token():
    out = subprocess.run(["git", "credential", "fill"],
        input="protocol=https\nhost=github.com\n\n", capture_output=True, text=True).stdout
    for line in out.splitlines():
        if line.startswith("password="):
            return line.split("=", 1)[1]
    raise SystemExit("no github credential")

T = token()

def api(method, path, payload=None, raw_body=None):
    if raw_body is not None:
        data = raw_body
    elif payload is not None:
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    else:
        data = None
    req = urllib.request.Request(f"{API}/{path}", data=data, method=method, headers={
        "Authorization": f"Bearer {T}", "Accept": "application/vnd.github+json",
        "Content-Type": "application/json"})
    for attempt in range(3):
        try:
            with urllib.request.urlopen(req) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            raise SystemExit(f"HTTP {e.code} on {method} {path}: {e.read().decode('utf-8','replace')[:400]}")
        except urllib.error.URLError as e:
            if attempt == 2: raise SystemExit(f"URLError {method} {path}: {e}")
            print(f"  retry {attempt+1} after URLError")

def make_blob(content_bytes):
    if len(content_bytes) > RAW_THRESHOLD:
        try:
            text = content_bytes.decode("utf-8")
        except UnicodeDecodeError:
            text = None
        if text is not None:
            body = json.dumps({"content": text, "encoding": "utf-8"}, ensure_ascii=False).encode("utf-8")
            return api("POST", "git/blobs", raw_body=body)
    return api("POST", "git/blobs", {"content": base64.b64encode(content_bytes).decode(), "encoding": "base64"})

def push_one(local, parent):
    status = sh(["git", "-c", "core.quotepath=false", "diff-tree", "-r",
                 "--no-commit-id", "--name-status", local]).splitlines()
    items = []
    for line in status:
        if not line.strip():
            continue
        parts = line.split("\t")
        st, path = parts[0], parts[-1]
        if st.startswith("D"):
            items.append({"path": path, "mode": "100644", "type": "blob", "sha": None})
            continue
        blob = sh(["git", "show", f"{local}:{path}"], binary=True)
        created = make_blob(blob)
        mode = sh(["git", "ls-tree", local, "--", path]).split()[0]
        if len(mode) == 5 and mode.startswith("100"):
            mode = "100644" if mode.endswith("644") else "100755"
        else:
            mode = "100644"
        items.append({"path": path, "mode": mode, "type": "blob", "sha": created["sha"]})
        print(f"  blob ok: {path} ({len(blob)} B)", flush=True)
    local_tree = sh(["git", "rev-parse", f"{local}^{{tree}}"]).strip()
    tree = api("POST", "git/trees", {"base_tree": parent, "tree": items})
    if tree["sha"] != local_tree:
        raise SystemExit(f"tree mismatch {tree['sha'][:8]} != {local_tree[:8]}")
    raw = sh(["git", "cat-file", "commit", local], binary=True)
    msg = raw.partition(b"\n\n")[2].decode("utf-8")
    ident = [x.strip() for x in sh(["git", "log", "-1",
             "--format=%an|%ae|%aI|%cn|%ce|%cI", local]).split("|")]
    payload = {"message": msg, "tree": tree["sha"], "parents": [parent],
               "author": {"name": ident[0], "email": ident[1], "date": ident[2]},
               "committer": {"name": ident[3], "email": ident[4], "date": ident[5]}}
    newc = api("POST", "git/commits", payload)
    if newc["sha"] != local:
        raise SystemExit(f"commit sha mismatch {newc['sha'][:8]} != {local[:8]}")
    api("PATCH", "git/refs/heads/main", {"sha": local, "force": False})
    print(f"PUSHED {local[:8]}", flush=True)

def main():
    remote = api("GET", "git/ref/heads/main")["object"]["sha"]
    chain = sh(["git", "rev-list", "--reverse", f"{remote}..HEAD"]).split()
    print(f"remote={remote[:8]} chain={len(chain)}", flush=True)
    for c in chain:
        parent = sh(["git", "rev-parse", f"{c}^"]).strip()
        if parent != remote:
            raise SystemExit(f"non-ff at {c[:8]} (parent {parent[:8]} != remote {remote[:8]})")
        push_one(c, remote)
        remote = c
    print("CHAIN ALL PUSHED", flush=True)

if __name__ == "__main__":
    main()
