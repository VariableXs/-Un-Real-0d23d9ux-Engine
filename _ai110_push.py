# -*- coding: utf-8 -*-
"""AI-110 推送器：临时 worktree 自 origin/main 建单提交（父=远端 HEAD）→ gh_api_push 快进。
主册(>100MB blob 上限)沿 AI-71/81/86/90/98/108 判例不入 pathspec，欠账登记于提交信息。
竞态：远端移动则重置重建，最多 6 轮。
用法：项目根目录执行  python _ai110_push.py
"""
import subprocess, shutil, os, sys, json, urllib.request, time

ROOT = os.path.dirname(os.path.abspath(__file__))
WT = os.path.join(ROOT, "_attic", "_ai110_push_wt")
REPO = "VariableXs/-Un-Real-0d23d9ux-Engine"
API = f"https://api.github.com/repos/{REPO}"
REL_BOOKLET = "docs/Varix/CoRun Varix STAR II · Unxreal/AI-110 · 内核工程链增补线 · 300项新功能增补册（B01–B15 · A110-001–A110-300）.md"
REL_DETAIL = "docs/Varix/CoRun Varix STAR II · Unxreal/AI-110 · 内核工程链增补线 · 300项新功能定制详述册（A110-001–A110-300 · 每条300字）.md"
FILES = [
    REL_BOOKLET,
    REL_DETAIL,
    "_ai110_gen.py",
    "_ai110_append.py",
    "_ai110_push.py",
] + [f"_ai110_deep/{n}" for n in
     [f"B{i:02d}.md" for i in range(1, 16)] + ["supplements.md"]]
MSG = ("unxreal(k110): AI-110 300项新功能定制详述册落件（A110-001-A110-300 · 每条≥300字定制深化 · 功能定位/定制详述/完成与验收/落地与回归口径逐条定制 · 12.9万字 · 300/300达标min=300avg=382 · 防重零撞号）——主汇编册纯追加登记（详述前sha333a3a2d后b2ed2b10，他会话追加共存如实登记）；主册超100MB沿AI-71/81/86/90/98/108先例欠账登记不入pathspec；随附增补册/生成器/追加器/推送器/15批深化底稿；他AI域账零触碰。前案 commit 0a21c6f。")

def sh(args, cwd=ROOT, check=True):
    r = subprocess.run(args, cwd=cwd, capture_output=True)
    out = (r.stdout or b"").decode("utf-8", "replace") + (r.stderr or b"").decode("utf-8", "replace")
    if check and r.returncode != 0:
        raise RuntimeError(f"{args} -> {out[:500]}")
    return out.strip()

def token():
    out = subprocess.run(["git", "credential", "fill"], cwd=ROOT,
                         input="protocol=https\nhost=github.com\n\n", capture_output=True, text=True).stdout
    for line in out.splitlines():
        if line.startswith("password="):
            return line.split("=", 1)[1]
    raise SystemExit("git credential 里没有 github.com 的凭据")

def remote_head():
    req = urllib.request.Request(f"{API}/git/ref/heads/main",
                                 headers={"Authorization": f"Bearer {token()}",
                                          "Accept": "application/vnd.github+json"})
    with urllib.request.urlopen(req) as r:
        return json.load(r)["object"]["sha"]

def build(remote_sha):
    sh(["git", "fetch", "--quiet", "origin", "main"], check=False)
    if os.path.exists(WT):
        sh(["git", "worktree", "remove", "--force", WT])
    sh(["git", "worktree", "add", "--detach", WT, remote_sha])
    for rel in FILES:
        dst = os.path.join(WT, rel.replace("/", os.sep))
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copyfile(os.path.join(ROOT, rel.replace("/", os.sep)), dst)
    sh(["git", "add", "--"] + FILES, cwd=WT)
    sh(["git", "-c", "core.quotepath=false", "commit", "-m", MSG, "--"] + FILES, cwd=WT)
    return sh(["git", "rev-parse", "HEAD"], cwd=WT)

def gh_push(local_sha):
    env = dict(os.environ)
    r = subprocess.run([sys.executable, os.path.join(ROOT, "_attic", "tools", "gh_api_push.py"), local_sha],
                       cwd=ROOT, capture_output=True, env=env)
    out = (r.stdout or b"").decode("utf-8", "replace") + (r.stderr or b"").decode("utf-8", "replace")
    print(out[-1500:])
    return r.returncode == 0

def main():
    for attempt in range(1, 7):
        try:
            remote = remote_head()
            print(f"[{attempt}/6] remote={remote[:7]}")
            local = build(remote)
            print(f"local commit={local[:7]}")
            if gh_push(local):
                print("PUSH OK")
                return 0
        except Exception as e:
            print("attempt failed:", e)
        time.sleep(3)
    print("PUSH FAILED after 6 attempts")
    return 1

if __name__ == "__main__":
    sys.exit(main())
