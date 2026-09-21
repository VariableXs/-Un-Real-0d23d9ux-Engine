# -*- coding: utf-8 -*-
"""调试：抓 Win11 下载页与其 JS，找 contentinclude API 的真实 pageId/segments。"""
import re
import sys
import urllib.request

UA = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36"}


def get(url):
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read().decode("utf-8", "replace")


for page in ["https://www.microsoft.com/zh-cn/software-download/windows11",
             "https://www.microsoft.com/en-us/software-download/windows11"]:
    try:
        html = get(page)
    except Exception as e:
        print(f"[page] {page} -> {e}")
        continue
    print(f"[page] {page} len={len(html)}")
    for pat in [r'pageId=([0-9a-f-]+)', r'productEditionId[=:]\s*"?(\d+)', r'"skuId"\s*:\s*"?(\d+)',
                r'contentinclude[^"\']*', r'api/controls/[^"\']{0,120}']:
        hits = list(dict.fromkeys(re.findall(pat, html, re.I)))[:6]
        if hits:
            print(f"  {pat} -> {hits}")
    # 页面内嵌的脚本链接
    scripts = re.findall(r'src="([^"]+\.js[^"]*)"', html)[:8]
    print(f"  scripts: {scripts}")
    sys.stdout.flush()
