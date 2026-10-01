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
FILES = [
    REL_BOOKLET,
    "_ai110_gen.py",
    "_ai110_append.py",
    "_ai110_push.py",
]
MSG = ("unxreal(k110): AI-110 内核工程链增补线 300项新功能增补册（B01-B15 · A110-001-A110-300 · 15批×20条×6,000行=90,000行 · 增补卷独立账不占域账）——全量内核锚定（引导链/Limine 模块通道/中断 SMP/内存 fb 戒律/存储探针红线/NVMe·AHCI·xHCI·exFAT/last_boot 闭环/ushell/调度 Wine/断电安全/日志中心/测试门禁/安全红线收口）；生成器 _ai110_gen.py 七断言 ALL PASS（300条连续唯一/判据一一对应/批守恒15×6,000/15收官印/A110-300终钉/状态列统一增补/防重grep主册零撞号）；主汇编册 AI-110 登记块已纯追加落主册本地卷（追加前 sha256 495e9b62…，追加后 192acab3…，他会话 AI-108 后续追加共存如实登记）；主册 100MB+ 超 blobs 上限沿 AI-71/81/86/90/98/108 先例欠账登记不入 pathspec；AI-97 缺陷账本仅冻结对接位不代写（R-100-004 尊重条款），他 AI 域账零触碰。")

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
