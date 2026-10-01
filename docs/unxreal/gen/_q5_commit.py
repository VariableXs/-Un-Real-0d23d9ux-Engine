# -*- coding: utf-8 -*-
"""AI-105 会话提交器：pathspec 限定 7 件，提交信息走文件通道避开 shell 引号。"""
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
os.chdir(ROOT)

FILES = [
    "docs/Varix/CoRun Varix STAR II · Unxreal/AI-105 · Q5 · 300项新功能增补册（B01–B15 · W90-Q5-001–300）.md",
    "docs/unxreal/gen/_q5_firstprod.py",
    "docs/unxreal/gen/_q5_data1.py",
    "docs/unxreal/gen/_q5_data2.py",
    "docs/unxreal/gen/_q5_data3.py",
    "docs/unxreal/gen/_q5_data4.py",
    "docs/unxreal/gen/_q5_data5.py",
]

MSG = """unxreal(w90-q5): AI-105 W90-Q5 UWP/MSIX 现代应用模型 首产段 300 项立账——W90-Q5-001–300（15 批 × 20 条 × 6,000 行 = 90,000 行 · 域账 90,000/240,000 = 37.5% · 状态「骨架」不冒充深化 · 承 AI-102/107/109 W90 线判例）；批主题对位任务书 §105.3：B01–B05 包格式与签名（OPC 容器与 ZIP 解析/AppxManifest 清单与包身份/AppxBlockMap 块映射与完整性/签名与信任链/资源·捆绑·可选·appinstaller 变体体系）+ B06–B10 包管理服务与 PowerShell 语义（包仓库与部署状态机/Get-Add-Remove Appx cmdlet 两批/依赖解析与框架包/部署可观测性与体验日志）+ B11–B14 AppContainer 与沙箱接线（能力 SID 与令牌/资源隔离·文件注册表命名对象/能力语义与同意门控/激活与进程模型）+ B15 挂起恢复与后台任务基座+域收官联轧；判据全部围绕 varix 内核链（pkgstore/fsview/reggate/signchain/sec·secgate/proc·proc::job/kvsrv/syslogd·observ/task·power/ktest M-Q5-###/kcheck 0 违例·门禁铁值 ktest 3146）；T1/T2/T3 三档判据全给断言数字与执行入口，实弹随 CW-07～12 波次窗闸门补测（开发期零 QEMU 零实机写·存储探针门禁红线全程适用）；虚拟化设施反哺 AI-110 复用位与 AI-108/AI-114/AI-121/AI-133/AI-135 联签位在册；生成器 docs/unxreal/gen/_q5_firstprod.py 五断言 ALL PASS exit=0（15 批在位/300 ID 连续零跳号/300 深化名唯一/每批 6,000 行守恒全卷 90,000/判据 J1 锚与 T 档一一对应+断言数字全覆盖/内核锚定 300/300/主册追加前 W90-Q5- 前缀零命中防重/主册·分工图·台账三处纯追加前缀 SHA-256 1a4115fa…a833033e…68f3d03c… 一致零删除零改写）；R-PROC-002 现场复发如实登记：主汇编册落件时实测已被并行会话整册重写（883,049 行/253.6MB→377,128 行/104.5MB·GV99/W90-Q9 段零命中），本会话基于重写后新态保位落件（AI-26/102 判例），异常观察补记入台账再报 AI-92/98 联席；他会话在途产物零触碰（pathspec 显式限定本会话 7 件）"""

def sh(args, **kw):
    r = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", errors="replace", **kw)
    print("$", " ".join(args[:3]), "->", r.returncode)
    if r.stdout.strip():
        print(r.stdout.strip()[:1500])
    if r.returncode != 0 and r.stderr.strip():
        print("STDERR:", r.stderr.strip()[:1500])
    return r

msg_path = os.path.join(ROOT, "_attic", "reports", "_q5_commit_msg.txt")
os.makedirs(os.path.dirname(msg_path), exist_ok=True)
with open(msg_path, "w", encoding="utf-8", newline="\n") as f:
    f.write(MSG)

r = sh(["git", "add", "-A", "--"] + FILES)
assert r.returncode == 0, "git add 失败"

# 只允许这 7 件进入暂存差异：核对 staged 与 pathspec 一致
staged = subprocess.run(["git", "-c", "core.quotepath=false", "diff", "--cached", "--name-only", "--"] + FILES,
                        capture_output=True, text=True, encoding="utf-8").stdout.strip().splitlines()
print("staged for pathspec:", len(staged))
for p in staged:
    print("  ", p)

r = sh(["git", "commit", "-F", msg_path, "--"] + FILES)
assert r.returncode == 0, "git commit 失败"

r = sh(["git", "log", "--oneline", "-1"])
print("COMMIT OK")
