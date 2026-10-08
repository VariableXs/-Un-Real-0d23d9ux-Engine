# -*- coding: utf-8 -*-
"""AI-80 · P5 远端主册 v2 修补挂件（分步版）：远端 blob 下载 → 段整替 → blob 上传 → 树/提交/ref。"""
import base64, io, json, subprocess, sys, time, urllib.request, urllib.error

sys.path.insert(0, 'docs/unxreal/gen')
import _p5_firstprod as g

T = subprocess.run(['git', 'credential', 'fill'], input='protocol=https\nhost=github.com\n\n',
                   capture_output=True, text=True).stdout
tok = [l.split('=', 1)[1] for l in T.splitlines() if l.startswith('password=')][0]
BASE = 'https://api.github.com/repos/VariableXs/-Un-Real-0d23d9ux-Engine'
H = {'Authorization': 'Bearer ' + tok, 'User-Agent': 'ai80-p5', 'Accept': 'application/vnd.github+json'}


def api(method, u, payload=None, raw_body=None, headers=None):
    data = json.dumps(payload).encode() if payload is not None else (raw_body if raw_body is not None else None)
    hh = dict(H)
    if headers:
        hh.update(headers)
    r = urllib.request.Request(BASE + u, data=data, headers=hh, method=method)
    resp = urllib.request.urlopen(r, timeout=300)
    body = resp.read()
    try:
        return json.loads(body)
    except Exception:
        return body


print('step1: download remote master blob', flush=True)
blob = api('GET', '/git/blobs/a988ac9ba2cfc95a358c82b1d38d0f6777732ec4')
remote_text = base64.b64decode(blob['content']).decode('utf-8')
print('remote chars', len(remote_text), flush=True)

print('step2: locate v1 section', flush=True)
H0 = '# 增补卷 · AI-80 · UNX-P5 里程碑发布与年度镜像 · 首产段 B01–B15（F63201–F63500 · 300 项）'
END = '**（AI-80 首产段终）**'
h0 = remote_text.find(H0)
assert h0 > 0, 'v1 heading not found'
h1 = remote_text.find(END, h0)
assert h1 > h0, 'v1 end marker not found'
h1 += len(END)
old_seg = remote_text[h0:h1]
assert old_seg.count('UNX-F63201 |') == 1
print('v1 seg chars', len(old_seg), flush=True)

print('step3: build v2 section', flush=True)
rows = g.build_rows()
new_seg = g.build_master_section(rows).strip()

print('step4: patch registration line', flush=True)
assert g.REG_LINE in remote_text, 'v1 registration line not found'
new_reg = g.REG_LINE.replace(
    '批位差异（任务书 B09–B20 M 型标注 vs 连续零跳号推算 B07–B18）按连续零跳号公理恒等归位并诚实登记（AI-62 判例）',
    '批位差异（任务书 B09–B20 M 型标注 vs 连续零跳号推算 B07–B18；F63401 任务书批标 B12 vs 推算 B11——终审证据树整批归位 B11、四站批顺移 B12）按连续零跳号公理恒等归位并诚实登记（AI-62/AI-74 判例）',
).replace(
    '生成器 docs/unxreal/gen/_p5_firstprod.py 五断言 ALL PASS exit=0。',
    '生成器 docs/unxreal/gen/_p5_firstprod.py（数据）+ _p5_repair.py（主册段整替修复 v2）五断言 ALL PASS exit=0。',
)
patched = remote_text.replace(g.REG_LINE, new_reg, 1).replace(old_seg, new_seg, 1)
assert patched.count(H0) == 1
assert 'UNX-F63401 | 终审证据树组装器（R1–R10） | 480' in patched
with io.open('docs/unxreal/gen/_p5_master_v2_remote.txt', 'w', encoding='utf-8', newline='\n') as f:
    f.write(patched)
print('patched chars', len(patched), flush=True)

MSG = ('unxreal(p5): AI-80 UNX-P5 首产段 v2 修正挂件——F63401（终审证据树组装器·480 行·G1 类判据）锚位配对修正'
       '（v1 误配署名页站深化行），终审证据树批按连续零跳号恒等归位 B11、四站批顺移 B12（AI-62/AI-74 判例诚实登记），'
       '卷首登记注同步补归位声明；本域 300 条/90,000 行守恒不变，他域内容零触碰。内容与本地修复（_p5_repair.py v2）一致。')

print('step5: upload blob', flush=True)
with open('docs/unxreal/gen/_p5_master_v2_remote.txt', 'rb') as f:
    payload = f.read()
blob2 = None
for attempt in range(6):
    try:
        blob2 = api('POST', '/git/blobs', raw_body=json.dumps({
            'content': payload.decode('utf-8'), 'encoding': 'utf-8'}).encode(),
            headers={'Content-Type': 'application/json'})
        break
    except urllib.error.HTTPError as e:
        print('blob utf-8 try', attempt, e.code, flush=True)
        if e.code in (401, 422, 502):
            try:
                blob2 = api('POST', '/git/blobs', raw_body=json.dumps({
                    'content': base64.b64encode(payload).decode('ascii'), 'encoding': 'base64'}).encode(),
                    headers={'Content-Type': 'application/json'})
                break
            except urllib.error.HTTPError as e2:
                print('blob b64 try', attempt, e2.code, flush=True)
        time.sleep(10)
if blob2 is None:
    sys.exit('blob upload failed')
print('new blob', blob2['sha'], flush=True)

print('step6: commit + ref (race retry)', flush=True)
for attempt in range(8):
    head = api('GET', '/git/refs/heads/main')['object']['sha']
    tree = api('POST', '/git/trees', {'base_tree': head, 'tree': [
        {'path': 'docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md',
         'mode': '100644', 'type': 'blob', 'sha': blob2['sha']}]})['sha']
    c = api('POST', '/git/commits', {'message': MSG, 'tree': tree, 'parents': [head]})
    try:
        api('PATCH', '/git/refs/heads/main', {'sha': c['sha'], 'force': False})
        print('PUSHED master v2', c['sha'][:10], 'on', head[:10], flush=True)
        break
    except urllib.error.HTTPError as e:
        print('ref race, retry', e.code, flush=True)
        time.sleep(8)
else:
    print('FAILED after retries')
