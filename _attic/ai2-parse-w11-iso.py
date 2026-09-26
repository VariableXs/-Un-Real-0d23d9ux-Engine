# -*- coding: utf-8 -*-
"""AI-2 · Win11 官方 ISO 直链解析（微软官网 API，Fido 同源流程）。

流程：
  1) getProduct API（候选 productEditionId）→ 解析 SKU 下拉选项；
  2) GetProductDownloadLinksBySku → 解析 x64 ISO 直链（software-download.microsoft.com）；
  3) 输出直链+文件大小（不下载——下载由单独的脚本/命令执行）。

用法：python ai2-parse-w11-iso.py [zh-cn]
"""
import re
import sys
import uuid
import urllib.request

LANG = sys.argv[1] if len(sys.argv) > 1 else "zh-cn"
UA = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36",
      "Accept-Language": "zh-CN,zh;q=0.9"}
SID = uuid.uuid4()

# Win11 multi-edition ISO 的候选 productEditionId（MS 页面内部 id；逐个试到出 SKU 为止）
EDITION_CANDIDATES = [2935, 2618, 2384, 2998, 3086]

API = "https://www.microsoft.com/{lang}/api/controls/contentinclude/html"


def api_get(page_id, action, extra):
    url = (f"{API.format(lang=LANG)}?pageId={page_id}&host=www.microsoft.com"
           f"&segments=software-download,windows11&action={action}&sessionId={SID}&{extra}")
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read().decode("utf-8", "replace")


def resolve():
    sku = None
    for ed in EDITION_CANDIDATES:
        try:
            html = api_get("a8f8f489-4c7f-463a-9ca6-79cff3ada061", "getProduct",
                           f"productEditionId={ed}&requestType=ProductEditionId")
        except Exception as e:
            print(f"[ed {ed}] api error: {e}")
            continue
        skus = re.findall(r'<option[^>]*value="(\d+)"[^>]*>([^<]*)</option>', html)
        if not skus:
            print(f"[ed {ed}] no sku options (len={len(html)})")
            continue
        print(f"[ed {ed}] sku options: {skus[:6]}")
        # 取 x64 的 sku（Win11 只有 64 位）；选项文本通常含 '64-bit'
        pick = None
        for val, label in skus:
            if "64" in label:
                pick = val
                break
        sku = pick or skus[0][0]
        break
    if not sku:
        raise SystemExit("未解析到 SKU：微软 API 结构可能变化，需按实际输出调整")

    html = api_get("cfa9e580-a81e-4a4b-a846-7f21ba45e4c3", "GetProductDownloadLinksBySku",
                   f"skuId={sku}&language=zh-cn%3Bchinese%20%28simplified%29")
    links = re.findall(r'href="(https?://[^"]+\.iso[^"]*)"', html, re.I)
    sizes = re.findall(r'(\d+(?:\.\d+)?)\s*(GB|MB)', html)
    if not links:
        print(f"[sku {sku}] no iso links; page head:")
        print(html[:800])
        raise SystemExit("未解析到 ISO 直链")
    print(f"[sku {sku}] links:")
    for l in links:
        print("  " + l[:160])
    print(f"[sizes seen] {sizes[:6]}")


if __name__ == "__main__":
    resolve()
