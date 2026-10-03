# -*- coding: utf-8 -*-
"""
VTaskBoard — 独立任务板软件（单文件 .exe）
· 任务源 = 同目录 taskboard.md（MD 任务块，人可编辑，AI 可读写）
· 软件内渲染任务卡 + 实时进度；AI 通过 HTTP API 领单/完成/阻塞/退回
· 所有状态变化实时写回 taskboard.md（原子写）
"""
import hashlib
import json, os, re, subprocess, sys, threading, time, urllib.parse, webbrowser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# 运行期资源根目录。PyInstaller 3.x+ 用 sys._MEIPASS（旧的 _MEIPASS2 早已移除，
# 沿用它会让 onefile 包内所有静态资源都定位失败）。开发态=脚本目录。
APP_DIR = getattr(sys, "_MEIPASS", None) or os.path.dirname(os.path.abspath(__file__))
if getattr(sys, "frozen", False) and not os.path.isdir(os.path.join(APP_DIR, "wallpapers")):
    # onedir 布局：静态资源在 exe 同级或 _internal 下
    for _alt in (os.path.dirname(sys.executable),
                 os.path.join(os.path.dirname(sys.executable), "_internal")):
        if os.path.isdir(os.path.join(_alt, "wallpapers")):
            APP_DIR = _alt
            break
MD_PATH = None
for _cand in (os.path.join(APP_DIR, "taskboard.md"),
              os.path.join(os.path.dirname(APP_DIR), "taskboard.md")):
    if os.path.exists(_cand):
        MD_PATH = _cand
        break
if MD_PATH is None:
    MD_PATH = os.path.join(APP_DIR, "taskboard.md")
PORT = 8767
LOCK = threading.Lock()

# ---------- Wallpaper Engine 壁纸自动识别 ----------
WE_APPID = "431960"
_WE_CACHE = {"t": 0.0, "items": []}

def _steam_libraries():
    """从 Steam libraryfolders.vdf 发现所有 Steam 库根目录"""
    cands = [
        os.environ.get("PROGRAMFILES(X86)", r"C:\Program Files (x86)") + r"\Steam\steamapps\libraryfolders.vdf",
        r"C:\Steam\steamapps\libraryfolders.vdf",
        r"D:\Steam\steamapps\libraryfolders.vdf",
        r"E:\Steam\steamapps\libraryfolders.vdf",
    ]
    for vdf in cands:
        if os.path.isfile(vdf):
            libs, cur = [], None
            try:
                with open(vdf, encoding="utf-8", errors="replace") as f:
                    for line in f:
                        m = re.match(r'\s*"path"\s+"(.+?)"', line)
                        if m:
                            p = m.group(1).replace("\\\\", "\\")
                            if p not in libs:
                                libs.append(p)
            except OSError:
                pass
            if libs:
                return libs
    return [os.path.dirname(os.path.dirname(cands[0]))] if cands else []

def we_root():
    """Wallpaper Engine 创意工坊内容目录（431960）"""
    for lib in _steam_libraries():
        d = os.path.join(lib, "steamapps", "workshop", "content", WE_APPID)
        if os.path.isdir(d):
            return d
    return None

# 母版缓存目录。**必须是持久可写目录**，不能用 __file__ 所在处：
# PyInstaller onefile 下那是进程退出即销毁的 _MEIPASS 临时目录，
# 写进去等于每次启动都要重算一遍 AI 超分（单张 10~45 秒，十几张就是十几分钟）。
# 统一放 exe 同级的 _m4k / _wehd，开发态与打包态行为一致。
_CACHE_BASE = (os.path.dirname(sys.executable)
               if getattr(sys, "frozen", False) else APP_DIR)
_HD_DIR = os.path.join(_CACHE_BASE, "_wehd")
_M4K_DIR = os.path.join(_CACHE_BASE, "_m4k")
# scene.pkg 原生素材解包缓存（PNG/JPG/MP4）。与 _m4k 同理，必须持久可写。
_TEX_DIR = os.path.join(_CACHE_BASE, "_wetexture")
for _d in (_HD_DIR, _M4K_DIR, _TEX_DIR):
    try:
        os.makedirs(_d, exist_ok=True)
    except OSError:
        pass

# 壁纸 4K 母版引擎（AI 超分 + 分阶段 Lanczos + 自适应锐化）。
# 缺 Pillow 或引擎异常时降级为纯插值产线，绝不让壁纸列表整体失败。
try:
    import wall4k as _wall4k
except ImportError:                # PyInstaller 单文件模式下从 _MEIPASS 取
    try:
        import wall4k as _wall4k
    except Exception:
        _wall4k = None

# scene.pkg 原生素材提取器（PKGV + TEX 容器逆向）。
# 有了它，scene 壁纸的源从 600x600 缩略图换成 2560x1440 / 3840x2160 / 4096x2160
# 原生纹理，甚至内嵌的完整 MP4 —— 这是"很糊"的真正根治点。
try:
    import wetex as _wetex
except Exception:
    _wetex = None

# 母版超采样系数：2x = 3840x2160 母版映射到 1920x1080 屏，等效 2x2 SSAA。
# 实测（JIT实验）再往上提到 5760 对最终上屏像素零额外收益，只吃内存/带宽。
MASTER_SS = 2

# AI 超分启用阈值。取自引擎（单一事实来源）；引擎缺失时用同名默认值兜底，
# 保证「UI 标注的画质来源」这件事在降级状态下依然成立。
_SR_MIN_UPSCALE = getattr(_wall4k, "SR_MIN_UPSCALE", 1.8) if _wall4k else 1.8
# 母版生成并发度：AI 超分是 Vulkan 调用 + 磁盘 IO，串行太慢；但 Vulkan 上下文
# 在 wall4k 内已加锁串行，这里只限制同时「准备」的数量，避免一次开十几个解码器。
_M4K_SEM = threading.Semaphore(2)
_M4K_MEM = {}
_M4K_MEM_LOCK = threading.Lock()
# 正在后台生成的母版：pid -> True。防止并发请求重复排同一个任务
# （AI 超分是秒级开销，重复排队会把 CPU/GPU 占满并拖慢一切）。
_M4K_PEND = set()
_M4K_PEND_LOCK = threading.Lock()


def _m4k_pending(pid):
    with _M4K_PEND_LOCK:
        return pid in _M4K_PEND


def _m4k_mark(pid, on):
    with _M4K_PEND_LOCK:
        if on:
            _M4K_PEND.add(pid)
        else:
            _M4K_PEND.discard(pid)


def _m4k_spawn(fn, pid):
    """把母版生成丢到后台线程，接口立即返回（绝不阻塞列表 API）。

    为什么必须异步：AI 超分单张要 10~25s、GIF 要 4~5 分钟。若在 /api/we/list
    里同步生成，整个 HTTP 线程池会被占满 —— 实测表现为前端一个 fetch 都发不出去、
    壁纸栏空白。正确做法是：列表立刻返回「先用原图/占位」，后台生成完成后
    下一轮轮询自然带上 4K 母版 URL。
    """
    if _m4k_pending(pid):
        return
    _m4k_mark(pid, True)

    def _run():
        try:
            fn()
        except (OSError, ValueError, Exception) as e:      # 后台线程绝不能把异常吞掉
            sys.stderr.write("[m4k] background %s failed: %r\n" % (pid, e))
        finally:
            _m4k_mark(pid, False)
            # 生成完成即失效列表缓存，让前端下一轮拿到新的 4K URL
            _WE_CACHE["t"] = 0.0
            _M4K_MEM.clear()

    threading.Thread(target=_run, daemon=True, name="m4k-%s" % pid).start()

# 统一 MIME 表。此前四条静态路由各写一份局部字典，遗漏项导致真实故障：
# 字体 MIME 缺失会让 @font-face 被浏览器拒载（定制皮肤静默回退系统字体）；
# avif/bmp 缺失会让对应壁纸 200 但类型错误。集中一处，新增类型不再漏。
_MIME = {
    ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".png": "image/png",
    ".webp": "image/webp", ".avif": "image/avif", ".bmp": "image/bmp",
    ".gif": "image/gif", ".svg": "image/svg+xml",
    ".mp4": "video/mp4", ".webm": "video/webm", ".mkv": "video/x-matroska",
    ".mp3": "audio/mpeg", ".ogg": "audio/ogg", ".wav": "audio/wav",
    ".ttf": "font/ttf", ".otf": "font/otf",
    ".woff": "font/woff", ".woff2": "font/woff2",
    ".html": "text/html; charset=utf-8", ".htm": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8", ".mjs": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8", ".json": "application/json; charset=utf-8",
    ".txt": "text/plain; charset=utf-8",
}


def _mime(path, default="application/octet-stream"):
    """按扩展名取 Content-Type。未知类型返回 default（不猜、不裸奔）。"""
    return _MIME.get(os.path.splitext(path)[1].lower(), default)


def _eff_upscale(src, mw, mh):
    """源图 cover 到 (mw, mh) 后的有效放大倍数（相对**母版**）。

    决定保真度的是 cover 之后的**有效短边**，不是原图短边：600x600 方形
    preview 铺到 16:9 母版时纵向被裁掉 44%，短边实际是 600x337 那条边。
    算法与 wall4k.build_master 内的 eff 完全一致 —— 两处必须同口径，
    否则 UI 会把「已超分」标成未超分，或反之。
    仅用于**判定画质来源**（是否越过超分阈值），不可直接展示给用户。
    """
    if not src or not src[0] or not src[1] or not mw or not mh:
        return 0.0
    sw, sh = float(src[0]), float(src[1])
    src_ar, dst_ar = sw / sh, float(mw) / float(mh)
    eff = min(sw, sh * dst_ar / src_ar) if src_ar > dst_ar \
        else min(sh, sw * src_ar / dst_ar)
    return round(max(mw / max(1.0, eff), mh / max(1.0, eff)), 2)


def _screen_upscale(src):
    """素材铺到实际屏幕所需的放大倍数 —— **这是给用户看的那个数字**。

    与 _eff_upscale 的区别是母版系数：母版是屏的 MASTER_SS 倍超采样，
    若直接展示母版口径，所有数字都会虚高 MASTER_SS 倍（600px 方形素材会
    报 11.38x，实际铺到 1920x1080 屏只需 5.69x）。用户关心的是「我的素材
    离原生差几倍」，所以展示必须用上屏口径。
    """
    w, h = _display_px()
    return _eff_upscale(src, w, h)


def _display_px():
    """壁纸实际显示区域的物理像素尺寸（宽, 高）。

    必须用物理像素而非 CSS 逻辑像素：壁纸元素由 Edge/Chromium 以 devicePixelRatio
    栅格化，125% 缩放下 CSS 1536x864 对应物理 1920x1080。若按逻辑像素出图，
    浏览器会再放大 1.25 倍插值 —— 那是"越清晰越糊"的根因。
    PER_MONITOR_AWARE 进程下 GetSystemMetrics 返回物理像素；DPI 不可用时回退 1920x1080。
    """
    try:
        if _u32:
            w = _u32.GetSystemMetrics(0)      # SM_CXSCREEN：主屏物理宽（DPI 感知）
            h = _u32.GetSystemMetrics(1)      # SM_CYSCREEN：主屏物理高
            if w > 0 and h > 0:
                return w, h
    except (AttributeError, OSError):
        pass
    return 1920, 1080

def _cover_to(im, tw, th):
    """等比 cover 预裁切到目标宽高比（与 CSS object-fit:cover 语义一致，居中裁切）。

    为什么必须做：官方 preview 是 600x600 / 1024x1024 方形图，而显示区是 16:9。
    若先拉成 tw x tw 方形，浏览器再 cover 裁到 tw x th，等效于「1.875 倍放大 + 一次
    额外的重采样」；实测像素浪费 1.78 倍。先按比例裁掉多余轴、再一次性缩放到
    显示尺寸，浏览器端就只剩一次等比映射，等于把插值损失降到最低。
    """
    s = max(tw / im.width, th / im.height)          # cover 缩放系数
    nw, nh = max(tw, round(im.width * s)), max(th, round(im.height * s))
    if (nw, nh) != im.size:
        im = im.resize((nw, nh), _PImage.LANCZOS)
    if nw > tw or nh > th:                            # 居中裁掉超出轴
        l, t = (nw - tw) // 2, (nh - th) // 2
        im = im.crop((l, t, l + tw, t + th))
    return im

def _mp4_dims(path):
    """读 MP4/MOV 显示尺寸（递归下钻 moov/trak 找 tkhd 的 16.16 定点宽高）。
    取所有轨道中宽高最大者：音频轨的 tkhd 宽高为 0，必须跳过，否则取到 0 或错值。
    不依赖 ffprobe/外部进程；解析失败返回 None。"""
    try:
        with open(path, "rb") as f:
            d = f.read()
    except OSError:
        return None
    stack = [(0, len(d))]
    best = None
    while stack:
        start, end = stack.pop()
        i = start
        while i + 8 <= end:
            # box 声明长度可能超出实际文件（截断/分片下载的 mdat 很常见）：
            # 此时停止下钻但保留已解析出的尺寸，绝不因尾部越界丢掉前面的真值。
            if i + 4 > end:
                return best
            sz = int.from_bytes(d[i:i + 4], "big")
            typ = d[i + 4:i + 8]
            hs = 8
            if sz == 1:
                if i + 16 > end:
                    return best
                sz = int.from_bytes(d[i + 8:i + 16], "big")
                hs = 16
            elif sz == 0:
                sz = end - i
            if sz < hs:
                return best
            if typ == b"tkhd":
                ver = d[i + hs]
                off = i + hs + (4 if ver == 0 else 8) + 4 + 4 + 4 + 4 + 4 + 8 + 2 + 2 + 2 + 2 + 36
                if off + 8 <= min(i + sz, end):
                    w = int.from_bytes(d[off:off + 4], "big") >> 16
                    h = int.from_bytes(d[off + 4:off + 8], "big") >> 16
                    if w > 0 and h > 0 and (best is None or w * h > best[0] * best[1]):
                        best = (w, h)
                if i + sz > end:
                    return best            # 本 box 被截断，后续无意义
            elif typ in (b"moov", b"trak", b"mdia", b"minf", b"stbl", b"edts", b"udta"):
                if i + hs > end:
                    return best
                stack.append((i + hs, min(i + sz, end)))
                if i + sz > end:
                    return best
            i += sz
    return best

