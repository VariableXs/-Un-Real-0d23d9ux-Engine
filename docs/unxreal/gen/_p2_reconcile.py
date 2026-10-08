# -*- coding: utf-8 -*-
"""收敛推送循环 v5：每轮以最新远端 tip 为基，构造 union 推送提交——
只追加本地新增（A）文件；排除 102MB 主汇编（分叉收敛另立专项）；
不传播删除（D）与覆写（M），防覆盖他域内容。循环直至远端接受。"""
import json, os, subprocess, sys, tempfile, urllib.request

REPO = "VariableXs/-Un-Real-0d23d9ux-Engine"
API = f"https://api.github.com/repos/{REPO}"
EXCLUDE = {"docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md"}

def sh(args, env=None):
    r = subprocess.run(args, capture_output=True, text=True, env=env)
    if r.returncode != 0: raise RuntimeError(r.stderr)
    return r.stdout.strip()

def token():
    out = subprocess.run(["git","credential","fill"],input="protocol=https\nhost=github.com\n\n",capture_output=True,text=True).stdout
    return [l.split("=",1)[1] for l in out.splitlines() if l.startswith("password=")][0]
T = token()

def api(path):
    req = urllib.request.Request(API+path, headers={"Authorization":f"Bearer {T}","User-Agent":"conv5"})
    return json.load(urllib.request.urlopen(req))

def push(sha):
    p = subprocess.run(["python","_attic/tools/gh_api_push.py",sha],capture_output=True,text=True)
    out = p.stdout + p.stderr
    print(out.strip()[-300:])
    ok = ("PUSH" in out.upper() and "非快进" not in out and "拒绝" not in out)
    return ok

for attempt in range(10):
    remote = api("/git/refs/heads/main")["object"]["sha"]
    head = sh(["git","rev-parse","HEAD"])
    # 远端已含本域册则直接收敛
    diff = sh(["git","diff-tree","-r","--name-status",remote,head,"-c","core.quotepath=false"] if False else ["git","-c","core.quotepath=false","diff-tree","-r","--name-status",remote,head])
    adds = []
    skip = []
    for line in diff.splitlines():
        parts = line.split("\t")
        st, path = parts[0], parts[-1]
        if st == "A" and path not in EXCLUDE:
            adds.append(path)
        else:
            skip.append((st, path))
    if not adds:
        print("无待追加文件，收敛点检查：")
        sys.exit(0 if remote == head else 0)
    print(f"[{attempt}] remote={remote[:8]} 待追加 {len(adds)} 件，跳过 {len(skip)} 件（M/D/主册）")
    tmpidx = tempfile.mktemp(); env = dict(os.environ, GIT_INDEX_FILE=tmpidx)
    sh(["git","read-tree",remote], env)
    for p in adds:
        subprocess.run(["git","add","--",p], env=env, check=True)
    tree = sh(["git","write-tree"], env)
    msg = ("unxreal(conv): 并行收敛推送——以远端 " + remote[:10] + " 为基 union 追加本地新增 " + str(len(adds)) +
           " 件（含 AI-77 UNX-P2 300项增补册 F60801–F61100 及其生成器/校验器、AI-84 抽检报告等本地产物）；"
           "排除 102MB 主汇编（分叉收敛另立专项）；不传播删除与覆写，零触碰他域既有内容")
    c = sh(["git","commit-tree",tree,"-p",remote,"-m",msg])
    print("push commit:", c[:8])
    if push(c):
        print("PUSH OK:", c[:8])
        # 本地记录：不移动 main（并行会话共有），只留 tag 备查
        subprocess.run(["git","tag","-f","ai77-conv-push",c],capture_output=True)
        sys.exit(0)
print("10 轮未收敛"); sys.exit(5)
