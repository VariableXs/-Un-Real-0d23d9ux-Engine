# -*- coding: utf-8 -*-
"""调试2：MS contentinclude API 参数组合探测（找 Win11 multi-edition ISO 的 SKU）。"""
import re
import uuid
import urllib.request
import urllib.error

UA = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36"}
SID = uuid.uuid4()

def try_url(url):
    req = urllib.request.Request(url, headers=UA)
    try:
        with urllib.request.urlopen(req, timeout=45) as r:
            body = r.read().decode("utf-8", "replace")
            return r.status, body
    except urllib.error.HTTPError as e:
        return e.code, ""
    except Exception as e:
        return -1, str(e)

bases = ["https://www.microsoft.com/en-us/api/controls/contentinclude/html",
         "https://www.microsoft.com/zh-cn/api/controls/contentinclude/html"]
editions = [2935, 2618, 2384]
pageid_product = "a8f8f489-4c7f-463a-9ca6-79cff3ada061"

for b in bases:
    for ed in editions:
        url = (f"{b}?pageId={pageid_product}&host=www.microsoft.com"
               f"&segments=software-download,windows11&productEditionId={ed}"
               f"&requestType=ProductEditionId&sessionId={SID}")
        st, body = try_url(url)
        if st == 200:
            opts = re.findall(r'value="(\d+)"', body)
            print(f"[200] {b.split('/')[2][:5]} ed={ed} len={len(body)} opts={opts[:8]}")
            if opts:
                open(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\ai2-ms-product-page.html", "w", encoding="utf-8").write(body)
        else:
            print(f"[{st}] {b.split('/')[2][:5]} ed={ed}")
