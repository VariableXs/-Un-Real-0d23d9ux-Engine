import base64, json, subprocess, urllib.request

REPO = "VariableXs/-Un-Real-0d23d9ux-Engine"
def token():
    out = subprocess.run(["git", "credential", "fill"], input="protocol=https\nhost=github.com\n\n",
                         capture_output=True, text=True).stdout
    return [l.split("=",1)[1] for l in out.splitlines() if l.startswith("password=")][0]
T = token()
def api(path, payload=None):
    req = urllib.request.Request(f"https://api.github.com/repos/{REPO}/{path}",
        data=json.dumps(payload).encode() if payload else None, method=("POST" if payload else "GET"),
        headers={"Authorization": f"Bearer {T}", "Accept": "application/vnd.github+json"})
    return json.load(urllib.request.urlopen(req))

base = api("git/ref/heads/main")["object"]["sha"]
base_commit = api(f"git/commits/{base}")
files = subprocess.run(["git","-c","core.quotepath=false","diff","--name-only","2250e9b","934f903"],capture_output=True,text=True).stdout.split()
tree_items = []
for f in files:
    data = open(f, "rb").read()
    blob = api("git/blobs", {"content": base64.b64encode(data).decode(), "encoding": "base64"})
    mode = "100755" if subprocess.run(["git","ls-files","-s",f],capture_output=True,text=True).stdout.split()[0]=="100755" else "100644"
    tree_items.append({"path": f, "mode": mode, "type": "blob", "sha": blob["sha"]})
tree = api("git/trees", {"base_tree": base_commit["tree"]["sha"], "tree": tree_items})
msg = subprocess.run(["git","log","-1","--format=%B","934f903"],capture_output=True,text=True).stdout
c = api("git/commits", {"message": msg, "tree": tree["sha"], "parents": [base]})
r = api("git/refs/heads/main", {"sha": c["sha"], "force": False})
print("PUSHED", r["object"]["sha"][:7], "files:", len(files))
