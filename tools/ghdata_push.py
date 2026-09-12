"""github.com:443 不可达时的兜底推送：走 api.github.com 的 Git Data API。

用法（仓库根执行）：
    GH_TOKEN=<pat> python tools/ghdata_push.py "<提交信息>" <相对路径...>

令牌取法：
    printf 'protocol=https\\nhost=github.com\\nusername=VariableXs\\n\\n' | \
        git -c credential.helper= -c credential.helper=wincred credential fill

只覆盖传入的路径，其余远端内容原样保留（tree 用 base_tree 增量）。
注意：这样推上去的提交 SHA 与本地不同（内容等价），下次需 fetch + merge 对齐。
"""
import io, json, os, sys, urllib.request

TOKEN = os.environ["GH_TOKEN"]
REPO = "VariableXs/-Un-Real-0d23d9ux-Engine"
API = "https://api.github.com"

MSG = sys.argv[1]
FILES = sys.argv[2:]
assert MSG and FILES, "需要提交信息与至少一个文件路径"


def req(method, path, payload=None):
    data = None
    if payload is not None:
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    r = urllib.request.Request(
        API + path,
        data=data,
        method=method,
        headers={
            "Authorization": "Bearer " + TOKEN,
            "Accept": "application/vnd.github+json",
            "Content-Type": "application/json",
            "User-Agent": "ghdata-push",
        },
    )
    with urllib.request.urlopen(r, timeout=60) as resp:
        return json.loads(resp.read().decode("utf-8"))


ref = req("GET", "/repos/%s/git/ref/heads/main" % REPO)
remote_sha = ref["object"]["sha"]
print("remote main:", remote_sha)

tree_entries = []
for f in FILES:
    content = io.open(f, encoding="utf-8").read()
    tree_entries.append(
        {"path": f.replace("\\", "/"), "mode": "100644", "type": "blob", "content": content}
    )

tree = req("POST", "/repos/%s/git/trees" % REPO, {"base_tree": remote_sha, "tree": tree_entries})
print("tree:", tree["sha"])

commit = req(
    "POST",
    "/repos/%s/git/commits" % REPO,
    {"message": MSG, "tree": tree["sha"], "parents": [remote_sha]},
)
print("commit:", commit["sha"])

upd = req("PATCH", "/repos/%s/git/refs/heads/main" % REPO, {"sha": commit["sha"]})
print("ref now:", upd["object"]["sha"])