def _we_url_safe(fn):
    """WE 素材文件名含中文全角冒号「：」、尾随空格等 URL 不安全字符，
    直接拼接会被客户端/代理规范化导致 404。映射为 pid 级安全名（幂等、可逆）。

    规则：<we原名> -> /we/<pid>/_f<base64url(utf-8 原名)>
    服务端按同一规则解码回真实路径，故无需改动磁盘文件。"""
    import base64
    b = base64.urlsafe_b64encode(fn.encode("utf-8")).decode("ascii").rstrip("=")
    return "_f" + b

def _img_size(path):
    """读图片首帧尺寸（GIF 不触发全帧解码；jpg 直接读头）。失败返回 None。
    用容错打开：部分本地 JPEG 尾部缺 1~4 字节，严格模式会直接抛异常，
    导致该壁纸的分辨率也无法读取（标注随之丢失）。"""
    if not _PIL:
        return None
    try:
        with (_wall4k.open_tolerant(path) if _wall4k else _PImage.open(path)) as im:
            return im.size
    except (OSError, ValueError):
        return None

# ---------------------------------------------------------------- 原生源
# pid -> {"path": 落地文件绝对路径, "kind": "img"|"video", "w","h","src":"tex"|"file"}
# 提取一次即落盘复用，避免每次列表刷新都重解 8~136MB 的 .tex。
_NATIVE_SRC = {}
_NATIVE_LOCK = threading.Lock()


def _native_source(pid, pdir):
    """取该项目可用的最佳原生素材（scene.pkg 内解包），失败返回 None。

    优先级：scene.pkg 里的原生纹理 / 内嵌 MP4 > 项目目录里的真视频 > preview 缩略图。
    判据是**像素总量**：只有当原生素材确实比 preview 缩略图更大时才采用，
    否则宁可留在 preview（避免"降级"反而更清晰）。
    """
    if _wetex is None:
        return None
    with _NATIVE_LOCK:
        hit = _NATIVE_SRC.get(pid)
    if hit is not None:
        return hit

    prev = None
    for c in ("preview.jpg", "preview.png", "preview.gif"):
        p = os.path.join(pdir, c)
        if os.path.isfile(p):
            prev = _img_size(p)
            break

    rec = None
    try:
        files = _wetex.extract_project_textures(pdir, _TEX_DIR)
    except Exception as e:                       # 解析异常不得拖垮壁纸列表
        sys.stderr.write("[native] %s 提取异常: %r\n" % (pid, e))
        files = []
    if files:
        fp = files[0]
        kind = "video" if fp.lower().endswith((".mp4", ".webm")) else "img"
        if kind == "img":
            sz = _img_size(fp)
        else:
            sz = _mp4_dims(fp)
        if sz and (not prev or sz[0] * sz[1] > prev[0] * prev[1]):
            rec = {"path": fp, "kind": kind, "w": sz[0], "h": sz[1], "src": "tex"}
    if rec is None and prev:
        rec = None
    with _NATIVE_LOCK:
        _NATIVE_SRC[pid] = rec
    return rec


def _we_native_video(pdir, min_w=1280):
    """项目目录内的真视频素材（部分 scene 项目附带原生动画）。
    仅当分辨率 >= min_w 才采用：低分辨率真视频不如 GIF 可动，且会被拉伸得更丑。"""
    try:
        names = os.listdir(pdir)
    except OSError:
        return None
    for fn in sorted(names):
        if not fn.lower().endswith((".mp4", ".webm", ".mov", ".m4v")):
            continue
        fp = os.path.join(pdir, fn)
        try:
            if os.path.getsize(fp) < 512 * 1024:      # 太小必是占位/损坏
                continue
        except OSError:
            continue
        if fn.lower().endswith((".webm", ".mov", ".m4v")):
            # 非 mp4 容器无法用 tkhd 解析，信任文件大小（WE 原生 webm 均 ≥ 数 MB）
            return fn.replace("\\", "/")
        dims = _mp4_dims(fp)
        if dims and dims[0] >= min_w:
            return fn.replace("\\", "/")
    return None

def _m4k_cache_key(pid, tw, th, ss, tag="jpg"):
    """母版缓存键。带目标尺寸 + 超采样系数，换屏/换窗口比例不会命中旧尺寸母版。"""
    return "%s_%dx%d_s%d.%s" % (pid, int(tw * ss), int(th * ss), ss, tag)


# pid -> (源目录, 源文件名, 类型)。前端用 ?r=WxH 请求非默认比例母版时，
# 靠这张表反查源文件并按需重建（不预生成所有比例 —— 超分是秒级开销，
# 按需生成 + 磁盘缓存才是正解）。
_M4K_SRC = {}
_M4K_SRC_LOCK = threading.Lock()


def _m4k_register(pid, pdir, fname, kind):
    with _M4K_SRC_LOCK:
        _M4K_SRC[pid] = (pdir, fname, kind)


def _m4k_lookup(pid):
    with _M4K_SRC_LOCK:
        return _M4K_SRC.get(pid)


def _m4k_rebuild(pid, tw, th):
    """按前端请求的比例重建母版。返回 URL 或 None。

    前端只在视口比例与当前母版比例差异 >6% 时才带 ?r= 触发本路径，
    所以调用频率极低（拖窗口跨比例时一次），不会造成重建风暴。
    """
    ent = _m4k_lookup(pid)
    if not ent:
        return None
    pdir, fname, kind = ent
    try:
        # 视口物理像素已是最终上屏尺寸；再乘 MASTER_SS 做超采样母版。
        # 这里必须走「同步生成」而非后台队列：调用方是 /m4k/?r= 请求，
        # 语义就是「按这个比例给我图」，用户正在等这张图。
        if kind == "gif":
            m = _we_gif_make_cached(pid, pdir, tw, th, MASTER_SS)
        elif kind == "video":
            m = _we_vid_make_cached(pid, pdir, fname, tw, th, MASTER_SS)
        else:
            m = _wehd_make_cached(pid, pdir, fname, tw, th, MASTER_SS)
        return m.get("url") if m else None
    except (OSError, ValueError) as e:
        sys.stderr.write("[m4k] rebuild %s failed: %r\n" % (pid, e))
        return None


def _m4k_remember(pid, tw, th, ss, meta, tag="jpg"):
    with _M4K_MEM_LOCK:
        if len(_M4K_MEM) > 256:
            _M4K_MEM.clear()
        _M4K_MEM[_m4k_cache_key(pid, tw, th, ss, tag)] = meta
    return meta


def _m4k_meta(pid, tw, th, ss, tag="jpg"):
    with _M4K_MEM_LOCK:
        return _M4K_MEM.get(_m4k_cache_key(pid, tw, th, ss, tag))


def _wehd_item(pid, pdir, tw=None, th=None, ss=None, src_name="preview.jpg"):
    """WE 官方 preview.jpg（600~1024 方形小图）→ 4K 母版。

    与旧版「只出屏物理尺寸」的本质区别（旧版就是"糊"的直接来源之一）：
      · 母版按 MASTER_SS(默认 2x) 超采样生成 —— 3840x2160 母版映射到 1920x1080
        屏时由 GPU 做 2:1 降采样，等效 2x2 SSAA，边缘比直出 1080p 干净；
      · 低分辨率素材（600px 这类）自动走 Real-ESRGAN AI 超分 —— 实测证明
        纯插值到再大尺寸也不会产生新细节（上屏后 SSIM=1.0000），AI 超分是
        唯一能在低像素素材上补出可信高频的路径；
      · 失败逐级降级：AI 超分 -> 分阶段 Lanczos + 自适应锐化 -> 原始 preview，
        任何一级异常都不让壁纸列表挂掉。
    幂等缓存 _m4k/；键带尺寸+系数，换分辨率/换比例自动重建。
    """
    if not _PIL:
        return None
    src = os.path.join(pdir, src_name)
    if not os.path.isfile(src):
        return None
    _m4k_register(pid, pdir, src_name, "img")
    if tw is None or th is None:
        tw, th = _display_px()
    if ss is None:
        ss = MASTER_SS
    key = _m4k_cache_key(pid, tw, th, ss)
    cached = _m4k_meta(pid, tw, th, ss)
    if cached:
        return cached
    out = os.path.join(_M4K_DIR, key)

    # 已有文件且比源新 -> 直接复用（进程重启后不再重算）
    if os.path.isfile(out) and os.path.getsize(out) > 1024 \
            and os.path.getmtime(out) >= os.path.getmtime(src):
        ssz = _img_size(src)
        # 母版口径仅用于判定画质来源；对外展示一律用上屏口径（见 _screen_upscale）
        up_m = _eff_upscale(ssz, tw * ss, th * ss) if ssz else 0.0
        return _m4k_remember(pid, tw, th, ss, {
            "url": "/m4k/" + key, "w": int(tw * ss), "h": int(th * ss),
            "res": "%dx%d" % ssz if ssz else None,
            "upscale": _screen_upscale(ssz) or None,
            # 磁盘复用时原 mode 已随进程丢失。产物尺寸恒为母版尺寸；素材若原生
            # 就够大就不会走放大产线 —— 用「有效放大倍数是否越过超分阈值」反推
            # 是否超分过，保证 UI 标注与实际画质来源一致，不谎报也不虚报。
            "mode": ("sr" if up_m >= _SR_MIN_UPSCALE else "native"),
            "mb": round(os.path.getsize(out) / 1048576.0, 1)})

    # 未命中 -> 后台生成，接口立即返回占位信息（前端下一轮自动升级到 4K 母版）
    _m4k_spawn(lambda: _wehd_make_cached(pid, pdir, src_name, tw, th, ss), pid)
    ssz = _img_size(src)
    return {"url": "/m4k/" + key, "w": int(tw * ss), "h": int(th * ss),
            "res": "%dx%d" % ssz if ssz else None,
            "upscale": _screen_upscale(ssz) or None,
            "pending": True}


def _wehd_make_cached(pid, pdir, src_name, tw, th, ss):
    """后台线程入口：生成后写入内存缓存，供下一轮列表直接命中。"""
    m = _wehd_make(pid, pdir, src_name, tw, th, ss)
    if m:
        _m4k_remember(pid, tw, th, ss, m)
    return m


def _wehd_make(pid, pdir, src_name, tw, th, ss):
    """真正执行母版生成（只在后台线程里跑）。返回元信息或 None。"""
    src = os.path.join(pdir, src_name)
    key = _m4k_cache_key(pid, tw, th, ss)
    out = os.path.join(_M4K_DIR, key)
    with _M4K_SEM:
        try:
            os.makedirs(_M4K_DIR, exist_ok=True)
            if _wall4k is not None:
                r = _wall4k.build_master(src, out, tw, th, ss=ss)
                if r.get("ok"):
                    return {"url": "/m4k/" + key, "w": r["w"], "h": r["h"],
                            "res": "%dx%d" % tuple(r["src"]) if r.get("src") else None,
                            "upscale": r.get("upscale") or None, "mode": r.get("mode"),
                            "mb": round(os.path.getsize(out) / 1048576.0, 1)}
            # 降级：纯 Pillow cover 放大 + 自适应锐化（AI 超分不可用时的保底）
            mw, mh = int(tw * ss), int(th * ss)
            im = _wall4k.open_tolerant(src).convert("RGB")   # 容忍尾部截断的 JPEG
            ssz = im.size
            pre = _cover_to(im, mw, mh)
            up = _eff_upscale(ssz, mw, mh)
            if up > 1.15:
                pre = _wall4k.adaptive_sharpen(pre, up)
            pre.save(out, "JPEG", quality=97, subsampling=0, optimize=True)
            return {"url": "/m4k/" + key, "w": mw, "h": mh,
                    "res": "%dx%d" % ssz, "upscale": round(up, 2) if up > 1.15 else None,
                    "mode": "lanczos",
                    "mb": round(os.path.getsize(out) / 1048576.0, 1)}
        except (OSError, ValueError, ImportError) as e:
            sys.stderr.write("[m4k] %s failed: %r\n" % (pid, e))
            return None


