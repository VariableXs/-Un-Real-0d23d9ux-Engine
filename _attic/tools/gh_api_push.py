"""github.com:443 被重置、api.github.com 仍可达时的快进推送（Git Data API）。

用法：在项目根目录执行
    python _attic/tools/gh_api_push.py [本地 commit，默认 HEAD]

原理：不传 packfile，改用 REST 建 blob → 建 tree（base_tree = 远端当前 commit 的
tree，因此只需列出本次改动的文件）→ 建 commit → PATCH refs/heads/main 快进。
远端 ref 必须与本地提交的父节点一致（真快进），否则拒绝推送。
"""
import base64
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request

REPO = "VariableXs/-Un-Real-0d23d9ux-Engine"
API = f"https://api.github.com/repos/{REPO}"


def sh(args, binary=False):
    r = subprocess.run(args, capture_output=True, check=True)
    return r.stdout if binary else r.stdout.decode("utf-8", "replace")


def token():
    out = subprocess.run(
        ["git", "credential", "fill"],
        input="protocol=https\nhost=github.com\n\n",
        capture_output=True,
        text=True,
    ).stdout
    for line in out.splitlines():
        if line.startswith("password="):
            return line.split("=", 1)[1]
    raise SystemExit("git credential 里没有 github.com 的凭据")


T = token()


def api(method, path, payload=None):
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(
        f"{API}/{path}",
        data=data,
        method=method,
        headers={
            "Authorization": f"Bearer {T}",
            "Accept": "application/vnd.github+json",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        raise SystemExit(f"HTTP {e.code} on {method} {path}: {e.read().decode('utf-8', 'replace')[:600]}")


def main():
    commit = sys.argv[1] if len(sys.argv) > 1 else "HEAD"
    local = sh(["git", "rev-parse", commit]).strip()
    parent = sh(["git", "rev-parse", f"{commit}^"]).strip()

    remote = api("GET", "git/ref/heads/main")["object"]["sha"]
    force = bool(int(os.environ.get("GHPUSH_FORCE", "0")))
    local_tree = sh(["git", "rev-parse", f"{commit}^{{tree}}"]).strip()
    if remote != parent:
        # 只允许在强制模式下纠正「内容相同但 sha 不同」的兄弟提交：远端树的
        # 内容必须和本地完全一致，否则无条件拒绝（防止真的覆盖别人的提交）。
        if not force:
            raise SystemExit(
                f"远端 main={remote[:7]} 不是本地提交的父节点 {parent[:7]} —— 非快进，拒绝推送"
                "（确认是同内容的兄弟提交时可设 GHPUSH_FORCE=1 纠正）"
            )
        have = api("GET", f"git/commits/{remote}")["tree"]["sha"]
        if have != local_tree:
            raise SystemExit(
                f"远端 main={remote[:7]} 的内容与本地提交不同（tree {have[:7]} != {local_tree[:7]}）"
                " —— 不是可纠正的兄弟提交，拒绝强推"
            )
        print(f"纠正兄弟提交：远端 {remote[:7]} 与本地内容一致，改为指向本地提交")

    # A/M/C 取出文件内容；D 记删除（GitHub 用 sha=null 表示删除该路径）
    status = sh(["git", "-c", "core.quotepath=false", "diff-tree", "-r",
                 "--no-commit-id", "--name-status", local]).splitlines()
    items = []
    deleted = []
    for line in status:
        if not line.strip():
            continue
        parts = line.split("\t")
        st, path = parts[0], parts[-1]
        if st.startswith("D"):
            deleted.append(path)
            continue
        blob = sh(["git", "show", f"{local}:{path}"], binary=True)
        created = api("POST", "git/blobs", {
            "content": base64.b64encode(blob).decode(),
            "encoding": "base64",
        })
        # 用提交本身的 ls-tree 查 mode（当前 index 对历史提交里已被删除的路径会落空）
        mode = sh(["git", "ls-tree", local, "--", path]).split()[0]
        if len(mode) == 5 and mode.startswith("100"):
            mode = "100644" if mode.endswith("644") else "100755"
        else:
            mode = "100644"
        items.append({"path": path, "mode": mode, "type": "blob", "sha": created["sha"]})
    for d in deleted:
        items.append({"path": d, "mode": "100644", "type": "blob", "sha": None})

    tree = api("POST", "git/trees", {"base_tree": parent, "tree": items})
    local_tree = sh(["git", "rev-parse", f"{commit}^{{tree}}"]).strip()
    if tree["sha"] != local_tree:
        raise SystemExit(
            f"API 建的树 {tree['sha'][:8]} 与本地树 {local_tree[:8]} 不一致 —— "
            "推送内容与本地提交有偏差，拒绝继续（排查 items 的 mode/路径/删除项）"
        )
    # 提交消息必须取**原始字节**：`git log --format=%B` 会多补一个尾换行，
    # 消息一变，commit sha 就再也对不上本地的了。
    raw = sh(["git", "cat-file", "commit", local], binary=True)
    msg = raw.partition(b"\n\n")[2].decode("utf-8")
    # 关键：把本地提交的 author/committer（含时间戳）原样带给 GitHub，这样远端
    # 生成的 commit 对象字节完全一致 → **sha 与本地相同**。漏了这一步，远端会
    # 生成一个「内容一样但 sha 不同」的兄弟提交，本地和的远端历史就此分叉，
    # 下次再推就不是快进了。
    # 注意：`%cI` 是最后一个字段，输出会带上 git log 的尾换行 → 必须逐个 strip，
    # 否则日期非法、GitHub 会退回当前时间，commit sha 就再也对不上了。
    ident = [x.strip() for x in sh(["git", "log", "-1",
             "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI", local]).split("\0")]
    author = {"name": ident[0], "email": ident[1], "date": ident[2]}
    committer = {"name": ident[3], "email": ident[4], "date": ident[5]}
    # 父节点必须用**本地提交的父**：快进场景下 remote == parent，二者等价；
    # 兄弟纠正场景（GHPUSH_FORCE=1）下 remote 是内容相同的旁支提交，拿它当
    # 父会造出「父指错」的第三个兄弟提交，sha 永远对不上本地 —— 必须用 parent。
    new = api("POST", "git/commits", {
        "message": msg, "tree": tree["sha"], "parents": [parent],
        "author": author, "committer": committer,
    })
    if new["sha"] != local:
        raise SystemExit(
            f"远端生成的 commit {new['sha'][:7]} 与本地 {local[:7]} 不一致 —— 未刷 ref，"
            "请先对齐（多半是 tree/blobs 或作者信息与本地有差异）"
        )
    res = api("PATCH", "git/refs/heads/main", {"sha": new["sha"], "force": force})
    subprocess.run(["git", "update-ref", "refs/remotes/origin/main", new["sha"]], check=True)
    print(f"PUSHED {res['object']['sha'][:7]}  files={len(items)} (removed {len(deleted)})")


if __name__ == "__main__":
    main()
