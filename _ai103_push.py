# -*- coding: utf-8 -*-
"""AI-103 推送器：临时 worktree 自 origin/main 建单提交（父=远端 HEAD）→ gh_api_push 快进。
主册(>100MB blob 上限)沿 AI-71/81/86/90/98/102/108/110 判例不入 pathspec，欠账登记于提交信息。
竞态：远端移动则重置重建，最多 6 轮。
用法：项目根目录执行  python _ai103_push.py
"""
import subprocess, shutil, os, sys, json, urllib.request

ROOT = os.path.dirname(os.path.abspath(__file__))
WT = os.path.join(ROOT, "_attic", "_ai103_push_wt")
REPO = "VariableXs/-Un-Real-0d23d9ux-Engine"
API = f"https://api.github.com/repos/{REPO}"
REL_BOOKLET = "docs/Varix/CoRun Varix STAR II · Unxreal/AI-103 · Q3 · 300项新功能增补册（B01–B15 · W90-Q3-001–W90-Q3-300）.md"
FILES = [
    REL_BOOKLET,
    "docs/unxreal/gen/_ai103_w90q3_gen.py",
    "docs/unxreal/gen/_ai103_w90q3_volume.md",
]
MSG = ("unxreal(w90-q3): AI-103 W90-Q3 .NET 与托管运行时 域首产增补册——300 项新功能（B01-B15 · W90-Q3-001-W90-Q3-300 · 15批×20条×6,000行=90,000行守恒 · 域账90,000/240,000=37.5%·状态列统一增补·深化轮未启动如实登记）——B01-B05 Mono上栈（宿主装载/JIT W^X 内存权限/跨语言异常 SEH 穿透深水 40 例对拍/GAC 融合语义/注册表接线）+B06-B10 Framework API 面（BCL 对拍三卷 Mono 公开单测跑绿框架/System.Configuration 深水/2.0-3.5-4.x 三 profile 收口）+B11-B14 WinForms（消息循环映射/60 控件×3 主题 SSIM>=0.98 视觉回归两卷/复杂控件深水）+B15 WPF 开卷（翻译层渲染/完整性分级账/e-pod 兜底协议）；判据全部锚定 varix 内核工程链（进程/线程/GC/代码页落内核 vm 与调度原语、文件注册表落 fs23/VFS/注册表语义层、GUI 落 vxwm 合成器与 DWrite 语义、断电安全继承 fs23 日志口径），T1/T2/T3 三档判据全给执行入口，T2/T3 实机对照登记随闸门补测（双轨产线）；法律红线零逆向 Windows CLR，全部路线基于开源 Mono 与官方 .NET（W90 源册 7.6 五条红线适用）；生成器 _ai103_w90q3_gen.py 四项守恒断言+防重预检 ALL PASS（300 ID 连续零跳号/判据编号全卷唯一/每批 6,000 守恒/六范围防重 grep 零撞号）；主汇编册 119MB 超 blobs 100MB 硬上限沿 AI-71/81/86/90/98/102/108/110 先例不入 pathspec（AI-103 卷 15 批头+300 表行+收口段已入本地 HEAD 主册·回读机械核验在位·远端欠账如实登记）；他会话在途产物零触碰")


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
    r = subprocess.run([sys.executable, os.path.join(ROOT, "_attic", "tools", "gh_api_push.py"), local_sha],
                       cwd=ROOT, capture_output=True)
    out = (r.stdout or b"").decode("utf-8", "replace") + (r.stderr or b"").decode("utf-8", "replace")
    return r.returncode, out


def main():
    for attempt in range(1, 7):
        rh = remote_head()
        print(f"[{attempt}/6] 远端 HEAD = {rh[:12]}")
        local_sha = build(rh)
        print(f"      worktree 单提交 = {local_sha[:12]}（父=远端 HEAD，真快进）")
        rc, out = gh_push(local_sha)
        tail = out[-400:].replace("\n", " | ")
        if rc == 0:
            print("      推送 OK:", tail)
            sh(["git", "worktree", "remove", "--force", WT])
            print("DONE remote main =", remote_head()[:12])
            return 0
        print("      推送失败（远端竞态或拒绝），重试:", tail)
    raise SystemExit("6 轮竞态重试耗尽，推送未完成（本地提交 679219e6 与产物均在，可复跑）")


if __name__ == "__main__":
    sys.exit(main())