def _we_gif4k(pid, pdir, tw=None, th=None, ss=None):
    """WE 官方 preview.gif（160~224px, 50 帧）→ 4K H.264 循环 MP4。

    官方动图只有 160~224px —— 直接铺满 1080p 屏是8~12 倍放大，必然块状马赛克。
    GIF 是 8bit 调色板格式，官方预览本身就限死在这个尺寸；但它是该scene 项目
    唯一的动画来源，所以逐帧送AI 超分 + NVENC 编码成 4K MP4，把「马赛克动画」
    救成「可看的 4K 动画」。失败返回 None（前端回退原 GIF，绝不空窗）。
    """
    if not _PIL or _wall4k is None:
        return None
    src = os.path.join(pdir, "preview.gif")
    if not os.path.isfile(src):
        return None
    _m4k_register(pid, pdir, "preview.gif", "gif")
    if tw is None or th is None:
        tw, th = _display_px()
    if ss is None:
        ss = MASTER_SS
    key = _m4k_cache_key(pid, tw, th, ss, "mp4")
    cached = _m4k_meta(pid, tw, th, ss, "mp4")
    if cached:
        return cached
    out = os.path.join(_M4K_DIR, key)
    if os.path.isfile(out) and os.path.getsize(out) > 4096 \
            and os.path.getmtime(out) >= os.path.getmtime(src):
        ssz = _img_size(src)
        return _m4k_remember(pid, tw, th, ss, {
            "url": "/m4k/" + key, "w": int(tw * ss), "h": int(th * ss),
            "res": "%dx%d" % ssz if ssz else None,
            "upscale": _screen_upscale(ssz) or None, "mode": "gif4k",
            "mb": round(os.path.getsize(out) / 1048576.0, 1)}, "mp4")
    # 未命中 -> 后台生成（GIF 逐帧超分要 4~5 分钟，绝不能阻塞列表 API）。
    # 先回退原始 GIF：它虽只有 160~224px，但至少马上能动；4K 母版就绪后
    # 前端下一轮轮询会自动切过去（url 从 /we/<pid>/preview.gif 变成 /m4k/...）。
    _m4k_spawn(lambda: _we_gif_make_cached(pid, pdir, tw, th, ss), pid)
    ssz = _img_size(src)
    return None


def _we_gif_make_cached(pid, pdir, tw, th, ss):
    """后台线程入口：GIF -> 4K MP4 生成，成功后写内存缓存。"""
    src = os.path.join(pdir, "preview.gif")
    key = _m4k_cache_key(pid, tw, th, ss, "mp4")
    out = os.path.join(_M4K_DIR, key)
    with _M4K_SEM:
        try:
            os.makedirs(_M4K_DIR, exist_ok=True)
            r = _wall4k.build_master_gif(src, out, tw, th, ss=ss)
            if r.get("ok") and os.path.isfile(out):
                ssz = _img_size(src)
                return _m4k_remember(pid, tw, th, ss, {
                    "url": "/m4k/" + key, "w": r["w"], "h": r["h"],
                    "res": "%dx%d" % ssz if ssz else None,
                    "upscale": _screen_upscale(ssz) or None,
                    "mode": "gif4k", "frames": r.get("frames"),
                    "mb": round(os.path.getsize(out) / 1048576.0, 1)}, "mp4")
        except (OSError, ValueError) as e:
            sys.stderr.write("[m4k] gif %s failed: %r\n" % (pid, e))
    return None


# ──────────────────────────────────────────── 视频母版（重编码 + 锐化）
# 实测依据：WE 视频码率跨度极大 —— 2878714045 只有 1864 kb/s（2560x1440，
# 块效应比 28.8，肉眼马赛克），而 3806392502 有 52872 kb/s。分级处理：
#   · 低码率（<8Mbps）或非 4K -> 高码率重编码到母版尺寸 + 按放大倍数锐化
#   · 已是 4K 且高码率（>=32Mbps）-> 直出。二次编码只会有代际损失，
#     花几分钟把 52Mbps 压更低是纯负收益。
# 附带收益：+faststart 让 moov 前置，浏览器立刻能播首帧。实测 802MB 的视频
# 只缓冲 15.8s/249s —— 这是用户说的"有些壁纸没有加载"的直接原因。
def _we_vid4k(pid, pdir, fname, tw=None, th=None, ss=None):
    """判断视频是否需要母版，返回母版信息 dict 或 None（表示直出原始视频）。

    契约与 _we_gif4k 一致：命中缓存直接返回；未命中则后台生成并返回
    pending 占位，让前端先播原视频、母版就绪后无缝升级（不黑屏、不假装 4K）。
    """
    if _wall4k is None or not fname:
        return None
    src = os.path.join(pdir, fname)
    if not os.path.isfile(src):
        return None
    _m4k_register(pid, pdir, fname, "video")
    if tw is None or th is None:
        tw, th = _display_px()
    if ss is None:
        ss = MASTER_SS
    key = _m4k_cache_key(pid, tw, th, ss, "mp4")
    cached = _m4k_meta(pid, tw, th, ss, "mp4")
    if cached:
        return cached
    out = os.path.join(_M4K_DIR, key)
    fresh = (os.path.isfile(out) and os.path.getsize(out) > 4096
             and os.path.getmtime(out) >= os.path.getmtime(src))
    if fresh:
        return _m4k_remember(pid, tw, th, ss, _vid_meta(
            pid, key, out, fname, mode="cache"), "mp4")
    # 磁盘未命中：先按内容判定值不值得重编码。不需要就直接返回 None（直出），
    # 需要则排队生成 —— 800MB 视频重编码要几分钟，绝不能阻塞列表 API。
    info = _wall4k._ffprobe_like(src)
    need, why = _wall4k.video_needs_master(info, int(tw * ss), src)
    if not need:
        return None
    _m4k_spawn(lambda: _we_vid_make_cached(pid, pdir, fname, tw, th, ss), pid)
    return {"url": "/m4k/" + key, "w": int(tw * ss), "h": int(th * ss),
            "res": "%dx%d" % (info.get("w", 0), info.get("h", 0)) or None,
            "pending": True, "why": why}


def _vid_meta(pid, key, out, fname, mode=None, info=None):
    """视频母版的对外元信息。res 用「上屏产物尺寸」而非源尺寸 ——
    UI 要显示的是用户实际看到的画质，源尺寸放在 srcRes 里备查。"""
    try:
        w, h = _mp4_dims(out)
    except (OSError, ValueError):
        w = h = 0
    ssz = None
    if info:
        ssz = "%dx%d" % (info.get("w", 0), info.get("h", 0))
    return {
        "url": "/m4k/" + key, "w": w, "h": h,
        "res": ("%dx%d" % (w, h)) if w else None, "srcRes": ssz,
        "mode": mode or "v4k", "sr": False, "v4k": True,
        "mb": round(os.path.getsize(out) / 1048576.0, 1) if os.path.isfile(out) else None,
    }


def _we_vid_make_cached(pid, pdir, fname, tw, th, ss):
    """后台线程入口：视频 -> 母版 MP4，成功后写内存缓存。"""
    src = os.path.join(pdir, fname)
    key = _m4k_cache_key(pid, tw, th, ss, "mp4")
    out = os.path.join(_M4K_DIR, key)
    with _M4K_SEM:
        try:
            os.makedirs(_M4K_DIR, exist_ok=True)
            r = _wall4k.build_master_video(src, out, tw, th, ss=ss)
            if r.get("ok") and os.path.isfile(out):
                return _m4k_remember(pid, tw, th, ss, _vid_meta(
                    pid, key, out, fname, mode="v4k", info=r), "mp4")
            sys.stderr.write("[m4k] video %s 未生成: %s\n"
                             % (pid, r.get("reason") or "未知原因"))
        except (OSError, ValueError) as e:
            sys.stderr.write("[m4k] video %s failed: %r\n" % (pid, e))
    return None

def _we_video_item(pid, title, pdir, fn, preview):
    """构造一条视频壁纸条目（video 类型与 scene 附带原生视频共用）。

    分级逻辑（实测依据见 _we_vid4k 上方注释）：
      · 需要母版且已就绪 -> url 指向 /m4k/ 4K 母版，标注 v4k
      · 需要母版但后台生成中 -> url 指向 /m4k/ 目标 + pending + fallback 原视频，
        前端先播原视频（马上能动），母版就绪后无缝升级，全程不黑屏
      · 不需要母版（原生 4K 高码率）-> 直出，零处理零损耗
    """
    src = os.path.join(pdir, fn)
    raw = "/we/%s/%s" % (pid, _we_url_safe(fn))
    item = {"id": pid, "name": title, "type": "video",
            "url": raw, "fallback": raw, "preview": preview, "native": True,
            "mb": round(os.path.getsize(src) / 1048576.0, 1)}
    if fn.lower().endswith(".mp4"):
        d = _mp4_dims(src)
        item["res"] = ("%dx%d" % d) if d else None
    m = _we_vid4k(pid, pdir, fn)          # None = 判定无需母版
    if m:
        item["url"] = m["url"]
        item["pending"] = bool(m.get("pending"))
        item["v4k"] = True
        if m.get("res"):
            item["res"] = m["res"]          # 上屏产物尺寸（用户实际看到的）
        if m.get("srcRes"):
            item["srcRes"] = m["srcRes"]    # 源尺寸备查
        if m.get("mb"):
            item["mb"] = m["mb"]
        # 如实命名：重编码母版的尺寸现在等于源尺寸（禁止上采样），
        # 所以叫"4K重编码"会误导 —— 只有真的落到 4K 才叫 4K。
        if not item["pending"]:
            item["name"] = title + (" (4K重编码)" if _is4k(m.get("res")) else " (高码率重编码)")
        else:
            item["why"] = m.get("why")
    return item


def _is4k(res):
    """标注用：分辨率字符串是否达到 4K 门槛（宽>=3840）。"""
    try:
        return int(str(res).split("x")[0]) >= 3840
    except (ValueError, IndexError):
        return False


# ---------------------------------------------------------- 原生源条目构造
# 原生源文件解包在 _wetexture/ 缓存目录里，不在项目目录，因此不能复用
# _we_video_item（它按 pdir + fname 拼路径）。这里给原生源单独一套 URL 与母版流程。

_NATIVE_URL = {}


def _native_rel_url(path):
    """把 _wetexture 下的文件映射成可访问 URL，并登记到静态路由表。"""
    try:
        rel = os.path.relpath(path, _TEX_DIR).replace("\\", "/")
    except ValueError:
        return None
    url = "/wetexture/" + rel
    _NATIVE_URL[url] = path
    return url


def _we_native_video_item(pid, title, nat, preview, pdir):
    """scene.pkg 内嵌的原生动画（完整 MP4）条目。

    分级与普通视频一致，但**默认直出**：原生纹理里的 MP4 是素材作者放的
    真实动画（实测 Minecraft 1920x1080、午后闲暇 1934x1080），它已经是
    干净的高码率源。旧版无条件重编码到 4K，对 1920x1080 的源就是上采样
    —— 插值伪高频 + 每像素码率减半，实测比直出更糊。只有码率过低
    （<8Mbps，块效应明显）时才值得重编码抹平马赛克。
    """
    url = _native_rel_url(nat["path"])
    if not url:
        return None
    src = nat["path"]
    item = {"id": pid, "name": title, "type": "video",
            "url": url, "fallback": url, "preview": preview, "native": True,
            "origin": "tex",
            "srcRes": "%dx%d" % (nat["w"], nat["h"]),
            "res": "%dx%d" % (nat["w"], nat["h"]),
            "mb": round(os.path.getsize(src) / 1048576.0, 1)}
    tw, th = _display_px()
    ss = MASTER_SS
    key = _m4k_cache_key(pid, tw, th, ss, "mp4")
    out = os.path.join(_M4K_DIR, key)
    cached = _m4k_meta(pid, tw, th, ss, "mp4")
    if cached and cached.get("url"):
        item.update(cached)
        return item
    if os.path.isfile(out) and os.path.getsize(out) > 4096:
        item["url"] = "/m4k/" + key
        item["mb"] = round(os.path.getsize(out) / 1048576.0, 1)
        try:
            dw, dh = _mp4_dims(out)
            if dw:
                item["res"] = "%dx%d" % (dw, dh)
        except (OSError, ValueError):
            pass
        return item
    # 磁盘也没有：先按内容判定值不值得重编码。不需要就保持直出（零损失、
    # 零等待）；需要才排队生成，且期间直出原生 MP4，不黑屏。
    # 判据复用 wall4k.video_needs_master —— 它含**可播性闸门**（moov 位置 +
    # 体积）。这里原先写死 kbps<8000，会让 338MB 慢启动包直接被丢给浏览器，
    # 表现为"壁纸加载不出来"（readyState 恒 0）。
    info = _wall4k._ffprobe_like(src) if _wall4k is not None else {}
    need = False
    if info:
        need, _why = _wall4k.video_needs_master(info, int(tw * ss), src)
    if not need:
        return item
    # 未生成：先给原生视频直出（马上能动），后台重编码，完成后前端自动升级
    _m4k_register(pid, _TEX_DIR, os.path.basename(src), "video")
    _m4k_spawn(lambda: _we_vid_make_cached(pid, _TEX_DIR,
                                            os.path.basename(src),
                                            tw, th, ss), pid)
    item["pending"] = True
    item["v4k"] = True
    return item


def _we_native_img_item(pid, title, nat, preview, pdir, has_jpg):
    """scene.pkg 原生静图条目：原生纹理 -> 4K 母版（必要时才超分）。"""
    url = _native_rel_url(nat["path"])
    if not url:
        return None
    fb = "/we/%s/preview.jpg" % pid if has_jpg else url
    item = {"id": pid, "name": title, "type": "live",
            "url": url, "fallback": fb, "preview": fb,
            "origin": "tex", "srcRes": "%dx%d" % (nat["w"], nat["h"]),
            "native": True}
    tw, th = _display_px()
    ss = MASTER_SS
    key = _m4k_cache_key(pid, tw, th, ss)
    cached = _m4k_meta(pid, tw, th, ss)
    if cached and cached.get("url"):
        item.update(cached)
        return item
    out = os.path.join(_M4K_DIR, key)
    if os.path.isfile(out) and os.path.getsize(out) > 1024:
        item["url"] = "/m4k/" + key
        item["master"] = "%dx%d" % (int(tw * ss), int(th * ss))
        item["mb"] = round(os.path.getsize(out) / 1048576.0, 1)
        return item
    # 后台生成 4K 母版；期间先直出原生图（本身已是 2560~4096 宽，不糊）
    _m4k_register(pid, _TEX_DIR, os.path.basename(nat["path"]), "img")
    _m4k_spawn(lambda: _wehd_make_cached(pid, _TEX_DIR,
                                          os.path.basename(nat["path"]),
                                          tw, th, ss), pid)
    item["master"] = "%dx%d" % (int(tw * ss), int(th * ss))
    item["pending"] = True
    return item


def we_wallpapers():
    """扫描 WE 创意工坊 project.json → video(webm/mp4) / web(iframe) / scene(静帧)"""
    root = we_root()
    if not root:
        return []
    if _WE_CACHE["items"] and time.time() - _WE_CACHE["t"] < 60:
        return _WE_CACHE["items"]
    items = []
    try:
        proj_ids = sorted(os.listdir(root))
    except OSError:
        return []
    for pid in proj_ids:
        pdir = os.path.join(root, pid)
        pj = os.path.join(pdir, "project.json")
        if not os.path.isfile(pj):
            continue
        try:
            with open(pj, encoding="utf-8-sig", errors="replace") as f:
                meta = json.load(f)
        except (OSError, ValueError):
            continue
        typ = (meta.get("type") or "").lower()
        title = (meta.get("title") or pid).strip() or pid
        preview = "/we/%s/preview.jpg" % pid if os.path.isfile(os.path.join(pdir, "preview.jpg")) else None
        if typ == "video":
            fn = meta.get("file") or ""
            if os.path.isfile(os.path.join(pdir, fn)):
                items.append(_we_video_item(pid, title, pdir, fn, preview))
        elif typ == "web":
            fn = meta.get("file") or ""
            if os.path.isfile(os.path.join(pdir, fn)):
                items.append({"id": pid, "name": title, "type": "web",
                              "url": "/we/%s/%s" % (pid, _we_url_safe(fn)), "preview": preview})
        elif typ == "scene":
            # scene 壁纸素材优先级（2026-10-03 画质终判，实测取证）：
            #   0) **scene.pkg 原生素材**（最高优先，新增）
            #      商店页 preview.* 只是缩略图（实测 160x160 ~ 1024x1024），
            #      真实素材封在 scene.pkg 里：实测拿到 1581x936 / 2560x1440 /
            #      3840x2160 / 4096x2160 原生纹理，甚至 1920x1080 完整 MP4。
            #      这一级是"很糊"的真正根治点 —— 换源，而不是把缩略图拉大。
            #   1) 项目目录内的真视频 mp4/webm（部分 scene 项目附带原生动画）
            #   2) preview.gif —— 官方动态预览，但仅 160~224px，放全屏要放大
            #      8~12 倍，必然块状马赛克（用户实机截图证实）。
            #   3) preview.jpg —— 600~1024 方形静图，最后兜底。
            nat = _native_source(pid, pdir)
            native = _we_native_video(pdir)
            has_jpg = os.path.isfile(os.path.join(pdir, "preview.jpg"))
            has_gif = os.path.isfile(os.path.join(pdir, "preview.gif"))
            if nat and nat["kind"] == "video":
                # 原生动画（.tex 内嵌完整 MP4）—— 这就是 WE 原生的动态效果
                items.append(_we_native_video_item(pid, title, nat, preview, pdir))
            elif nat and nat["kind"] == "img":
                items.append(_we_native_img_item(pid, title, nat, preview, pdir, has_jpg))
            elif native:
                items.append(_we_video_item(pid, title, pdir, native, preview))
            elif has_gif:
                # 官方动图仅 160~224px（8bit 调色板，官方预览本身就限死这个尺寸）。
                # 直接铺满 1080p = 8~12 倍放大 -> 块状马赛克（用户实机截图证实）。
                # 现在走 4K 母版管线：逐帧 AI 超分 -> 3840x2160 -> NVENC 编码 MP4，
                # 得到真正 4K 的循环动画。失败才回退原 GIF（不假装成功）。
                g4k = _we_gif4k(pid, pdir)
                gsz = _img_size(os.path.join(pdir, "preview.gif"))
                up = _screen_upscale(gsz)
                gname = title + " (4K超分)" if g4k else title
                # 4K 母版就绪 -> video（4K 循环动画）；未就绪（后台生成中）->
                # 仍给 /m4k/ 的目标 URL 并标 pending，前端轮询到就绪自动升级，
                # 期间用 fallback 原 GIF 顶上，绝不留空窗、也绝不假装 4K 已好。
                items.append({"id": pid, "name": gname, "type": "video",
                              "url": g4k["url"] if g4k else "/m4k/" +
                              _m4k_cache_key(pid, _display_px()[0], _display_px()[1],
                                             MASTER_SS, "mp4"),
                              "fallback": "/we/%s/preview.gif" % pid,
                              "preview": ("/we/%s/preview.jpg" % pid) if has_jpg else None,
                              "native": True, "sr": True, "pending": not bool(g4k),
                              "res": ("%dx%d" % (g4k["w"], g4k["h"])) if g4k else
                                      "%dx%d" % (int(_display_px()[0] * MASTER_SS),
                                                 int(_display_px()[1] * MASTER_SS)),
                              "srcRes": ("%dx%d" % gsz) if gsz else None,
                              "upscale": up or None,
                              "mb": g4k.get("mb") if g4k else None})
            elif has_jpg:
                # scene 静图：官方 preview.jpg 600~1024 方形 -> 4K 母版（AI 超分优先）。
                # 原生分辨率真动画走芯片上的「🖥 桌面」按钮（WE 真渲染 scene.pkg）。
                hd = _wehd_item(pid, pdir)
                if hd and hd.get("url"):
                    items.append({"id": pid, "name": title, "type": "live",
                                  "url": hd["url"],
                                  "fallback": "/we/%s/preview.jpg" % pid,
                                  "preview": "/we/%s/preview.jpg" % pid,
                                  "master": "%dx%d" % (hd.get("w", 0), hd.get("h", 0)),
                                  "sr": hd.get("mode") == "sr",
                                  "res": hd.get("res"), "upscale": hd.get("upscale"),
                                  "mb": hd.get("mb")})
                else:
                    ssz = _img_size(os.path.join(pdir, "preview.jpg"))
                    up = _screen_upscale(ssz)
                    items.append({"id": pid, "name": title, "type": "live",
                                  "url": "/we/%s/preview.jpg" % pid,
                                  "fallback": "/we/%s/preview.jpg" % pid,
                                  "preview": "/we/%s/preview.jpg" % pid,
                                  "res": ("%dx%d" % ssz) if ssz else None, "upscale": up or None})
    _WE_CACHE["t"] = time.time()
    _WE_CACHE["items"] = items
    return items

# ---------- WE 运行时取帧桥（scene 壁纸深度集成）----------
# 原理：WE 把场景壁纸渲染在桌面 WorkerW 层，用 PrintWindow(PW_RENDERFULLCONTENT)
# 直接抓取该窗口实时画面，编码 MJPEG 流给前端 —— 场景壁纸即在本软件里真动态。
import ctypes
from ctypes import wintypes as _wt

try:
    from PIL import Image as _PImage
    from PIL import ImageFilter as _PImageFilter
    _PIL = True
except ImportError:
    _PIL = False

_u32 = ctypes.windll.user32 if os.name == "nt" else None
_g32 = ctypes.windll.gdi32 if os.name == "nt" else None
_k32 = ctypes.windll.kernel32 if os.name == "nt" else None
_dwm = ctypes.windll.dwmapi if os.name == "nt" else None
if _u32:
    try:  # DPI 感知：取帧按物理像素（4K 屏 = 真 4K 画质）
        ctypes.windll.shcore.SetProcessDpiAwareness(2)
    except (AttributeError, OSError):
        try:
            _u32.SetProcessDPIAware()
        except OSError:
            pass
PW_RENDERFULLCONTENT = 2
_LIVE = {"hwnd": 0, "t": 0.0, "fail": 0.0}

class _BMIH(ctypes.Structure):
    _fields_ = [("biSize", ctypes.c_uint32), ("biWidth", ctypes.c_int32),
                ("biHeight", ctypes.c_int32), ("biPlanes", ctypes.c_uint16),
                ("biBitCount", ctypes.c_uint16), ("biCompression", ctypes.c_uint32),
                ("biSizeImage", ctypes.c_uint32), ("biXPelsPerMeter", ctypes.c_int32),
                ("biYPelsPerMeter", ctypes.c_int32), ("biClrUsed", ctypes.c_uint32),
                ("biClrImportant", ctypes.c_uint32)]

def we_wallpaper_hwnd():
    """找 WE 自己的渲染窗 WPEDesktopDX11Window（D3D 交换链，DWM 缩略图可实时取）；
    找不到退回 WorkerW/Progman"""
    if not _u32:
        return 0
    now = time.time()
    if _LIVE["hwnd"] and _u32.IsWindow(_LIVE["hwnd"]) and now - _LIVE["t"] < 5:
        return _LIVE["hwnd"]
    hit = []
    prog = _u32.FindWindowW("Progman", None)
    # 路径1：Progman → WorkerW → WPEDesktopDX11Window（WE 本机实证挂载点）
    if prog:
        wk = _u32.FindWindowExW(prog, 0, "WorkerW", None)
        while wk:
            wpe = _u32.FindWindowExW(wk, 0, "WPEDesktopDX11Window", None)
            if wpe:
                hit.append(wpe)
                break
            wk = _u32.FindWindowExW(prog, wk, "WorkerW", None)
    # 路径2：顶层 WorkerW 下的 WPE 子窗
    if not hit:
        w = 0
        while True:
            w = _u32.FindWindowExW(0, w, "WorkerW", None)
            if not w:
                break
            wpe = _u32.FindWindowExW(w, 0, "WPEDesktopDX11Window", None)
            if wpe:
                hit.append(wpe)
                break
    hwnd = hit[0] if hit else (prog or 0)
    _LIVE.update(hwnd=hwnd, t=now)
    return hwnd

# ---------- WE 实况窗（DWM 缩略图原生实时预览） ----------
# 原理：DwmRegisterThumbnail(实况窗, Progman)——即任务栏预览的同款系统合成机制，
# DWM 直接把正在播放的桌面壁纸逐帧合成到实况窗客户区 = 原生分辨率原生帧率的实时画面。
# 这不是截屏（PrintWindow/BitBlt 全部取不到缩略内容），而是显示级合成，无遮挡污染。
class _LVWIN:
    hwnd = 0
    thumb = None
    thr = None
    on = False

def _lv_rect():
    sw, sh = _u32.GetSystemMetrics(0), _u32.GetSystemMetrics(1)
    w = min(560, sw // 3)
    h = round(w * 9 / 16)
    return sw - w - 28, sh - h - 100, w, h

def _lv_thread():
    try:
        sw, sh = _u32.GetSystemMetrics(0), _u32.GetSystemMetrics(1)
        x, y, w, h = _lv_rect()
        # 屏幕物理像素换算（服务进程非 DPI aware 时 GetSystemMetrics 返回虚拟化值，先拉起感知）
        try:
            ctypes.windll.shcore.SetProcessDpiAwareness(2)
        except Exception:
            try:
                _u32.SetProcessDPIAware()
            except Exception:
                pass
        sw, sh = _u32.GetSystemMetrics(0), _u32.GetSystemMetrics(1)
        x, y, w, h = _lv_rect()
        hwnd = _u32.CreateWindowExW(0x08000081, "STATIC", "VTaskLiveView", 0x80000000,
                                    x, y, w, h, 0, 0, 0, None)  # TOOLWINDOW|NOACTIVATE|TOPMOST
        if not hwnd:
            return
        _LVWIN.hwnd = hwnd
        _u32.SetWindowPos(hwnd, -1, 0, 0, 0, 0, 0x0053)  # TOPMOST|NOSIZE|NOMOVE|SHOWWINDOW
        _u32.ShowWindow(hwnd, 5)
        src = _u32.FindWindowW("Progman", None) or we_wallpaper_hwnd()
        if src:
            th = _wt.HANDLE()
            if _dwm.DwmRegisterThumbnail(hwnd, src, ctypes.byref(th)) == 0:
                _LVWIN.thumb = th.value
                class _TP(ctypes.Structure):
                    _fields_ = [("dwFlags", _wt.DWORD), ("rcDestination", _wt.RECT),
                                ("rcSource", _wt.RECT), ("opacity", ctypes.c_byte),
                                ("fVisible", _wt.BOOL), ("fSourceClientAreaOnly", _wt.BOOL)]
                p = _TP()
                p.rcDestination = _wt.RECT(0, 0, w, h)
                p.fVisible = True
                p.fSourceClientAreaOnly = True
                p.opacity = 255
                _dwm.DwmUpdateThumbnailProperties(th, ctypes.byref(p))
        msg = _wt.MSG()
        while _LVWIN.on:
            while _u32.PeekMessageW(ctypes.byref(msg), 0, 0, 0, 1):
                _u32.TranslateMessage(ctypes.byref(msg))
                _u32.DispatchMessageW(ctypes.byref(msg))
            time.sleep(0.05)
    finally:
        if _LVWIN.thumb:
            for nm in ("DwmUnRegisterThumbnail", "DwmUnregisterThumbnail"):
                fn = getattr(_dwm, nm, None)
                if fn:
                    try:
                        fn(_LVWIN.thumb)
                    except OSError:
                        pass
                    break
            _LVWIN.thumb = None
        if _LVWIN.hwnd:
            _u32.DestroyWindow(_LVWIN.hwnd)
            _LVWIN.hwnd = 0

def we_liveview(on):
    """开关实况小窗；返回是否成功"""
    if not (_u32 and _g32 and _dwm):
        return False
    if on:
        if _LVWIN.thr and _LVWIN.thr.is_alive() and _LVWIN.on:
            return True
        _LVWIN.on = True
        _LVWIN.thr = threading.Thread(target=_lv_thread, daemon=True)
        _LVWIN.thr.start()
        threading.Thread(target=we_cmd, args=("hideIcons",), daemon=True).start()
        return True
    was = _LVWIN.on
    _LVWIN.on = False
    if was:
        threading.Thread(target=we_cmd, args=("showIcons",), daemon=True).start()
    return True

def _find_window_rect(cls_name=None, own_pid=False):
    """按类名/属主进程找可见窗口矩形（找不到返回 None）"""
    hits = []
    if own_pid:
        proto = ctypes.WINFUNCTYPE(_wt.BOOL, _wt.HWND, _wt.LPARAM)
        me = _k32.GetCurrentProcessId()

        def cb(h, lp):
            pid = _wt.DWORD()
            _u32.GetWindowThreadProcessId(h, ctypes.byref(pid))
            if pid.value == me and _u32.IsWindowVisible(h):
                rc = _wt.RECT()
                _u32.GetWindowRect(h, ctypes.byref(rc))
                if (rc.right - rc.left) > 200 and (rc.bottom - rc.top) > 150:
                    hits.append((rc.right - rc.left) * (rc.bottom - rc.top))
                    hits.append((rc.left, rc.top, rc.right, rc.bottom))
            return True
        _u32.EnumWindows(proto(cb), 0)
    else:
        h = _u32.FindWindowW(cls_name, None)
        if h and _u32.IsWindowVisible(h):
            rc = _wt.RECT()
            _u32.GetWindowRect(h, ctypes.byref(rc))
            hits = [(rc.right - rc.left) * (rc.bottom - rc.top), (rc.left, rc.top, rc.right, rc.bottom)]
    if len(hits) >= 2:
        return hits[1]
    return None

class _LIVE_PROBE:
    """DWM 缩略图探针（原生分辨率·沉底隐形）：
    探针窗=全屏原生像素大小的 STATIC 窗，沉到 Z 序最底（被本应用/其他窗口盖住，用户不可见）。
    DwmRegisterThumbnail(探针, 源窗) 把壁纸动画实时合成进探针的窗口重定向表面；
    实证（2026-10-03）：窗口被不透明窗完全盖住时，GetWindowDC(探针)+BitBlt 读到的
    仍是与可见时逐像素相同的缩略图内容（DWM 持续合成），而屏幕 DC 是遮挡物——
    因此抓探针自身表面 = 原生全分辨率真动画 + 零画面污染。"""
    hwnd = 0
    thumb = None
    rect = (0, 0, 1280, 720)
    shown = False

def _probe_ensure():
    """惰性创建探针窗（全屏原生像素，沉底）+ 注册 DWM 缩略图（失败返回 False）"""
    if not (_u32 and _g32 and _dwm):
        return False
    if _LIVE_PROBE.hwnd and _u32.IsWindow(_LIVE_PROBE.hwnd):
        return _probe_retarget()
    sw, sh = _u32.GetSystemMetrics(0), _u32.GetSystemMetrics(1)
    _LIVE_PROBE.rect = (0, 0, sw, sh)
    WS_POPUP, WS_EX = 0x80000000, 0x08000080  # TOOLWINDOW|TRANSPARENT|NOACTIVATE|TOPMOST
    hwnd = _u32.CreateWindowExW(WS_EX, "STATIC", "VTaskLiveProbe", WS_POPUP,
                                0, 0, sw, sh, 0, 0, 0, None)
    if not hwnd:
        return False
    _LIVE_PROBE.hwnd = hwnd
    if not _probe_retarget():
        return False
    # 沉底：藏在所有普通窗口之下（表面合成不受遮挡影响，见类注释）
    _u32.SetWindowPos(hwnd, 1, 0, 0, 0, 0, 0x0013)  # HWND_BOTTOM|NOMOVE|NOSIZE|NOACTIVATE
    return True

def _probe_retarget():
    """（重新）注册缩略图到当前 WPE 渲染窗；子窗被 DWM 拒绝（E_INVALIDARG 实证）时
    回退顶层 Progman——Progman 缩略图含整棵 WorkerW/WPE 子树，即完整实时壁纸"""
    if not _LIVE_PROBE.hwnd:
        return False
    candidates = [we_wallpaper_hwnd()]
    prog = _u32.FindWindowW("Progman", None)
    if prog:
        candidates.append(prog)
    for src in candidates:
        if not src or not _u32.IsWindow(src):
            continue
        if _LIVE_PROBE.thumb:
            for nm in ("DwmUnRegisterThumbnail", "DwmUnregisterThumbnail"):
                fn = getattr(_dwm, nm, None)
                if fn:
                    try:
                        fn(_LIVE_PROBE.thumb)
                    except OSError:
                        pass
                    break
            _LIVE_PROBE.thumb = None
        thumb = _wt.HANDLE()
        if _dwm.DwmRegisterThumbnail(_LIVE_PROBE.hwnd, src, ctypes.byref(thumb)) != 0:
            continue
        _LIVE_PROBE.thumb = thumb.value
        class DWM_THUMB_PROPS(ctypes.Structure):
            _fields_ = [("fSource", _wt.DWORD), ("rcDestination", _wt.RECT),
                        ("rcSource", _wt.RECT), ("opacity", ctypes.c_byte),
                        ("fVisible", _wt.BOOL), ("fSourceClientAreaOnly", _wt.BOOL)]
        props = DWM_THUMB_PROPS()
        props.rcDestination = _wt.RECT(*_probe_client_rect())
        props.fSourceClientAreaOnly = True
        props.fVisible = True
        props.opacity = 255
        _dwm.DwmUpdateThumbnailProperties(_LIVE_PROBE.thumb, ctypes.byref(props))
        _u32.SetWindowPos(_LIVE_PROBE.hwnd, 1, 0, 0, 0, 0, 0x0053)  # BOTTOM|NOSIZE|NOMOVE|SHOWWINDOW
        _u32.ShowWindow(_LIVE_PROBE.hwnd, 5)
        _LIVE_PROBE.shown = True
        return True
    return False

def _probe_client_rect():
    x, y, r, b = _LIVE_PROBE.rect
    return (0, 0, r - x, b - y)

def _probe_hide():
    if _LIVE_PROBE.hwnd and _LIVE_PROBE.shown:
        _u32.ShowWindow(_LIVE_PROBE.hwnd, 0)
        _LIVE_PROBE.shown = False

def we_live_frame(maxw=0):
    """经 DWM 缩略图探针抓 WE 壁纸真动画一帧 → JPEG；探针不可用返回 None。
    关键：读探针自身的窗口重定向表面（GetWindowDC(探针)），不是屏幕——
    探针沉底被完全遮挡时表面仍被 DWM 持续合成（2026-10-03 实证），
    抓到的永远是壁纸，绝不混入任何遮挡窗口（镜中镜根治）"""
    if not (_PIL and _u32 and _g32):
        return None
    if not _probe_ensure():
        return None
    x, y, r, b = _LIVE_PROBE.rect
    w, h = r - x, b - y
    wdc = _u32.GetWindowDC(_LIVE_PROBE.hwnd)
    if not wdc:
        return None
    mem = _g32.CreateCompatibleDC(wdc)
    bmp = _g32.CreateCompatibleBitmap(wdc, w, h)
    old = _g32.SelectObject(mem, bmp)
    ok = _g32.BitBlt(mem, 0, 0, w, h, wdc, 0, 0, 0x00CC0020)
    bmi = _BMIH()
    bmi.biSize = ctypes.sizeof(_BMIH)
    bmi.biWidth, bmi.biHeight = w, -h
    bmi.biPlanes, bmi.biBitCount = 1, 32
    buf = ctypes.create_string_buffer(w * h * 4)
    got = _g32.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(bmi), 0) if ok else False
    _g32.SelectObject(mem, old)
    _g32.DeleteObject(bmp)
    _g32.DeleteDC(mem)
    _u32.ReleaseDC(_LIVE_PROBE.hwnd, wdc)
    if not got:
        return None
    img = _PImage.frombuffer("RGB", (w, h), buf, "raw", "BGRX", 0, 1)
    if maxw and img.width > maxw:
        img = img.resize((maxw, round(img.height * maxw / img.width)), _PImage.BILINEAR)
    return _jpg(img, 85)

def _jpg(img, q=88):
    import io
    b = io.BytesIO()
    img.save(b, "JPEG", quality=q)
    return b.getvalue()

# ---------- WE 控制通道（-control 走运行中实例的 WPXCMD_ 命名管道）----------
_LIVE_REFCOUNT = {"n": 0, "lock": threading.Lock()}
_LIVE_FRAME = {"jpg": None, "t": 0.0, "sig": None, "stall": 0, "lock": threading.Lock()}
_PREVIEW_TIMER = {"t": None, "lock": threading.Lock()}
_CAP_MAXW = 0      # 0=原生全分辨率取帧（1920×1080 物理像素，探针表面直读无遮挡）
_CAP_DT = 0.1      # 帧间隔 → ~10fps（全分辨率编码更重，10fps 已流畅）

def _capture_loop():
    """常驻捕获线程：抓取+降采样编码，流端只推送。异常自愈（线程崩=画面永久冻结）"""
    n = 0
    idle = 0
    while True:
        try:
            with _LIVE_REFCOUNT["lock"]:
                active = _LIVE_REFCOUNT["n"] > 0
            if not active:
                idle += 1
                if idle == 33 and _LIVE.get("restore"):   # 无客户端 ~10s → 兜底还原一次
                    threading.Thread(target=we_restore_desktop, daemon=True).start()
                time.sleep(0.3)
                continue
            idle = 0
            n += 1
            if n % 50 == 0:                    # 每 ~5s 重assert图标隐藏（防 Explorer 重绘回显）
                threading.Thread(target=we_cmd, args=("hideIcons",), daemon=True).start()
            f = we_live_frame(_CAP_MAXW)       # 降采样：降 CPU/内存带宽，画面依旧清晰
            if f is not None:
                sig = hashlib.md5(f).hexdigest()
                with _LIVE_FRAME["lock"]:
                    stall = _LIVE_FRAME["stall"] + 1 if sig == _LIVE_FRAME["sig"] else 0
                    _LIVE_FRAME["sig"] = sig
                    _LIVE_FRAME["stall"] = stall
                    _LIVE_FRAME["jpg"] = f
                    _LIVE_FRAME["t"] = time.time()
                time.sleep(_CAP_DT)            # ~12fps：风景场景流畅足够，CPU 减半以上
            else:
                time.sleep(0.25)
        except Exception:
            time.sleep(0.5)                    # 任何异常不杀线程，下一轮自愈

threading.Thread(target=_capture_loop, daemon=True).start()

def we_exe():
    """wallpaper64.exe 路径：由 workshop 根上溯 Steam 库 → steamapps/common/wallpaper_engine"""
    root = we_root()
    if root:
        lib = root
        for _ in range(4):
            lib = os.path.dirname(lib)          # 431960→content→workshop→steamapps→库根
        cand = os.path.join(lib, "steamapps", "common", "wallpaper_engine", "wallpaper64.exe")
        if os.path.isfile(cand):
            return cand
    for c in (r"D:\steam\steamapps\common\wallpaper_engine\wallpaper64.exe",
              r"C:\Program Files (x86)\Steam\steamapps\common\wallpaper_engine\wallpaper64.exe"):
        if os.path.isfile(c):
            return c
    return None

def we_cmd(*args):
    exe = we_exe()
    if not exe:
        return False
    try:
        r = subprocess.run([exe, "-control"] + list(args),
                           capture_output=True, timeout=15)
        return r.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False

def _find_key(obj, name):
    """递归找第一个同名键的值（WE config.json 结构时变，位置不固定）"""
    if isinstance(obj, dict):
        for k, v in obj.items():
            if k == name:
                return v
            r = _find_key(v, name)
            if r is not None:
                return r
    elif isinstance(obj, list):
        for v in obj:
            r = _find_key(v, name)
            if r is not None:
                return r
    return None

def we_desktop_wallpaper():
    """读 WE config.json 里用户当前桌面壁纸文件（Monitor0），用于临时渲染后还原"""
    exe = we_exe()
    if not exe:
        return None
    cfg = os.path.join(os.path.dirname(exe), "config.json")
    try:
        with open(cfg, encoding="utf-8") as f:
            data = json.load(f)
        # 还原目标优先取「用户最近在 WE UI 里主动设置的一张」（wallpaperconfigrecent[0]），
        # 防止我们此前临时切换留下的 selectedwallpapers 脏状态被当成用户原壁纸
        rec = _find_key(data, "wallpaperconfigrecent") or []
        if rec:
            rsw = _find_key(rec[0], "selectedwallpapers") or {}
            f = next(iter(rsw.values()), {}).get("file")
            if f:
                return f
        sw = _find_key(data, "selectedwallpapers") or {}
        mon = next(iter(sw.values()), {})
        return mon.get("file")
    except (OSError, ValueError, StopIteration):
        return None
    return None

_LIVE["restore"] = None   # 用户原桌面壁纸（还原用）；_LIVE 已含 hwnd/t/fail

def we_open_scene(pid):
    """把桌面 WE 临时切到指定场景工程（真渲染该 scene.pkg）：
    首次切换前记录用户原壁纸，镜像结束自动还原 —— Windows 桌面最终不被改变"""
    root = we_root()
    pkg = os.path.join(root or "", pid, "scene.pkg")
    if not os.path.isfile(pkg):
        return False
    with _LIVE_REFCOUNT["lock"]:
        first_switch = _LIVE["restore"] is None
        if first_switch:
            _LIVE["restore"] = we_desktop_wallpaper()
    ok = we_cmd("openWallpaper", "-file", pkg)
    we_cmd("hideIcons")
    return ok

def we_restore_desktop():
    """把桌面 WE 还原为用户原壁纸 + 恢复图标"""
    f = _LIVE.get("restore")
    if f and os.path.isfile(f):
        we_cmd("openWallpaper", "-file", f)
    we_cmd("showIcons")

def we_live_client_begin():
    """首个取帧客户端接入 → 隐藏桌面图标（镜像画面纯净）"""
    with _LIVE_REFCOUNT["lock"]:
        _LIVE_REFCOUNT["n"] += 1
        first = _LIVE_REFCOUNT["n"] == 1
    if first:
        threading.Thread(target=we_cmd, args=("hideIcons",), daemon=True).start()

def we_live_client_end():
    """取帧客户端全部断开 → 还原用户原桌面壁纸 + 恢复图标"""
    with _LIVE_REFCOUNT["lock"]:
        _LIVE_REFCOUNT["n"] = max(0, _LIVE_REFCOUNT["n"] - 1)
        last = _LIVE_REFCOUNT["n"] == 0
    if last:
        threading.Thread(target=we_restore_desktop, daemon=True).start()

import atexit as _atexit
_atexit.register(we_restore_desktop)
threading.Timer(2.0, lambda: we_restore_desktop()).start()  # 启动兜底：还原用户原壁纸与图标

BLOCK_HEAD = re.compile(r"^## \[([A-Za-z0-9_\-\.]+)\]\s*(.+)$")
FIELD = re.compile(r"^-\s+(册|域|行数|前置|书路径|验收|状态|领取人|结果|时间):\s*(.*)$")

def parse_md():
    """解析 taskboard.md → tasks 列表 + 原始行"""
    with open(MD_PATH, encoding="utf-8") as f:
        lines = f.read().split("\n")
    tasks, cur, i = [], None, 0
    while i < len(lines):
        m = BLOCK_HEAD.match(lines[i])
        if m:
            cur = {"_start": i, "_end": i + 1, "id": m.group(1), "title": m.group(2).strip(),
                   "册": "", "域": "", "行数": "", "前置": "", "书路径": "", "验收": "",
                   "状态": "待领", "领取人": "", "结果": "", "时间": ""}
            tasks.append(cur)
        elif cur is not None:
            fm = FIELD.match(lines[i])
            if fm:
                cur[fm.group(1)] = fm.group(2).strip()
                cur["_end"] = i + 1
            elif lines[i].strip() == "" and cur["_end"] == i:
                cur["_end"] = i + 1
            else:
                cur = None  # 离开块
        i += 1
    for t in tasks:
        t["langs"], t["files"] = infer_langs(t)
    return lines, tasks

def write_back(lines, tasks):
    """把任务状态写回原始行并原子落盘"""
    for t in tasks:
        block_lines = lines[t["_start"]:t["_end"]]
        for key in ("状态", "领取人", "结果", "时间"):
            pat = re.compile(r"^-\s+" + key + r":\s*(.*)$")
            newv = t.get(key, "")
            hit = False
            for j in range(len(block_lines)):
                if pat.match(block_lines[j]):
                    block_lines[j] = f"- {key}: {newv}"
                    hit = True
                    break
            if not hit:
                block_lines.append(f"- {key}: {newv}")
        lines[t["_start"]:t["_end"]] = block_lines
    tmp = MD_PATH + ".tmp"
    with open(tmp, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines))
    os.replace(tmp, MD_PATH)

def done_ids(tasks):
    return {t["id"] for t in tasks if t["状态"] == "已完成"}

# ---------- 产物文件与编程语言识别（每语言独立图标由前端渲染） ----------
FILE_RE = re.compile(r"[A-Za-z0-9_\-\u4e00-\u9fff]+\.(?:py|rs|tsx?|jsx?|html?|css|ps1|bat|cmd|toml|json|sh)\b")
EXT_LANG = {"py": "Python", "rs": "Rust", "ts": "TypeScript", "tsx": "TypeScript",
            "js": "JavaScript", "jsx": "JavaScript", "html": "HTML", "css": "CSS",
            "ps1": "PowerShell", "bat": "Batch", "cmd": "Batch", "toml": "TOML",
            "json": "JSON", "sh": "Shell", "md": "Markdown"}
BOOK_LANG = {"CoRun": "Rust", "CGPU": "Rust", "VE": "TypeScript"}
LANG_ORDER = ["TypeScript", "JavaScript", "Python", "Rust", "HTML", "CSS",
              "PowerShell", "Batch", "Shell", "JSON", "TOML", "Markdown"]
# 词边界匹配，防 "rust"→"trust"、"css"→"process" 之类误判
KW_RE = re.compile(r"\b(rust|typescript|javascript|python|powershell|html|css|json|shell|batch)\b", re.I)
KW_CANON = {"rust": "Rust", "typescript": "TypeScript", "javascript": "JavaScript",
            "python": "Python", "powershell": "PowerShell", "html": "HTML", "css": "CSS",
            "json": "JSON", "shell": "Shell", "batch": "Batch"}
# Varix 内核线索：命中即把 Rust 提前（内核/驱动/引导/存储类条目必然是内核语言）
KERNEL_RE = re.compile(r"内核|kernel|驱动|driver|引导|boot|AHCI|NVMe|xHCI|中断|中断|页表|调度|syscall|系统调用", re.I)

def infer_langs(t):
    """从标题/域/验收/书路径提取产物文件与语言；册默认语言恒居首（Varix 内核=CoRun/CGPU→Rust、VE→TypeScript）"""
    text = " ".join([t.get("title", ""), t.get("域", ""), t.get("验收", ""), t.get("书路径", "")])
    files = []
    for m in FILE_RE.finditer(text):
        f = m.group(0)
        if f not in files:
            files.append(f)
    default = BOOK_LANG.get(t.get("册", ""))
    langs = [default] if default else []
    for m in KW_RE.finditer(text):
        kw = KW_CANON[m.group(1).lower()]
        if kw not in langs:
            langs.append(kw)
    for f in files:
        lg = EXT_LANG.get(f.rsplit(".", 1)[-1].lower())
        if lg and lg not in langs:
            langs.append(lg)
    if not langs:
        return [], files
    # 内核线索命中 → Rust 恒排最前（即便是 VE 册里引用内核行为的条目也标明主语言归属）
    if "Rust" in langs and KERNEL_RE.search(text):
        langs.remove("Rust"); langs.insert(0, "Rust")
    elif default and default in langs:
        langs.remove(default); langs.insert(0, default)
    rest = [l for l in langs[1:]]
    rest.sort(key=LANG_ORDER.index)
    return [langs[0]] + rest, files

def is_unlocked(t, done):
    return not t.get("前置") or t["前置"] in done

# ---------- 原书详述索引：书路径锚点 → 该条 ~300 字功能详述 ----------
_DETAIL_CACHE = {"key": None, "map": {}}
DETAIL_HEAD = re.compile(r"^#{2,4}\s+([A-Z]{2,12}-[A-Za-z0-9\-]+)\s*[·•]\s*(.+)$")

def _load_detail_map(path):
    """解析一本施工书：### ID · 标题 之后的正文段（到下一个标题为止）"""
    out = {}
    try:
        with open(path, encoding="utf-8") as f:
            lines = f.read().split("\n")
    except OSError:
        return out
    cur, buf = None, []
    for ln in lines:
        hm = DETAIL_HEAD.match(ln)
        if hm:
            if cur:
                out[cur] = "\n".join(buf).strip()
            cur, buf = hm.group(1), []
        elif cur is not None:
            if re.match(r"^#{1,4}\s", ln):          # 下一标题：收束
                out[cur] = "\n".join(buf).strip()
                cur, buf = None, []
            else:
                buf.append(ln)
    if cur:
        out[cur] = "\n".join(buf).strip()
    return out

def book_detail(bookpath, tid):
    """按 书路径#ID 取原书详述段（含 mtime 缓存；多篇书共享一个索引）"""
    p = bookpath.split("#")[0].strip()
    if not p or not os.path.exists(p):
        return ""
    try:
        st = os.stat(p)
        key = (p, st.st_mtime_ns, st.st_size)
    except OSError:
        return ""
    if _DETAIL_CACHE["key"] != key:
        try:
            m = dict(_DETAIL_CACHE["map"])
            m[p] = _load_detail_map(p)
            _DETAIL_CACHE.update(key=key, map=m)
        except OSError:
            pass
    return _DETAIL_CACHE["map"].get(p, {}).get(tid, "")

def snapshot():
    """mtime 缓存：文件没变就不重解析（2 万卡级必备）"""
    global _CACHE
    try:
        st = os.stat(MD_PATH)
        key = (st.st_mtime_ns, st.st_size)
    except OSError:
        key = None
    if _CACHE["key"] != key:
        lines, tasks = parse_md()
        _CACHE.update(key=key, lines=lines, tasks=tasks)
    return _CACHE["lines"], _CACHE["tasks"], [
        {k: v for k, v in t.items() if not k.startswith("_")} for t in _CACHE["tasks"]]

_CACHE = {"key": None, "lines": [], "tasks": []}

class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _json(self, obj, code=200):
        b = json.dumps(obj, ensure_ascii=False).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_HEAD(self):
        """只回响应头、不发body。前端用它探测 4K 母版是否已生成完毕
        （母版是几十 MB 的文件，用 GET 探测等于白下载一遍）。
        语义严格对齐 do_GET 的路由判定：命中且文件在 -> 200 + Content-Length。
        """
        p0 = urllib.parse.urlparse(self.path).path
        if p0.startswith("/m4k/") or p0.startswith("/wehd/"):
            if p0.startswith("/wehd/"):
                fp = os.path.join(_HD_DIR, os.path.basename(p0))
            else:
                rel = urllib.parse.unquote(p0[len("/m4k/"):])
                fp = os.path.join(_M4K_DIR, rel)
                if not os.path.isfile(fp):
                    # ?r=WxH 的母版在缓存里的键含目标尺寸，先按视口物理像素试一次
                    base = os.path.splitext(rel)[0]
                    pid = base.split("_")[0]
                    ent = _m4k_lookup(pid)
                    if ent:
                        from urllib.parse import parse_qs as _pq
                        rq = (_pq(urllib.parse.urlparse(self.path).query)
                              .get("r") or [""])[0]
                        m = re.match(r"^(\d+)x(\d+)$", rq or "")
                        if m:
                            ext = "mp4" if ent[2] == "gif" else "jpg"
                            k2 = _m4k_cache_key(pid, int(m.group(1)), int(m.group(2)),
                                                MASTER_SS, ext)
                            cand = os.path.join(_M4K_DIR, k2)
                            if os.path.isfile(cand):
                                fp = cand
            if fp and os.path.isfile(fp):
                self.send_response(200)
                self.send_header("Content-Type", _mime(fp))
                self.send_header("Content-Length", str(os.path.getsize(fp)))
                self.end_headers()
                return
        self.send_response(404)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self.path.startswith("/api/health"):
            return self._json({"ok": True, "app": "VTaskBoard", "build": 17, "port": PORT, "md": MD_PATH})
        from urllib.parse import urlparse, parse_qs
        q = parse_qs(urlparse(self.path).query)
        with LOCK:
            _, tasks, pub = snapshot()
            if self.path.startswith("/api/task?"):
                tid = q.get("id", [""])[0]
                t = next((x for x in pub if x["id"] == tid), None)
                if t:
                    bk = q.get("book", [""])[0]
                    scope = [x for x in pub if not bk or x["册"] == bk]
                    t = dict(t, 解锁=is_unlocked(t, done_ids(tasks)),
                             index=next(i for i, x in enumerate(scope) if x["id"] == tid),
                             简介=book_detail(t.get("书路径", ""), tid))
                return self._json({"task": t})
            if self.path.startswith("/api/summary"):
                by_status, by_book, workers = {}, {}, {}
                for t in tasks:
                    s, b = t["状态"], t["册"]
                    by_status[s] = by_status.get(s, 0) + 1
                    bk = by_book.setdefault(b, {"待领": 0, "已领": 0, "阻塞": 0, "已完成": 0, "总": 0})
                    bk[s] = bk.get(s, 0) + 1
                    bk["总"] += 1
                    if s == "已领" and t["领取人"]:
                        workers[t["领取人"]] = workers.get(t["领取人"], 0) + 1
                return self._json({"total": len(tasks), "by_status": by_status, "by_book": by_book,
                                   "workers": workers,
                                   "md": MD_PATH, "port": PORT})
            if self.path.startswith("/api/maptasks"):
                # 一次性返回全部域的任务（每域限量），供导图单请求拉齐
                try:
                    lim = min(300, max(1, int(q.get("limit", ["120"])[0])))
                except ValueError:
                    lim = 120
                stf = q.get("status", [""])[0]
                done = done_ids(tasks)
                out = {}
                for t in tasks:
                    if stf and t["状态"] != stf:
                        continue
                    k = t["册"] + "/" + t["域"]
                    lst = out.setdefault(k, [])
                    if len(lst) < lim:
                        if stf:
                            # 状态过滤模式（BOARD 泳道）：只投影板面需要的字段
                            lst.append({"id": t["id"], "title": t["title"], "状态": t["状态"],
                                        "前置": t.get("前置", ""), "解锁": is_unlocked(t, done),
                                        "langs": t.get("langs", [])})
                        else:
                            lst.append({x: v for x, v in t.items() if not x.startswith("_")}
                                       | {"解锁": is_unlocked(t, done)})
                return self._json({"domains": out, "md": MD_PATH, "port": PORT})
            if self.path.startswith("/api/map"):
                # 导图树：册 → 域 {total, done, active, first（域内第一条）}
                order = {}
                for t in tasks:
                    b, d = t["册"], t["域"]
                    o = order.setdefault(b, {})
                    if d not in o:
                        o[d] = {"d": d, "total": 0, "done": 0, "active": 0, "first": t["id"]}
                    n = o[d]
                    n["total"] += 1
                    if t["状态"] == "已完成":
                        n["done"] += 1
                    elif t["状态"] == "已领":
                        n["active"] += 1
                return self._json({"books": {b: list(o.values()) for b, o in order.items()},
                                   "md": MD_PATH, "port": PORT})
            if self.path.startswith("/api/tasks"):
                # 可选 status/book/domain/limit/offset/unlocked 过滤
                done = done_ids(tasks)
                flt = [t for t in pub if is_unlocked(t, done)] if q.get("unlocked") else pub
                for k, key in (("status", "状态"), ("book", "册"), ("domain", "域")):
                    if q.get(k):
                        v = q[k][0]
                        flt = [t for t in flt if t[key] == v]
                try:
                    off = max(0, int(q.get("offset", ["0"])[0]))
                    lim = min(2000, max(1, int(q.get("limit", ["100000"])[0])))
                except ValueError:
                    off, lim = 0, 100000
                page = [dict(t, 解锁=is_unlocked(t, done)) for t in flt[off:off+lim]]
                ndone = sum(1 for t in pub if t["状态"] == "已完成")
                return self._json({"tasks": page, "total": len(flt), "shown": min(lim, len(flt)),
                                   "done": ndone, "md": MD_PATH, "port": PORT})
        # 壁纸库：/api/wallpapers 列表；/wallpapers/<file> 静态服务（4K 图片 / 循环视频）
        p0 = self.path.split("?")[0]
        if p0 == "/we-live.mjpg":
            # WE 运行时取帧 MJPEG 流（捕获线程持续产出）；必须等到**新鲜帧**（连接后新产出），
            # 陈旧帧=捕获线程已死/冻结，直接 404 让前端回退静态预览
            we_live_client_begin()                 # 先激活捕获线程，再等首帧（防互相等死锁）
            t0 = time.time()
            while True:
                with _LIVE_FRAME["lock"]:
                    ft = _LIVE_FRAME["t"]
                if ft > t0 or time.time() - t0 > 6:
                    break
                time.sleep(0.1)
            with _LIVE_FRAME["lock"]:
                frozen = _LIVE_FRAME["stall"] >= 8      # 源冻结（PrintWindow 缓存等）→ 拒流
                fr = _LIVE_FRAME["jpg"] if (_LIVE_FRAME["t"] > t0 and not frozen) else None
            if fr is None:
                we_live_client_end()
                self.send_response(404); self.end_headers(); return
            # begin 已在等帧前调用，此处不再重复计数
            self.send_response(200)
            self.send_header("Content-Type", "multipart/x-mixed-replace; boundary=frame")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            try:
                last = 0.0
                while True:
                    with _LIVE_FRAME["lock"]:
                        fr = _LIVE_FRAME["jpg"]
                    if fr is not None and fr is not last:
                        self.wfile.write(b"--frame\r\nContent-Type: image/jpeg\r\nContent-Length: "
                                         + str(len(fr)).encode() + b"\r\n\r\n" + fr + b"\r\n")
                        self.wfile.flush()
                        last = fr
                    time.sleep(0.1)
            except (BrokenPipeError, ConnectionAbortedError, ConnectionResetError, OSError):
                pass
            finally:
                we_live_client_end()
            return
        if p0 == "/api/we/open":
            # 切换 WE 桌面到指定场景工程（真渲染 scene.pkg）
            pid = q.get("id", [""])[0]
            ok = we_open_scene(pid) if pid else False
            return self._json({"ok": ok})
        if p0 == "/api/wallpapers":
            wdir = os.path.join(APP_DIR, "wallpapers")
            os.makedirs(wdir, exist_ok=True)
            IMG = (".jpg", ".jpeg", ".png", ".webp", ".avif", ".bmp")
            VID = (".mp4", ".webm", ".mkv")
            out = []
            try:
                for fn in sorted(os.listdir(wdir)):
                    lo = fn.lower()
                    if lo.endswith(IMG):
                        # 本地静图同样过 4K 母版管线。分辨率不足时走 AI 超分，
                        # 足够时零重采样直出（native），分辨率标注如实回传前端。
                        fp = os.path.join(wdir, fn)
                        pid = "loc_" + hashlib.md5(fn.encode("utf-8")).hexdigest()[:10]
                        m = _wehd_item(pid, wdir, src_name=fn)
                        if m and m.get("url"):
                            out.append({"name": fn, "type": "live", "url": m["url"],
                                        "fallback": "/wallpapers/" + fn,
                                        "preview": "/wallpapers/" + fn,
                                        "master": "%dx%d" % (m.get("w", 0), m.get("h", 0)),
                                        "sr": m.get("mode") == "sr",
                                        "res": m.get("res"),
                                        "upscale": m.get("upscale"), "mb": m.get("mb")})
                        else:
                            out.append({"name": fn, "type": "img",
                                        "url": "/wallpapers/" + fn})
                    elif lo.endswith(VID):
                        # 本地视频直出：源分辨率 >= 屏物理像素时浏览器/GPU 做 1:1 或降采样，
                        # 已是最佳呈现，服务端再插值只会劣化并白烧带宽。
                        fp = os.path.join(wdir, fn)
                        d = _mp4_dims(fp) if lo.endswith(".mp4") else None
                        out.append({"name": fn, "type": "video",
                                    "url": "/wallpapers/" + fn,
                                    "native": True,
                                    "res": ("%dx%d" % d) if d else None,
                                    "mb": round(os.path.getsize(fp) / 1048576.0, 1)})
            except OSError:
                pass
            return self._json({"wallpapers": out, "dir": wdir})
        if p0.startswith("/wallpapers/"):
            fn = os.path.basename(p0[len("/wallpapers/"):])
            fp = os.path.join(APP_DIR, "wallpapers", fn)
            if os.path.isfile(fp):
                ext = os.path.splitext(fn)[1].lower()
                ct = _mime(fn)
                # Range 支持：大视频按需取段，避免整文件读进内存（与 /we/ 行为对齐）
                size = os.path.getsize(fp)
                rng = self.headers.get("Range")
                if rng and rng.startswith("bytes=") and size:
                    a, _, b = rng[6:].partition("-")
                    try:
                        a = int(a or 0)
                    except ValueError:
                        a = 0
                    b = int(b) if b else size - 1
                    b = min(b, size - 1)
                    if a > b or a >= size:      # 越界 -> 416
                        self.send_response(416)
                        self.send_header("Content-Range", "bytes */%d" % size)
                        self.end_headers()
                        return
                    self.send_response(206)
                    self.send_header("Content-Range", "bytes %d-%d/%d" % (a, b, size))
                    self.send_header("Content-Length", str(b - a + 1))
                    self.send_header("Content-Type", ct)
                    self.send_header("Accept-Ranges", "bytes")
                    self.send_header("Cache-Control", "max-age=86400")
                    self.end_headers()
                    with open(fp, "rb") as f:
                        f.seek(a)
                        self.wfile.write(f.read(b - a + 1))
                    return
                with open(fp, "rb") as f:
                    b = f.read()
                self.send_response(200)
                self.send_header("Content-Type", ct)
                self.send_header("Accept-Ranges", "bytes")
                # 图片/视频本体不重编码：原样透传，浏览器解码即原生画质。
                # 素材文件名可能不变但内容被替换过，故用 mtime+size 做 ETag 弱校验。
                self.send_header("Cache-Control", "max-age=86400")
                self.send_header("ETag", 'W/"%x-%x"' % (int(os.path.getmtime(fp)), size))
                self.send_header("Content-Length", str(len(b)))
                self.end_headers()
                return self.wfile.write(b)
            self.send_response(404); self.end_headers(); return
        # 皮肤贴图资产：/ui/<sub>/<file> 静态服务（wallpapers/ui/ 下，官方 MC GUI 贴图等）
        if p0.startswith("/ui/"):
            root = os.path.join(APP_DIR, "wallpapers", "ui")
            rel = urllib.parse.unquote(p0[len("/ui/"):])
            if not rel or ".." in rel:
                self.send_response(404); self.end_headers(); return
            fp = os.path.normpath(os.path.join(root, rel))
            if not fp.startswith(os.path.normpath(root)) or not os.path.isfile(fp):
                self.send_response(404); self.end_headers(); return
            ext = os.path.splitext(fp)[1].lower()
            # 字体 MIME 必须给对：octet-stream 下浏览器会拒载 @font-face，
            # 表现为定制皮肤字体静默回退系统字体（像素风失效）。见 _MIME。
            ct = _mime(fp)
            with open(fp, "rb") as f:
                b = f.read()
            self.send_response(200)
            self.send_header("Content-Type", ct)
            self.send_header("Cache-Control", "max-age=86400")
            self.send_header("Content-Length", str(len(b)))
            self.end_headers()
            return self.wfile.write(b)
        # Wallpaper Engine 壁纸自动识别：/api/we/list + /we/<proj>/<file> 服务
        if p0 == "/api/we/list":
            return self._json({"items": we_wallpapers(), "root": we_root(), "note":
                "video=原视频循环；web=网页嵌入；gif=官方动态预览循环；live=场景壁纸经 WE 运行时取帧实时镜像（WE 未运行回退静态预览）"})
        if p0.startswith("/m4k/"):
            # 4K 母版缓存服务（AI 超分 / Lanczos 放大产物）。
            # ?r=<w>x<h> 表示前端按当前视口物理比例请求母版：服务端先看该比例的
            # 产物是否已存在，没有就按需重建（超分是秒级开销，磁盘缓存复用）。
            rel = urllib.parse.unquote(p0[len("/m4k/"):])
            from urllib.parse import urlparse as _up, parse_qs as _pq
            rq = (_pq(_up(self.path).query).get("r") or [""])[0]
            base = os.path.splitext(rel)[0]
            pid = base.split("_")[0]
            fn_root = os.path.normpath(os.path.join(_M4K_DIR, rel))
            if not rel or ".." in rel or not fn_root.startswith(os.path.normpath(_M4K_DIR)):
                self.send_response(404); self.end_headers(); return
            fp = fn_root
            if rq:
                m = re.match(r"^(\d+)x(\d+)$", rq)
                if m:
                    rw, rh = int(m.group(1)), int(m.group(2))
                    # 尺寸上限：母版 = 视口物理 x MASTER_SS。若请求尺寸荒谬
                    # （例如前端传了 CSS 逻辑像素的 4 倍、或被篡改），
                    # 算出来的母版会到几万像素、单张吃掉 GB 级内存。
                    dw, dh = _display_px()
                    if rw > dw * MASTER_SS or rh > dh * MASTER_SS:
                        rw, rh = min(rw, dw * MASTER_SS), min(rh, dh * MASTER_SS)
                    if rw > 0 and rh > 0:
                        # 目标键的产物已存在 -> 直接用，零重算（前端宽度对齐
                        # 母版后，正常路径恒命中这里）
                        ext = "mp4" if rel.lower().endswith(".mp4") else "jpg"
                        cand = os.path.join(
                            _M4K_DIR, _m4k_cache_key(pid, rw, rh, MASTER_SS, ext))
                        if os.path.isfile(cand) and os.path.getsize(cand) > 1024:
                            fp = cand
                        elif os.path.isfile(fn_root):
                            # 未命中：先返回现有母版（渐进增强，绝不因重建
                            # 让浏览器等几十秒），后台补建目标比例。
                            _m4k_spawn(lambda: _m4k_rebuild(pid, rw, rh),
                                       "%s@%dx%d" % (pid, rw, rh))
            if not os.path.isfile(fp):
                self.send_response(404); self.end_headers(); return
            mtime = int(os.path.getmtime(fp))
            size = os.path.getsize(fp)
            etag = 'W/"%x-%x"' % (mtime, size)
            if self.headers.get("If-None-Match") == etag:
                self.send_response(304)
                self.send_header("ETag", etag)
                self.end_headers()
                return
            ext = os.path.splitext(fp)[1].lower()
            ct = _mime(fp)
            # Range 支持：母版 MP4 可能十几 MB，浏览器需要能分段取
            rng = self.headers.get("Range")
            if rng and rng.startswith("bytes=") and size:
                a, _, b = rng[6:].partition("-")
                try:
                    a = int(a or 0)
                except ValueError:
                    a = 0
                b = min(int(b) if b else size - 1, size - 1)
                if a > b or a >= size:
                    self.send_response(416)
                    self.send_header("Content-Range", "bytes */%d" % size)
                    self.end_headers()
                    return
                self.send_response(206)
                self.send_header("Content-Type", ct)
                self.send_header("Content-Range", f"bytes {a}-{b}/{size}")
                self.send_header("Content-Length", str(b - a + 1))
                self.send_header("Accept-Ranges", "bytes")
                self.send_header("ETag", etag)
                self.end_headers()
                with open(fp, "rb") as f:
                    f.seek(a)
                    self.wfile.write(f.read(b - a + 1))
                return
            self.send_response(200)
            self.send_header("Content-Type", ct)
            self.send_header("ETag", etag)
            self.send_header("Content-Length", str(size))
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Cache-Control", "max-age=3600")
            self.end_headers()
            with open(fp, "rb") as f:
                self.wfile.write(f.read())
            return
        if p0.startswith("/wetexture/"):
            # scene.pkg 原生素材（2560~4096 宽纹理 / 内嵌 MP4）。
            # 与 /m4k/ 同构：ETag + Range 全支持 —— 内嵌 MP4 实测有 130MB，
            # 浏览器必须能分段取，否则会表现为"一直转圈/未加载"。
            rel = urllib.parse.unquote(p0[len("/wetexture/"):])
            if not rel or ".." in rel:
                self.send_response(404); self.end_headers(); return
            fp = os.path.normpath(os.path.join(_TEX_DIR, rel))
            if not fp.startswith(os.path.normpath(_TEX_DIR)) or not os.path.isfile(fp):
                self.send_response(404); self.end_headers(); return
            mtime = int(os.path.getmtime(fp))
            size = os.path.getsize(fp)
            etag = 'W/"%x-%x"' % (mtime, size)
            if self.headers.get("If-None-Match") == etag:
                self.send_response(304)
                self.send_header("ETag", etag)
                self.end_headers()
                return
            ct = _mime(fp)
            rng = self.headers.get("Range")
            if rng and rng.startswith("bytes=") and size:
                a, _, b = rng[6:].partition("-")
                try:
                    a = int(a or 0)
                except ValueError:
                    a = 0
                b = min(int(b) if b else size - 1, size - 1)
                if a > b or a >= size:
                    self.send_response(416)
                    self.send_header("Content-Range", "bytes */%d" % size)
                    self.end_headers()
                    return
                self.send_response(206)
                self.send_header("Content-Type", ct)
                self.send_header("Content-Range", f"bytes {a}-{b}/{size}")
                self.send_header("Content-Length", str(b - a + 1))
                self.send_header("Accept-Ranges", "bytes")
                self.send_header("ETag", etag)
                self.end_headers()
                with open(fp, "rb") as f:
                    f.seek(a)
                    # 分块传输：130MB 一次性 write 会占满内存且无法提前结束
                    left = b - a + 1
                    while left > 0:
                        chunk = f.read(min(262144, left))
                        if not chunk:
                            break
                        self.wfile.write(chunk)
                        left -= len(chunk)
                return
            self.send_response(200)
            self.send_header("Content-Type", ct)
            self.send_header("ETag", etag)
            self.send_header("Content-Length", str(size))
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Cache-Control", "max-age=3600")
            self.end_headers()
            with open(fp, "rb") as f:
                while True:
                    chunk = f.read(262144)
                    if not chunk:
                        break
                    self.wfile.write(chunk)
            return
        if p0.startswith("/wehd/"):
            rel = urllib.parse.unquote(p0[len("/wehd/"):])
            if not rel or ".." in rel:
                self.send_response(404); self.end_headers(); return
            fp = os.path.normpath(os.path.join(_HD_DIR, rel))
            if not fp.startswith(os.path.normpath(_HD_DIR)) or not os.path.isfile(fp):
                self.send_response(404); self.end_headers(); return
            mtime = int(os.path.getmtime(fp))
            size = os.path.getsize(fp)
            etag = 'W/"%x-%x"' % (mtime, size)
            if self.headers.get("If-None-Match") == etag:
                self.send_response(304)
                self.send_header("ETag", etag)
                self.end_headers()
                return
            self.send_response(200)
            self.send_header("Content-Type", "image/jpeg")
            self.send_header("ETag", etag)
            self.send_header("Content-Length", str(size))
            self.send_header("Cache-Control", "max-age=3600")
            self.end_headers()
            with open(fp, "rb") as f:
                self.wfile.write(f.read())
            return
        if p0.startswith("/we/"):
            root = we_root()
            rel = urllib.parse.unquote(p0[len("/we/"):])
            if not root or not rel or ".." in rel:
                self.send_response(404); self.end_headers(); return
            # _f<base64url> 安全名还原为真实素材名（见 _we_url_safe）
            if "/" in rel:
                head, _, tail = rel.rpartition("/")
                if tail.startswith("_f"):
                    try:
                        import base64 as _b64
                        tail = _b64.urlsafe_b64decode(tail[2:] + "=" * (-len(tail[2:]) % 4)).decode("utf-8")
                        rel = head + "/" + tail.replace("\\", "/")
                    except (ValueError, UnicodeDecodeError):
                        pass
            fp = os.path.normpath(os.path.join(root, rel))
            if not fp.startswith(os.path.normpath(root)) or not os.path.isfile(fp):
                self.send_response(404); self.end_headers(); return
            ext = os.path.splitext(fp)[1].lower()
            ct = _mime(fp)
            # Range 支持大视频拖动/循环
            size = os.path.getsize(fp)
            rng = self.headers.get("Range")
            if rng and rng.startswith("bytes="):
                a, _, b = rng[6:].partition("-")
                a = int(a or 0); b = int(b) if b else size - 1
                b = min(b, size - 1)
                self.send_response(206)
                self.send_header("Content-Range", f"bytes {a}-{b}/{size}")
                self.send_header("Content-Length", str(b - a + 1))
                self.send_header("Content-Type", ct)
                self.send_header("Accept-Ranges", "bytes")
                self.end_headers()
                with open(fp, "rb") as f:
                    f.seek(a); self.wfile.write(f.read(b - a + 1))
                return
            self.send_response(200)
            self.send_header("Content-Type", ct)
            self.send_header("Content-Length", str(size))
            self.send_header("Accept-Ranges", "bytes")
            self.end_headers()
            with open(fp, "rb") as f:
                self.wfile.write(f.read())
            return
        # 其余路径 = ui.html / favicon.ico
        if self.path.split("?")[0] == "/favicon.ico":
            fp = os.path.join(APP_DIR, "favicon.ico")
            if os.path.exists(fp):
                with open(fp, "rb") as f:
                    b = f.read()
                self.send_response(200)
                self.send_header("Content-Type", "image/x-icon")
                self.send_header("Content-Length", str(len(b)))
                self.end_headers()
                return self.wfile.write(b)
        try:
            with open(os.path.join(APP_DIR, "ui.html"), encoding="utf-8") as f:
                html = f.read()
        except FileNotFoundError:
            html = "<h1>VTaskBoard</h1><p>ui.html 缺失</p>"
        b = html.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        try:
            body = json.loads(self.rfile.read(n) or b"{}")
        except Exception:
            return self._json({"ok": False, "err": "bad json"}, 400)
        act = self.path.split("?")[0]
        if act == "/api/we/open":
            from urllib.parse import urlparse as _up, parse_qs as _pq
            _q = _pq(_up(self.path).query)
            pid = _q.get("id", [""])[0]
            ok = we_open_scene(pid) if pid else False
            return self._json({"ok": ok})
        if act == "/api/we/liveview":
            # 实况小窗：DWM 缩略图原生实时预览（任务栏预览同款合成机制）
            on = bool(body.get("on"))
            return self._json({"ok": we_liveview(on)})
        if act == "/api/we/preview":
            # 显式「桌面临时预览」：真渲染 15 分钟后自动还原（不经镜像直接看原生画面）
            from urllib.parse import urlparse as _up, parse_qs as _pq
            _q = _pq(_up(self.path).query)
            pid = _q.get("id", [""])[0]
            ok = we_open_scene(pid) if pid else False
            if ok:
                with _PREVIEW_TIMER["lock"]:
                    if _PREVIEW_TIMER["t"]:
                        _PREVIEW_TIMER["t"].cancel()
                    _PREVIEW_TIMER["t"] = threading.Timer(900, lambda: we_restore_desktop())
                    _PREVIEW_TIMER["t"].daemon = True
                    _PREVIEW_TIMER["t"].start()
            return self._json({"ok": ok})
        with LOCK:
            lines, tasks, _ = snapshot()
            tid = body.get("id")
            t = next((x for x in tasks if x["id"] == tid), None) if tid else None
            if act == "/api/claim":
                done = done_ids(tasks)
                if t:  # 指名领单
                    if t["状态"] != "待领":
                        return self._json({"ok": False, "err": "已被人领取", "status": t["状态"]}, 409)
                    if not is_unlocked(t, done):
                        return self._json({"ok": False, "err": f"前置未完成: {t['前置']}（先完成前置才解锁）"}, 423)
                    t["状态"], t["领取人"] = "已领", body.get("worker", "AI-?")
                    t["时间"] = time.strftime("%m-%d %H:%M")
                else:  # 自动领第一张已解锁待领
                    t = next((x for x in tasks if x["状态"] == "待领" and is_unlocked(x, done)), None)
                    if not t:
                        return self._json({"ok": False, "err": "队列空或全部前置未完成，无可领单"})
                    t["状态"], t["领取人"] = "已领", body.get("worker", "AI-?")
                    t["时间"] = time.strftime("%m-%d %H:%M")
                write_back(lines, tasks)
                return self._json({"ok": True, "task": {k: v for k, v in t.items() if not k.startswith("_")}})
            if act == "/api/release_worker":
                # 取消 AI 施工：把某领取人（或全部）名下已领单全部退回待领
                w = body.get("worker", "")
                freed = []
                for x in tasks:
                    if x["状态"] == "已领" and (w == "" or x["领取人"] == w):
                        freed.append(f"{x['id']}({x['领取人']})")
                        x["状态"], x["领取人"], x["结果"], x["时间"] = "待领", "", "", ""
                if not freed:
                    return self._json({"ok": False, "err": "该领取人名下没有在施工的单"})
                write_back(lines, tasks)
                return self._json({"ok": True, "freed": freed, "n": len(freed)})
            if not t:
                return self._json({"ok": False, "err": "no such id"}, 404)
            if act == "/api/complete":
                if t["状态"] not in ("已领", "阻塞"):
                    return self._json({"ok": False, "err": "状态不允许完成"}, 409)
                if body.get("worker") and body["worker"] != t["领取人"]:
                    return self._json({"ok": False, "err": "非本单领取人"}, 403)
                t["状态"], t["结果"] = "已完成", body.get("result", "")[:200]
                t["时间"] = time.strftime("%m-%d %H:%M")
            elif act == "/api/block":
                t["状态"], t["结果"] = "阻塞", body.get("reason", "")[:200]
                t["时间"] = time.strftime("%m-%d %H:%M")
            elif act == "/api/release":
                t["状态"], t["领取人"], t["结果"], t["时间"] = "待领", "", "", ""
            else:
                return self._json({"ok": False, "err": "unknown action"}, 404)
            write_back(lines, tasks)
        return self._json({"ok": True, "task": {k: v for k, v in t.items() if not k.startswith("_")}})

def open_window():
    """只用独立 App 窗口（Edge --app）打开，不开浏览器标签"""
    url = f"http://127.0.0.1:{PORT}"
    cands = [
        os.path.expandvars(r"%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe"),
        os.path.expandvars(r"%ProgramFiles%\Microsoft\Edge\Application\msedge.exe"),
        os.path.expandvars(r"%LocalAppData%\Google\Chrome\Application\chrome.exe"),
    ]
    for exe in cands:
        if os.path.exists(exe):
            subprocess.Popen([exe, f"--app={url}", "--window-size=1500,940", "--start-fullscreen",
                              "--disable-features=Translate"])
            return
    webbrowser.open(url)  # 兜底

def main():
    if not os.path.exists(MD_PATH):
        sys.exit("taskboard.md 不在程序同目录：" + APP_DIR)
    srv = ThreadingHTTPServer(("127.0.0.1", PORT), H)
    print(f"VTaskBoard on http://127.0.0.1:{PORT}  |  任务册: {MD_PATH}")
    threading.Timer(0.8, open_window).start()
    srv.serve_forever()

if __name__ == "__main__":
    main()
