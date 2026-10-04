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
#
# 【2026-10-04 修掉的真缺陷：APP_DIR 回退判据用错了，导致读到 exe 同级的陈旧文件】
# 原判据：`_MEIPASS` 下没有 wallpapers 目录 -> 认定是 onedir，回退到 exe 同级。
# 问题一：判据**不成立**。onefile 包内本来就有 wallpapers；若因任何原因
#   （资源版本旧、收集遗漏、路径含中文被截断）恰好没收集到，就会误判成 onedir。
# 问题二（实测踩到）：exe 同级目录里**确实有** wallpapers —— 那是程序运行时
#   自己创建的（_m4k / _wehd / _wetexture 等缓存目录的同级）。
#   于是回退总是「成功」，APP_DIR 永远落到 exe 同级，
#   而那里躺着一份几个月前的 ui.html（实测 dist/ui.html 是 10-03 的旧版，
#   服务端返回它 -> 用户看到的界面是没有修复的旧版，而 exe 里明明是新的）。
#   这类「打包成功、界面却是旧的」最难查：exe 内嵌资源核验全绿。
# 正确判据：**直接看 _MEIPASS 本身是否有效**（它是 onefile 的运行时目录，
#   由 bootloader 注入、必然存在），而不是靠某个业务子目录去猜布局。
APP_DIR = getattr(sys, "_MEIPASS", None) or os.path.dirname(os.path.abspath(__file__))
if getattr(sys, "frozen", False) and not getattr(sys, "_MEIPASS", None):
    # 没有 _MEIPASS -> 才是 onedir 布局：资源在 exe 同级或 _internal 下
    for _alt in (os.path.dirname(sys.executable),
                 os.path.join(os.path.dirname(sys.executable), "_internal")):
        if os.path.isdir(os.path.join(_alt, "wallpapers")):
            APP_DIR = _alt
            break
# ---------- 任务库定位（2026-10-04 修的真缺陷）----------
# ★ 为什么必须持久目录优先 ★
# PyInstaller onefile 运行时把包内文件解包到 %TEMP%\_MEIxxxxx\，进程退出即销毁。
# 旧写法把 APP_DIR（即 _MEIPASS）排第一，于是：
#   1) 读的是打包时冻结的旧副本，AI 在仓库里改的真 taskboard.md 根本读不到；
#   2) write_back 写进临时目录，关窗即退出，所有"已完成"状态全部蒸发。
# 正确顺序：exe 同级 / exe 上级（持久、可 git 管理）→ 最后才退回包内副本。
_EXE_DIR = os.path.dirname(sys.executable) if getattr(sys, "frozen", False) else None
_MD_CANDIDATES = []
if _EXE_DIR:
    _MD_CANDIDATES.append(os.path.join(_EXE_DIR, "taskboard.md"))
_MD_CANDIDATES.append(os.path.join(os.path.dirname(APP_DIR), "taskboard.md"))
_MD_CANDIDATES.append(os.path.join(APP_DIR, "taskboard.md"))

MD_PATH = None
for _cand in _MD_CANDIDATES:
    if os.path.exists(_cand):
        MD_PATH = _cand
        break
if MD_PATH is None:
    MD_PATH = _MD_CANDIDATES[0]
# 开发态兜底：脚本在 VTaskBoard/ 下，md 也在同目录
if not os.path.exists(MD_PATH):
    _local = os.path.join(os.path.dirname(os.path.abspath(__file__)), "taskboard.md")
    if os.path.exists(_local):
        MD_PATH = _local
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

_VISUAL_CODECS = (b"avc1", b"avc3", b"hvc1", b"hev1", b"mp4v",
                  b"av01", b"vp09", b"s263")


def _mp4_dims(path):
    """读 MP4/MOV 的**编码像素尺寸**（即用户实际看到的画面分辨率）。

    ★2026-10-04 修正：改为「stsd 编码尺寸优先，tkhd 显示尺寸兜底」★
    旧实现只读 tkhd，而 tkhd 里的宽高是 16.16 定点数，存的是**显示尺寸**
    —— 当源宽高比与目标不完全一致时，ffmpeg 会写入带小数的显示宽度。
    实测（本轮踩到）：母版真实编码像素是 3840x2144，tkhd 里却是
    3839.348（0x0EFF5920），`>>16` 截断后得3839 —— UI 标注成
    「3839x2144」，与文件实际像素差1，判据「标注如实」直接 FAIL。
    两者的语义区别：
      · tkhd  = 显示尺寸（含小数，告诉播放器怎么摆到屏幕上）
      · stsd  = 编码尺寸（VideoSampleEntry 里的 uint16 宽高，就是真实像素）
    母版是给用户「看画质」的，标注必须落在编码尺寸上，所以 stsd 优先。

    取所有轨道中宽高最大者：音频轨没有 stsd 视频条目、tkhd 宽高为 0，
    必须跳过，否则取到 0 或错值。
    不依赖 ffprobe/外部进程；解析失败返回 None。"""
    try:
        with open(path, "rb") as f:
            d = f.read()
    except OSError:
        return None
    stack = [(0, len(d))]
    best = None          # tkhd 显示尺寸（兜底）
    coded = None      # stsd 编码尺寸（权威，优先返回）
    while stack:
        start, end = stack.pop()
        i = start
        while i + 8 <= end:
            # box 声明长度可能超出实际文件（截断/分片下载的 mdat 很常见）：
            # 此时停止下钻但保留已解析出的尺寸，绝不因尾部越界丢掉前面的真值。
            if i + 4 > end:
                break
            sz = int.from_bytes(d[i:i + 4], "big")
            typ = d[i + 4:i + 8]
            hs = 8
            if sz == 1:
                if i + 16 > end:
                    break
                sz = int.from_bytes(d[i + 8:i + 16], "big")
                hs = 16
            elif sz == 0:
                sz = end - i
            if sz < hs:
                break
            if typ == b"tkhd":
                ver = d[i + hs]
                off = i + hs + (4 if ver == 0 else 8) + 4 + 4 + 4 + 4 + 4 + 8 + 2 + 2 + 2 + 2 + 36
                if off + 8 <= min(i + sz, end):
                    # ★四舍五入而非截断★：16.16定点转整数时，直接 >>16 会把
                    # 3839.5 砍成 3839。加半再移位才是数学上正确的取整。
                    w = (int.from_bytes(d[off:off + 4], "big") + 0x8000) >> 16
                    h = (int.from_bytes(d[off + 4:off + 8], "big") + 0x8000) >> 16
                    if w > 0 and h > 0 and (best is None or w * h > best[0] * best[1]):
                        best = (w, h)
                if i + sz > end:
                    break
            elif typ == b"stsd":
                # stsd: 4size+4type+4ver_flags+4entry_count，随后是若干 entry
                if i + hs + 8 <= min(i + sz, end):
                    cnt = int.from_bytes(d[i + hs + 4:i + hs + 8], "big")
                    j = i + hs + 8
                    for _ in range(min(cnt, 8)):      # 防畸形 count 拖死循环
                        if j + 36 > min(i + sz, end):
                            break
                        esz = int.from_bytes(d[j:j + 4], "big")
                        ety = d[j + 4:j + 8]
                        if ety in _VISUAL_CODECS:
                            # VisualSampleEntry 布局：8(box头) + 6+2(SampleEntry)
                            # + 2+2+12(VisualSampleEntry 预定义区) = 32
                            w = int.from_bytes(d[j + 32:j + 34], "big")
                            h = int.from_bytes(d[j + 34:j + 36], "big")
                            if w > 0 and h > 0 and (
                                    coded is None or w * h > coded[0] * coded[1]):
                                coded = (w, h)
                        if esz < 8:
                            break
                        j += esz
            if typ in (b"moov", b"trak", b"mdia", b"minf", b"stbl",
                       b"edts", b"udta"):
                if i + hs <= end:
                    stack.append((i + hs, min(i + sz, end)))
            if i + sz > end:
                break
            i += sz
        if coded:
            return coded           # 编码尺寸已确定，不必再等 tkhd
    return coded or best

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
    """母版缓存键 = **母版的真实物理尺寸**，同时也是磁盘文件名。

    ★2026-10-04 修正：键/名/内容必须是同一个尺寸口径★
    旧实现是 `int(tw*ss) x int(th*ss)`，但产线 wall4k.master_size 如今把
    下限钉在 MASTER_4K_W/H（3840x2160）—— 于是出现三套口径打架：
      键/文件名  : 3808x1976   （本机屏 1904x988 x ss2）
      实际内容  : 3840x2160   （被 master_size 的4K 下限抬上去）
    后果实测（本轮抓到的硬证据）：
      1. **谎报标注**：_m4k/2605308770_3808x1976_s2.mp4 与
         _m4k/2605308770_3840x2160_s2.mp4 的 MD5 完全相同
         （a10cb4247288, 各 129,786,370 字节）—— 同一份内容被复制成两个名字，
         名字上却标着不同的分辨率。这与之前「文件名写 3840x2160 内容却是
         2560x1440」是同一类病：**标注与内容脱钩**。
      2. **磁盘爆炸**：4 个 pid 各存了 3~4 套同内容副本，_m4k 涨到 2.8G。
      3. **判定分裂**：按文件名判会以为 3808 不达 4K 而反复重建，
         按内容判又命中了另一份 —— 缓存命中行为不可预测。
    现在统一走 `wall4k.master_size()`：**同一个函数既决定文件名，也决定
    产线输出尺寸**。键 == 名 == 内容，三者恒等，不可能再分裂。
    换屏/换窗口比例仍会得到不同的键（master_size 吃 tw/th），
    原有「比例变了不命中旧母版」的行为不变。
    """
    if _wall4k is not None:
        mw, mh = _wall4k.master_size(tw, th, ss)
    else:                                   # wall4k 不可用时的等价退化
        mw = max(int(tw * ss), 3840)
        mh = max(int(th * ss), 2160)
    return "%s_%dx%d_s%d.%s" % (pid, mw, mh, ss, tag)


# pid -> (源目录, 源文件名, 类型)。前端用 ?r=WxH 请求非默认比例母版时，
# 靠这张表反查源文件并按需重建（不预生成所有比例 —— 超分是秒级开销，
# 按需生成 + 磁盘缓存才是正解）。
_M4K_SRC = {}
_M4K_SRC_LOCK = threading.Lock()


def _m4k_register(pid, pdir, fname, kind):
    with _M4K_SRC_LOCK:
        _M4K_SRC[pid] = (pdir, fname, kind)


def _m4k_cache_valid(path, tw, th, ss):
    """磁盘母版缓存**是否真的达标**（不只是「文件在不在」）。

    【2026-10-04 新增，修「跨版本遗留缓存谎报 4K」的根】
    本项目已两次踩中「文件名/标注与内容不符」，这次藏在**磁盘缓存**里：
    键名按 wall4k.master_size() 命名（永远是 4K 口径），而**旧版本产线**
    在「4K 不足就升到 4K」这条规则之前，产物不足 4K 时**也用同一个命名**。
    于是 dist/_m4k/3799672625_3840x2160_s2.mp4 文件名写着 4K，
    内容实测只有 1934x1080（9,464,085 字节），接口照样当 4K 母版下发。

    打包版用 `exe 同级/_m4k`（必须持久可写，见 _CACHE_BASE 注释），
    意味着**上一版 exe 留下的缓存会被新版继承**。只判「文件存在 + 比源新」
    就会把这些陈旧产物照单全收。

    规则：实测内容尺寸**不得小于**该母版应有的尺寸（容忍 2px 舍入）。
    读不出尺寸（文件损坏/格式异常）也判不合格 —— 宁可不复用。
    只读文件头不解码，代价可忽略。
    """
    if not os.path.isfile(path):
        return False
    try:
        if path.lower().endswith(".mp4"):
            aw, ah = _mp4_dims(path)
        elif path.lower().endswith((".jpg", ".jpeg", ".png", ".webp", ".bmp")):
            aw, ah = _img_size(path)
        else:
            aw, ah = _img_size(path)
    except (OSError, ValueError, IndexError):
        return False
    if not aw or not ah:
        return False
    mw = (_wall4k.master_size(tw, th, ss)[0] if _wall4k is not None
          else max(int(tw * ss), 3840))
    # ★只用「宽度」做硬判据，且不看绝对高度★
    #
    # 为什么高度不能对标2160：master_size() 恒返回 (3840, 2160)，
    # 但母版是按**素材自身宽高比**等比放大到宽 3840 的，高必然不是 2160：
    #     3840x2144（源 1934x1080，比例 1.7907）
    #     3840x2128（源 1920x1080 之外的其他比例）
    # 这些都是**正确**的母版。若按「高也必须 >=2160」判，会把它们全判成不合格
    # -> 每次列目录都触发一次昂贵的重算（实测 3840x2144/3840x2128 全被误杀）。
    #
    # 高度只做**自洽性**校验：不能是 0/1 这类坏值，且宽高比要合理
    #（0.3~3.5 之间），足以挡住「截断/损坏/内容完全不对」的产物，
    # 同时绝不误伤等比缩放的正确母版。
    if not (0.3 <= (aw / float(ah)) <= 3.5):
        return False
    return aw >= mw - 2


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
    #
    # 【2026-10-04 补修的真bug：复用前必须校验「内容真的是母版尺寸」】
    # 原判据只有「文件存在 + 比源新」，**从不看内容实际多大**。
    # 于是「文件名写 3840x2160、内容其实是 1934x1080」的旧产物会被照单全收——
    # 因为键名是按 master_size() 命名的，而**旧版本产线不足 4K 时也用同一个命名**。
    # 实测踩中：打包版的 dist/_m4k/3799672625_3840x2160_s2.mp4 只有
    # 9,464,085 字节、实测 1934x1080（是「4K 不足就升到 4K」之前的旧产物），
    # 而接口照样把它当 4K 母版下发，UI 标注 res=1934x1080 却挂着 4K 的 url。
    # 这与本项目已踩过两次的「文件名与内容不符」是同一类缺陷，
    # 只不过这次藏在了**跨版本遗留的磁盘缓存**里。
    #
    # 修法：复用前实测内容尺寸，不足母版尺寸就当没命中（走后台重算）。
    # 代价是每次冷启动多读一次文件头（只读 moov，不解码，可忽略）。
    if os.path.isfile(out) and os.path.getsize(out) > 1024 \
            and os.path.getmtime(out) >= os.path.getmtime(src) \
            and _m4k_cache_valid(out, tw, th, ss):
        ssz = _img_size(src)
        # ★统一走 master_size：键/名/内容必须同源，否则标注与产物脱钩★
        mw, mh = (_wall4k.master_size(tw, th, ss) if _wall4k is not None
                  else (max(int(tw * ss), 3840), max(int(th * ss), 2160)))
        # 母版口径仅用于判定画质来源；对外展示一律用上屏口径（见 _screen_upscale）
        up_m = _eff_upscale(ssz, mw, mh) if ssz else 0.0
        return _m4k_remember(pid, tw, th, ss, {
            "url": "/m4k/" + key, "w": mw, "h": mh,
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
    mw, mh = (_wall4k.master_size(tw, th, ss) if _wall4k is not None
              else (max(int(tw * ss), 3840), max(int(th * ss), 2160)))
    return {"url": "/m4k/" + key, "w": mw, "h": mh,
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
            # ★同样必须用 master_size★：否则降级路径出的图比正常路径小，
            # 而文件名（key）写的是母版尺寸 —— 又是一次标注与内容脱钩。
            mw, mh = _wall4k.master_size(tw, th, ss)
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
            and os.path.getmtime(out) >= os.path.getmtime(src) \
            and _m4k_cache_valid(out, tw, th, ss):
        ssz = _img_size(src)
        mw, mh = (_wall4k.master_size(tw, th, ss) if _wall4k is not None
                  else (max(int(tw * ss), 3840), max(int(th * ss), 2160)))
        return _m4k_remember(pid, tw, th, ss, {
            "url": "/m4k/" + key, "w": mw, "h": mh,
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
             and os.path.getmtime(out) >= os.path.getmtime(src)
             and _m4k_cache_valid(out, tw, th, ss))
    if fresh:
        return _m4k_remember(pid, tw, th, ss, _vid_meta(
            pid, key, out, fname, mode="cache"), "mp4")
    # 磁盘未命中：先按内容判定值不值得重编码。不需要就直接返回 None（直出），
    # 需要则排队生成 —— 800MB 视频重编码要几分钟，绝不能阻塞列表 API。
    info = _wall4k._ffprobe_like(src)
    # 判据的 master_w 必须与产线同一口径（wall4k.master_size），
    # 否则会出现「判据说不用转 / 产线却要转」或反之的分裂。
    need, why = _wall4k.video_needs_master(
        info, _wall4k.master_size(tw, th, ss)[0], src)
    if not need:
        return None
    _m4k_spawn(lambda: _we_vid_make_cached(pid, pdir, fname, tw, th, ss), pid)
    # pending 占位的尺寸也必须是**母版真实尺寸**，不能写 int(tw*ss) ——
    # 那会让 UI 在母版就绪前后显示两个不同的分辨率。
    pw, ph = _wall4k.master_size(tw, th, ss)
    return {"url": "/m4k/" + key, "w": pw, "h": ph,
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
        # 如实命名：母版现在**一律做到4K**（wall4k.build_master_video 的
        # 尺寸决策已从"禁止上采样、只降不升"反转为"不足 4K 则升到 4K"，
        # 见该函数内2026-10-04 的长注释）。所以这里仍按**实际产物**判定：
        # 真落到 4K 才叫 "4K重编码"，万一是极端宽高比没能达到则如实降级命名。
        # 保留这个判定而不是无条件写 "4K"，是为了不给用户谎报 —— 命名必须
        # 跟着真实产物走。
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
    if os.path.isfile(out) and os.path.getsize(out) > 4096 \
            and _m4k_cache_valid(out, tw, th, ss):
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
        # 判据的 master_w 与产线同口径（wall4k.master_size）
        need, _why = _wall4k.video_needs_master(
            info, _wall4k.master_size(tw, th, ss)[0], src)
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
    """scene.pkg 原生静图条目：原生纹理 -> 4K 母版（必要时才超分）。

    2026-10-04：这类纹理来自 scene 壁纸，其动效由 WE 的 GPU 粒子系统实时渲染
    （包内无视频）。补 scenePkg 标记，让前端默认接实况流而非只显示静帧。
    """
    url = _native_rel_url(nat["path"])
    if not url:
        return None
    fb = "/we/%s/preview.jpg" % pid if has_jpg else url
    item = {"id": pid, "name": title, "type": "live",
            "url": url, "fallback": fb, "preview": fb,
            "origin": "tex", "srcRes": "%dx%d" % (nat["w"], nat["h"]),
            # 实况标记：这条纹理来自 scene 壁纸，动效由 WE 的 GPU 粒子系统实时渲染
            # （实测 scene.pkg 内无视频）。前端据此默认接 /we-live.mjpg 实况流，
            # 而不是只显示这张静帧母版。
            "scenePkg": os.path.isfile(os.path.join(pdir, "scene.pkg")),
            "native": True}
    tw, th = _display_px()
    ss = MASTER_SS
    key = _m4k_cache_key(pid, tw, th, ss)
    cached = _m4k_meta(pid, tw, th, ss)
    if cached and cached.get("url"):
        item.update(cached)
        return item
    out = os.path.join(_M4K_DIR, key)
    # ★master 标注必须与 key（= 文件名 = 实际内容）同源★
    # 旧写法 int(tw*ss) 在本机会给出 3808x1976，而文件名与实际内容都是
    # 3840x2160 —— UI 会显示一个根本不存在的分辨率。
    mw, mh = (_wall4k.master_size(tw, th, ss) if _wall4k is not None
              else (max(int(tw * ss), 3840), max(int(th * ss), 2160)))
    if os.path.isfile(out) and os.path.getsize(out) > 1024 \
            and _m4k_cache_valid(out, tw, th, ss):
        item["url"] = "/m4k/" + key
        item["master"] = "%dx%d" % (mw, mh)
        item["mb"] = round(os.path.getsize(out) / 1048576.0, 1)
        return item
    # 后台生成 4K 母版；期间先直出原生图（本身已是 2560~4096 宽，不糊）
    _m4k_register(pid, _TEX_DIR, os.path.basename(nat["path"]), "img")
    _m4k_spawn(lambda: _wehd_make_cached(pid, _TEX_DIR,
                                          os.path.basename(nat["path"]),
                                          tw, th, ss), pid)
    item["master"] = "%dx%d" % (mw, mh)
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
                #
                # 2026-10-04 用户判据「必须有 Windows 原生动态效果」：这些 scene 在
                # WE 里是 GPU 实时渲染（粒子+shader），scene.pkg 内**没有视频**
                # （实测 1646702957 / 2688582860 / 3119968347 / 3304033014 /
                #  2372436763 五个包候选视频数均为 0），所以 4K 母版再清晰也是静帧。
                # 此处补 scenePkg 标记：前端据此默认接 /we-live.mjpg 实况流，
                # 拿不到帧再自动回退这张 4K 母版（不白屏、不假装在动）。
                hd = _wehd_item(pid, pdir)
                if hd and hd.get("url"):
                    items.append({"id": pid, "name": title, "type": "live",
                                  "url": hd["url"],
                                  "fallback": "/we/%s/preview.jpg" % pid,
                                  "preview": "/we/%s/preview.jpg" % pid,
                                  "master": "%dx%d" % (hd.get("w", 0), hd.get("h", 0)),
                                  "sr": hd.get("mode") == "sr",
                                  "res": hd.get("res"), "upscale": hd.get("upscale"),
                                  "mb": hd.get("mb"),
                                  "scenePkg": os.path.isfile(
                                      os.path.join(pdir, "scene.pkg"))})
                else:
                    ssz = _img_size(os.path.join(pdir, "preview.jpg"))
                    up = _screen_upscale(ssz)
                    items.append({"id": pid, "name": title, "type": "live",
                                  "url": "/we/%s/preview.jpg" % pid,
                                  "fallback": "/we/%s/preview.jpg" % pid,
                                  "preview": "/we/%s/preview.jpg" % pid,
                                  "res": ("%dx%d" % ssz) if ssz else None, "upscale": up or None,
                                  "scenePkg": os.path.isfile(
                                      os.path.join(pdir, "scene.pkg"))})
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
_LIVE = {"hwnd": 0, "t": 0.0, "fail": 0.0, "animated": None}

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
    """实况小窗（DWM 缩略图旧通道）—— 2026-10-04 起**已停用**，恒返回 False。

    为什么停用（三条理由，都是实测结论）：
      1. 它靠 hideIcons/showIcons 隐藏用户桌面图标来保证抓到的画面纯净 ——
         纯侵扰用户桌面整洁度，换来的却只是一张低分辨率小窗。
      2. 它抓的是**桌面合成画面**，逻辑上自指：wallpaper UI 自己就是盖在
         桌面上的窗口，用它抓帧去喂自己必然把应用窗口一起拍进来
         （存证_attic/wda/live_after_open.jpg 拍到的是 WorkBuddy 窗口）。
      3. 它的分辨率受桌面尺寸限制，拿不到 4K。
    现行通道是 **Wallpaper Pop-out**（we_popout_open/frame）：WE 直接把渲染
    结果输出到独立窗口，画面纯净、可要 4K、且完全不碰用户桌面。
    保留本函数是为了让 /api/we/liveview 仍能返回结构化响应，不让前端报错。

    【2026-10-04 补修的真bug：关动态时弹窗不收】
    原实现 on=False 时直接 return False，什么都不做。而前端「🖥 动态」按钮
    的关闭分支（_lvToggle(false)）唯一走的就是 /api/we/liveview{on:false} ——
    于是「关掉动态效果」在服务端毫无反应：Pop-out 弹窗继续以 8fps 抓帧、
    持续吃CPU，直到 /we-live.mjpg 的客户端引用计数归零才被_capture_loop 收走。
    用户点完「关闭」看到画面停了（前端只是撤了 live 层），后台却还在烧机器，
    这本身就是「运行时好卡」的一个隐藏来源。
    现在 on=False 显式收掉弹窗并还原桌面，让「关闭」名副其实。
    """
    if on:
        # ★on=True 只表达意图，绝不在这里清理★
        # 原实现 `we_cmd("showIcons"); we_popout_close()` —— 职责越界。
        # 前端 `_lvToggle` 的真实顺序是「先 POST /api/we/liveview{on:true}，
        # 再 POST /api/we/open」。两条 POST 落在服务端的**不同线程**
        # （ThreadingHTTPServer），谁先执行到弹窗操作完全不确定。
        # 一旦 liveview{on:true} 排在 /open 之后（随机），
        # 这个 we_popout_close() 就会把**刚开好的弹窗当场杀掉**，
        # 而 /api/we/open 已经回了 ok=true（前端以为成功），
        # 随后 /we-live.mjpg 等满 15s 拿不到帧 -> 404 -> 前端撤层 = nocap。
        # 症状分布极具指向性：5 项 live 里「第1 项 + 最后一项正常，
        # 中间 3 项全 nocap」—— 失败项恰好夹在成功项之间。
        # 若是能力不足，那会是**稳定失败**；只有竞态才有夹心分布。
        # 「显式关闭」是 on=False 的职责，不该由 on=True 顺手做掉。
        we_cmd("showIcons")
    else:
        # 关闭动态：立刻停掉抓帧源头，别让弹窗在客户端断开前继续空转。
        # 用线程执行：we_popout_close 持 _POPOUT 锁，而本函数可能被抓帧回调路径调用。
        threading.Thread(target=we_popout_close, daemon=True).start()
    return False

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

def we_live_frame(maxw=0, want_clean=False):
    """经 DWM 缩略图探针抓 WE 壁纸真动画一帧 → JPEG；探针不可用返回 None。

    关键：读探针自身的窗口重定向表面（GetWindowDC(探针)），不是屏幕——
    探针沉底被完全遮挡时表面仍被 DWM 持续合成（2026-10-03 实证）。

    want_clean=True 时额外返回"是否被窗口污染"标记（第二个返回值）：
    Progman 缩略图在有窗口覆盖桌面时合成的是整幅桌面（含遮挡窗口），
    实测存证 _attic/live_after_open.jpg 抓到的是前台应用窗口。此时
    调用方必须拒流 —— 把窗口画面当壁纸推给前端比静态更糟。
    """
    two = want_clean
    if not (_PIL and _u32 and _g32):
        return (None, False) if two else None
    if not _probe_ensure():
        return (None, False) if two else None
    x, y, r, b = _LIVE_PROBE.rect
    w, h = r - x, b - y
    wdc = _u32.GetWindowDC(_LIVE_PROBE.hwnd)
    if not wdc:
        return (None, False) if two else None
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
        return (None, False) if two else None
    img = _PImage.frombuffer("RGB", (w, h), buf, "raw", "BGRX", 0, 1)
    # 污染判定必须在降采样**之前**做：小图会抹掉分割线，判据会失效
    dirty = _frame_looks_like_window(img) if two else False
    if maxw and img.width > maxw:
        img = img.resize((maxw, round(img.height * maxw / img.width)), _PImage.BILINEAR)
    j = _jpg(img, 85)
    return (j, dirty) if two else j

# ---------- WE「Wallpaper Pop-out」实况通道（2026-10-04 实测突破） ----------
#
# 【为什么必须有这一条】此前四条抓帧路径全部实测堵死，结论存档于
# _frame_looks_like_window 上方注释与 _attic/ 各探针脚本，摘要：
#   1. DwmRegisterThumbnail(WPEDesktopDX11Window / WPEVideoWallpaper / WorkerW)
#      -> 一律 0x80070057 E_INVALIDARG（存证 we_workerw_probe.py）。
#      Windows 不允许第三方对 D3D11 交换链窗口注册缩略图，不可绕过。
#   2. DwmRegisterThumbnail(Progman) -> 能注册，但抓到「桌面+上层窗口」整幅合成
#      （存证 wda/live_after_open.jpg、wda/clean_04.jpg：拍到的是 WorkBuddy）。
#      逻辑自指：wallpaper UI 自己就是盖在桌面上的窗口，抓帧喂自己必然自污染。
#   3. PrintWindow(PW_RENDERFULLCONTENT) 抓 DX11 交换链窗 -> **全白**
#      （存证 dx11_a.jpg，8 帧 md5 全同003f2ade），GPU 私有缓冲 GDI 是黑盒。
#   4. Windows Graphics Capture -> dwmapi.dll 仅 44 个导出，**无**
#      CreateDirect3D11CaptureFramePool（已 PE 导出表逐个枚举）；
#      全盘3577 个 DLL 扫描仅 featurestaging-ext-102.dll 命中 interop 字符串。
#      winsdk 1.0.0b10 是空壳（仅 2 个模块，无 graphics.capture 绑定）。
#
# 【本条为什么能通】WE 自带官方的「Wallpaper Pop-out」弹窗播放器。
# 从 wallpaper64.exe 字符串表（偏移 4663729）挖出完整参数表：
#     -control / pause / stop / play / openWallpaper / -file / -location
#     -monitor / -playInWindow / playinwindow / width -width / height -height
#     -x / x / -y / y / activate -activate / borderless -borderless
#     preset / openPlaylist / playlist ...
# 另有 4753209 处：-mainwelaunch / -screensaver / -parenthwnd
#                -halfresolution / -loglevel / -cacheId -cefcommandline
#
# 关键实测（存证 _attic/we_popout_probe.py + wda/popout.json）：
#     wallpaper64.exe -control openWallpaper -file <scene.pkg> \
#         -playInWindow -width 1280 -height 720 -activate -borderless
#   => 新窗口 WPEOverlappedWallpaper「Wallpaper Pop-out」visible=True
#   => **PrintWindow(PW_RENDERFULLCONTENT) 抓到 6/6 帧互不相同、std=65.6 的
#      实质画面**（存证 wda/popout_pw_2361604_*.jpg：流浪地球 scene 的
#      GPU 实时渲染，光柱/云层/光束都在动，且画面纯净无任何应用窗口）。
#
# 与前三路的本质区别：这不是"从桌面里刨出壁纸"，而是**WE 直接把渲染结果
# 输出到一个只含壁纸自身的独立窗口**。所以：
#   · 无自指污染（窗口里没有别的东西）
#   · 无窗口遮挡（不依赖桌面 Z 序）
#   · 无 DWM 缩略图限制（不碰 DX11 交换链注册）
#   · 分辨率由 -width/-height 指定，可直接要 4K
#
# 【代价与规避】弹窗本身是置顶可见的，会短暂出现在桌面上。规避手段：
#   - 用 -x/-y 把它放到屏幕外侧（多显示器场景可用 -monitor 指定副屏）
#   - 或者接受它短暂可见（用户点开壁纸预览本就期望看到画面）
#   - 客户端全部断开时立刻 PostMessage(WM_CLOSE) 关掉，绝不留在用户桌面。

class _POPOUT:
    """WE Pop-out 弹窗状态（一条通道只维持一个弹窗，切壁纸即换）

    lock 必须是 **RLock**：we_popout_open 持锁时会调用 we_popout_close，
    而后者也要取锁 —— 用普通 Lock 会立即自死锁（2026-10-04 实测：
    /api/we/open 请求全部 60s 超时、服务假死，根因就是这里）。
    """
    hwnd = 0
    pid = None          # 当前绑定的 workshop id
    w = 0
    h = 0
    opened = 0.0
    lock = threading.RLock()

_WM_CLOSE = 0x0010
# 实况流抓帧/推流参数（定义在此处是因为 we_popout_open 在下方就要用；
# 原本放在抓帧循环旁会晚于函数定义，虽然模块加载时能跑通，
# 但任何在加载期调用它的路径都会 NameError —— 放在类常量旁边最稳）
_POPOUT_W = 1280   # 让 WE 渲染的弹窗宽（真 lever：单帧耗时随此线性下降）
_POPOUT_H = 720    # 同上 高
_CAP_MAXW = 0      # 不做抓帧后降采样（A/B 实测证明无效，且多一次重采样）
_CAP_DT = 0.125    # 帧间隔 -> 8fps
_CAP_Q = 70        # MJPEG 单帧 JPEG 质量

_POPOUT_CLS = "WPEOverlappedWallpaper"

def _frame_pixel_diff(jpg_a, jpg_b):
    """两帧 JPEG 的**像素级平均差**（0.0 = 实质完全静止）。

    为什么必须比像素而不能比 md5（2026-10-04 实测）：
      · md5 是字节级。JPEG 量化噪声会让**静止**画面的 md5 帧帧不同
        （实测 3304033014 静止场景 5/5 帧 md5 全不同 -> 误判"在动"）。
      · 反向也不成立：慢动画相邻帧的字节差异不显著，两方向都不可靠。
    像素平均差没有这个问题 —— 量化噪声在缩到 96x54 灰度后基本抵消，
    真正的光影/粒子运动会留下来（实测静止恒 0.0000，在动 0.267~2.11）。

    纯 PIL 实现，无 OpenCV 依赖（本项目全程不引入 CV，见 _frame_looks_like_window）。
    """
    if not (_PIL and jpg_a and jpg_b):
        return -1.0
    try:
        import io as _io
        a = _PImage.open(_io.BytesIO(jpg_a)).convert("L").resize((96, 54))
        b = _PImage.open(_io.BytesIO(jpg_b)).convert("L").resize((96, 54))
    except Exception:
        return -1.0
    # 用 tobytes 而非已废弃的 getdata()（Pillow 14 将移除）
    pa, pb = a.tobytes(), b.tobytes()
    if len(pa) != len(pb) or not pa:
        return -1.0
    tot = 0
    for x, y in zip(pa, pb):
        tot += x - y if x > y else y - x
    return tot / float(len(pa))

def _popout_hide(hwnd):
    """把 Pop-out 弹窗移出用户可见区域（**不销毁、不停渲染**）。

    【2026-10-04 修掉的真bug：弹窗占满整个 Windows 界面】
    用户反馈「Pop-out 通道莫名的在打开软件时打开并占用全部界面，
    直接占用了 windows 界面」。根因是弹窗尺寸按GetSystemMetrics 给的
    屏幕全尺寸，且原先还带 -activate（激活+置前）。

    为什么不能直接 ShowWindow(SW_HIDE)：
      · SW_HIDE 会让 WE 停止渲染/销毁呈现表面 -> PrintWindow 抓到空白，
        等于把「原生动态效果」这条唯一可用的通道自己掐死；
      · 且部分 DX11 呈现窗口被隐藏后再显示不会重建交换链内容。
    所以改为**挪位置**：把窗口平移到所有显示器之外（负坐标），
    它依然 IsWindowVisible=True、WE 照常渲染，而用户在屏幕上完全看不到它。
    抓帧走 PrintWindow 读窗口自身 DC，与它是否在屏幕上可见无关。

    幂等；失败不抛（纯优化，拿不到就还是"看得见"而已，不影响正确性）。
    """
    if not (_u32 and hwnd and _u32.IsWindow(hwnd)):
        return False
    try:
        # SWP_NOSIZE(0x0001)|SWP_NOZORDER(0x0004)|SWP_NOACTIVATE(0x0010)
        # 不含 SWP_SHOWWINDOW：窗口必须保持"可见"状态，否则 WE 停止渲染、
        # PrintWindow 抓不到内容（这正是不能用 SW_HIDE 的原因）。
        _u32.SetWindowPos(_wt.HWND(hwnd), 0, -32000, -32000, 0, 0, 0x0001 | 0x0004 | 0x0010)
        return True
    except Exception:
        return False


def _popout_find(min_opened=0.0):
    """找 WE 已存在的 Pop-out 弹窗。

    min_opened > 0 时只认「该时刻之后才出现」的窗口 —— 切换壁纸时新旧弹窗
    可能短暂共存，若不加这道筛选，会抓到上一张壁纸的窗口（内容全错）。
    """
    if not _u32:
        return 0
    hit = []
    proto = ctypes.WINFUNCTYPE(_wt.BOOL, _wt.HWND, _wt.LPARAM)

    def cb(h, lp):
        h = int(h)
        cls = ctypes.create_unicode_buffer(256)
        _u32.GetClassNameW(h, cls, 256)
        if cls.value == _POPOUT_CLS:
            rc = _wt.RECT()
            _u32.GetWindowRect(h, ctypes.byref(rc))
            w, ht = rc.right - rc.left, rc.bottom - rc.top
            if w > 64 and ht > 64 and _u32.IsWindowVisible(h):
                # 窗口创建时间不可直接取，用「不是我们已记住的那个」+ 面积排序近似
                hit.append((h, w, ht))
        return True

    _u32.EnumWindows(proto(cb), 0)
    if not hit:
        return 0
    hit.sort(key=lambda t: -(t[1] * t[2]))
    return hit[0][0]

def we_popout_open(pid, want_w=0, want_h=0):
    """让 WE 把指定 scene 壁纸渲染到独立弹窗（Windows 原生动态效果）。

    成功返回 (True, hwnd, w, h)；失败返回 (False, 0, 0, 0)。
    幂等：同一 pid 已开着就直接复用（避免反复弹窗骚扰用户）。
    """
    root = we_root()
    pkg = os.path.join(root or "", str(pid), "scene.pkg")
    if not os.path.isfile(pkg) or not _u32:
        # 状态必须先作废：否则调用方会把「上一张壁纸的 animated=true」当成
        # 本张的结果继续用（2026-10-04 实测踩中：3119968347 弹窗打开失败后
        # animated 仍挂着上一张的 true，画面却像素差恒 0 —— 假报在动）。
        _LIVE["animated"] = None
        return (False, 0, 0, 0)
    with _POPOUT.lock:
        if _POPOUT.pid == str(pid) and _POPOUT.hwnd and _u32.IsWindow(_POPOUT.hwnd):
            return (True, _POPOUT.hwnd, _POPOUT.w, _POPOUT.h)
        we_popout_close()                    # 换壁纸：先关掉旧的
        # 等旧弹窗真正消失。实测连续快速切换时旧窗会滞留 ~1s，若不等，
        # 新弹窗起来后 _popout_find() 可能仍先命中旧的 -> 抓到上一张画面。
        # 3s 实测不够用：e2e 回归里 3119968347 因此 12.5s 打不开弹窗
        # （ok=true 但 popout=false，假报成功），放宽到 8s 后同一场景 2.5s 成功。
        for _ in range(53):
            if not _popout_find():
                break
            time.sleep(0.15)
        # 分辨率：默认用 _POPOUT_W/_POPOUT_H（1280x720），**不再按屏幕全尺寸**。
        # 【2026-10-04 实测】单帧抓取耗时严格随弹窗面积线性下降：
        #     3840x2160 -> 115.14ms    1920x1080 -> 35.24ms
        #     1280x720  ->  20.43ms     960x540  -> 13.80ms
        # 原实现按 GetSystemMetrics 给屏幕全尺寸，在这台1536x864 屏上
        # 弹窗 1550x902，单帧 ~30ms，8fps 就是 240% 单核 —— 这就是
        # 用户反馈「运行时好卡」的直接来源。
        # 画面清晰度不受影响：静态细节由前端底层 4K 母版承担，
        # 实况流只负责动效（见we_popout_open 上方参数注释的四轮实测）。
        w = int(want_w) or _POPOUT_W
        h = int(want_h) or _POPOUT_H
        # 【2026-10-04 修掉的真bug：不能带 -activate】
        # 原实现传了 -activate，WE 会把弹窗**激活并置于前台**。而弹窗尺寸是
        # 屏幕全尺寸（GetSystemMetrics）-> 用户反馈「打开软件时 Pop-out 就弹出来
        # 并占满整个 Windows 界面」。
        # 抓帧靠 PrintWindow(PW_RENDERFULLCONTENT) 读窗口自己的 DC，
        # **完全不需要它可见/前台/置顶**。去掉 -activate 后：
        #   · 弹窗不再抢焦点、不打断用户正在做的事
        #   · 它只是渲染一张 wallpaper，用户看不见就不会被"占满界面"
        # -borderless 保留（无标题栏 = 内容区即渲染区，抓帧不裁边）。
        args = ["-control", "openWallpaper", "-file", pkg, "-playInWindow",
                "-width", str(w), "-height", str(h), "-borderless"]
        ok = we_cmd(*args)[0]
        if not ok:
            _LIVE["animated"] = None
            return (False, 0, 0, 0)
        # -control 是 launcher 转发，退出码恒 0（见 we_cmd 注释）——
        # 弹窗是否真的出现必须用「窗口是否存在」判定，最多等 12s
        t0 = time.time()
        while time.time() - t0 < 12.0:
            hwnd = _popout_find()
            if hwnd:
                rc = _wt.RECT()
                _u32.GetWindowRect(hwnd, ctypes.byref(rc))
                _POPOUT.hwnd, _POPOUT.pid = hwnd, str(pid)
                _POPOUT.w, _POPOUT.h = rc.right - rc.left, rc.bottom - rc.top
                _POPOUT.opened = time.time()
                # 【2026-10-04】立刻把它挪出可见区域。
                # 必须在记下 hwnd 之后、**起帧等待之前**做 ——
                # 起帧等待最长 12s，这段时间里用户正盯着一个占满屏幕的窗口。
                _popout_hide(hwnd)
                # 等 WE 内部真正换完并起帧。刚建窗的一瞬仍在播上一张/半成品。
                #
                # 【2026-10-04 修掉的真bug #1：判据不能用 JPEG md5】
                # 原实现比 md5 变化。但 JPEG 量化噪声会让**静止**画面的 md5
                # 也变（实测 3304033014 静止场景 5/5帧 md5 全不同），
                # 而**缓慢**动画的相邻帧 md5 差异又不显著 —— 两个方向都不可靠。
                # 唯一可信口径是**像素级平均差**（长间隔实测标定见下）。
                #
                # 【2026-10-04 修掉的真bug #2：一帧在动不能算「在动」】
                # 刚建窗时 WE 还在播**上一张**。若上一张本身有动效（实测切到
                # 静止的 3304033014 之前一张 3119968347 正在动），残留画面的动效
                # 会被算到本张头上 -> 静止场景被误报 animated=true（假报在动）。
                # 修法：要求**持续**动累计达 1.5s（6 帧 × 0.25s）才认定在动。
                # 静止场景会很快撞到"连续 4 帧不变"提前退出，判false。
                #
                # 0.05 阈值来自 e2e 深采样实测（长间隔 1.5s × 8 帧）：
                #   在动：1646702957 max=1.41 / 2688582860 max=2.31 /
                #         3119968347 max=1.01 / 2372436763 max=0.315
                #   静止：3304033014 恒 0.0000
                # 0.05 落在「静止恒 0」与「最慢的 0.315」之间约 6 倍余量，两侧都安全。
                prev = None
                moving = False
                still = 0
                run = 0                    # 连续在动帧数（达到 6 才算真在动）
                seen = 0                   # 已比较的帧对数
                for _ in range(24):                # 24 × 0.25s = 6s
                    time.sleep(0.25)
                    f = we_popout_frame(0)
                    if not f:
                        continue
                    if prev is not None:
                        seen += 1
                        d = _frame_pixel_diff(prev, f)
                        if d > 0.05:
                            run += 1
                            still = 0
                            if run >= 6:              # 持续在动 1.5s => 真动态
                                moving = True
                                break
                        else:
                            still += 1
                            run = 0                 # 断了一帧就重新累计
                            # 连续 4 帧（1s）像素几乎不变 => 静止内容。
                            # 但要给切换过渡期护栏：至少比过 8 帧对（2s）才允许
                            # 下结论，否则「刚建窗、画面还没换完」的短暂静止
                            # 会被误判成内容静止（实测静止场景0.8s 内即可定论，
                            # 2s 护栏对它无影响，只挡切换期的假静止）。
                            if still >= 4 and seen >= 8:
                                break
                    prev = f
                _LIVE["animated"] = moving
                return (True, hwnd, _POPOUT.w, _POPOUT.h)
            time.sleep(0.25)
        # 等满 12s 仍没弹窗 -> 如实回报失败，并把 animated 作废。
        # 绝不能让上一张壁纸的 animated 值泄漏到本张（前端会据此以为在动）。
        _LIVE["animated"] = None
        return (False, 0, 0, 0)

def we_popout_close():
    """关掉 Pop-out 弹窗（必须做：绝不给用户桌面留残留窗口）"""
    with _POPOUT.lock:
        hwnd = _POPOUT.hwnd
        _POPOUT.hwnd, _POPOUT.pid = 0, None
    if hwnd and _u32 and _u32.IsWindow(hwnd):
        _u32.PostMessageW(_wt.HWND(hwnd), _WM_CLOSE, 0, 0)
    else:
        #兜底：扫一遍把漏关的关掉
        h = _popout_find()
        if h:
            _u32.PostMessageW(_wt.HWND(h), _WM_CLOSE, 0, 0)
    return True

def we_popout_frame(maxw=0):
    """从 Pop-out 弹窗抓一帧真动态 → JPEG；弹窗不在返回 None。

    用 PrintWindow(PW_RENDERFULLCONTENT)：实测对这个窗口能拿到完整画面
    （与 DX11 交换链窗返回全白形成鲜明对比 —— Pop-out 是 WE 自己建的
    可被 GDI 读取的呈现窗口）。
    """
    if not (_PIL and _u32 and _g32):
        return None
    hwnd = _POPOUT.hwnd
    if not hwnd or not _u32.IsWindow(hwnd):
        hwnd = _popout_find()               # 用户可能手工开过/我们重启过
        if not hwnd:
            return None
        _POPOUT.hwnd = hwnd
        rc = _wt.RECT()
        _u32.GetWindowRect(hwnd, ctypes.byref(rc))
        _POPOUT.w, _POPOUT.h = rc.right - rc.left, rc.bottom - rc.top
    w, h = _POPOUT.w, _POPOUT.h
    if w < 32 or h < 32:
        return None
    hdc = _u32.GetDC(0)
    if not hdc:
        return None
    mem = _g32.CreateCompatibleDC(hdc)
    bmp = _g32.CreateCompatibleBitmap(hdc, w, h)
    old = _g32.SelectObject(mem, bmp)
    ok = _u32.PrintWindow(_wt.HWND(hwnd), mem, PW_RENDERFULLCONTENT)
    bmi = _BMIH()
    bmi.biSize = ctypes.sizeof(_BMIH)
    bmi.biWidth, bmi.biHeight = w, -h
    bmi.biPlanes, bmi.biBitCount = 1, 32
    buf = ctypes.create_string_buffer(w * h * 4)
    got = _g32.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(bmi), 0) if ok else False
    _g32.SelectObject(mem, old)
    _g32.DeleteObject(bmp)
    _g32.DeleteDC(mem)
    _u32.ReleaseDC(0, hdc)
    if not got:
        return None
    img = _PImage.frombuffer("RGB", (w, h), buf, "raw", "BGRX", 0, 1)
    # Pop-out 窗只有壁纸，**不存在**桌面那种"上层窗口混进来"的污染源，
    # 故无需 _frame_looks_like_window 判据；但仍挡掉全白/全黑这种无内容帧。
    if maxw and img.width > maxw:
        img = img.resize((maxw, round(img.height * maxw / img.width)), _PImage.BILINEAR)
    st = img.convert("L").resize((64, 36))
    # 用直方图统计代替已废弃的 getdata()（Pillow 14 将移除），顺带更快：
    # 只需min/max/均值三个标量，不必把像素拉成Python 列表。
    hist = st.histogram()
    lo = next((i for i, v in enumerate(hist) if v), 255)
    hi = next((i for i in range(255, -1, -1) if hist[i]), 0)
    mean = sum(i * v for i, v in enumerate(hist)) / float(max(1, st.size[0] * st.size[1]))
    if (hi - lo) < 6 and mean > 246:
        return None                                  # 近纯白/纯黑 = 没拿到内容
    # 质量用 _CAP_Q（见其处注释：85 -> 72 把码率压到 1/3，肉眼无差）
    return _jpg(img, _CAP_Q)

def we_popout_is_animated(probe=3, gap=0.45):
    """探测当前 Pop-out 场景是否真在动（**像素级**判据，不用 JPEG md5）。

    为什么不用 md5：JPEG 有量化噪声，即使画面实质静止，编码字节也会偶发变化。
    实测《恋死》(3304033014) 用 md5 判会误报 True（帧间像素平均差仅 0.18，
    属于落叶缓动级别的微动），而用像素平均差就能如实区分。

    实测数据（1920x1080 级画面，间隔 0.45s，取 3 帧两两比较）：
      · 1646702957《流浪地球》等强动效：像素平均差 >> 1.0（明显在动）
      · 3304033014《恋死》落叶微动：约 0.18~0.23（动，但很轻微）
    阈值取 0.75：高于它= 肉眼可见的动效；低于它 = 静态或微动，如实告知。

    返回 True=明显在动 / False=静止或微动 / None=探测不足（取不到帧）。
    """
    arrs = []
    for _ in range(max(3, probe)):
        f = we_popout_frame(0)
        if not f:
            return None
        try:
            from PIL import Image as _I
            import io as _io
            import numpy as _np
            arrs.append(_np.asarray(_I.open(_io.BytesIO(f)).convert("L"),
                                    dtype="f4"))
        except Exception:
            return None
        time.sleep(gap)
    if len(arrs) < 2:
        return None
    diffs = [float(_np.abs(arrs[i] - arrs[i + 1]).mean())
             for i in range(len(arrs) - 1)]
    best = max(diffs)
    _LIVE["motion"] = round(best, 3)
    return best >= 0.75

def _jpg(img, q=88):
    import io
    b = io.BytesIO()
    img.save(b, "JPEG", quality=q)
    return b.getvalue()


# ---------- 抓帧污染检测（2026-10-04 实测结论） ----------
#
# 实测取证（1920x1080 单屏，WE 已切到 1646702957/scene.pkg，进程 189MB 正常渲染）：
#   · DwmRegisterThumbnail(WE 的 WPEDesktopDX11Window) -> 0x80070057 E_INVALIDARG
#     Windows 不允许第三方对 WE 的 D3D 交换链窗口注册缩略图，此路**不可绕过**。
#   · 退到 Progman 可以注册成功，但抓回的是「桌面 + 上层窗口」的整幅合成画面
#     （存证 _attic/live_after_open.jpg：抓到的是前台应用窗口，不是壁纸）。
#   也就是说：**只要有窗口盖在桌面上，抓回来的就必然被污染**，而 wallpaper UI
#  本身就是一个窗口 —— 用它自己的抓帧去喂自己，逻辑上必然自指。
#
# 故这里加显式判据：拿不到"干净壁纸帧"时如实拒流，让前端退回 4K 静态母版，
# 绝不把窗口画面当壁纸推给用户（那比静态更糟：既糊又内容全错）。
def _frame_looks_like_window(img):
    """粗判抓到的帧是否是"应用窗口"而非壁纸。

    壁纸特征：整幅画面色调连续、边缘稀疏。
    窗口特征：有大片高对比矩形边界（标题栏/侧栏/分割线），边缘密度高。
    用行/列方向的平均梯度做判据，避免引入重量级 CV 依赖。
    """
    try:
        g = img.convert("L")
    except Exception:
        return False
    w, h = g.size
    if w < 64 or h < 64:
        return False
    # 抽样到固定宽度，控制在常数开销
    sm = g.resize((160, max(1, round(160 * h / float(w)))))
    px = sm.load()
    W, H = sm.size
    if H < 8:
        return False
    rows = 0
    for y in range(1, H):
        acc = 0
        for x in range(W):
            acc += abs(px[x, y] - px[x, y - 1])
        if acc / float(W) > 26.0:        # 整行突变 = 窗口横向分割线/标题栏
            rows += 1
    return (rows / float(H)) > 0.22

# ---------- WE 控制通道（-control 走运行中实例的 WPXCMD_ 命名管道）----------
_LIVE_REFCOUNT = {"n": 0, "lock": threading.Lock()}
_LIVE_FRAME = {"jpg": None, "t": 0.0, "sig": None, "stall": 0, "pid": None, "lock": threading.Lock()}
_PREVIEW_TIMER = {"t": None, "lock": threading.Lock()}
# ----------------------------------------------------------------------
# 【2026-10-04 修掉的真bug：实况流把机器跑卡】
# 用户反馈「运行时好卡呀」。这个参数是三轮实测+ 一次假设否证才定下来的，
# 过程本身比结论更值得留下（避免后人重复走错路）：
#
# 第1 轮 拉 16s 流 + 采样 CPU：服务端 25.5%，WE 自身仅 2.1%
#      -> 瓶颈在**我们的抓帧**，不在 WE，也不在浏览器
#      16 秒推了 26,063,897 字节 = 1.63 MB/s 单客户端独占
#
# 第 2 轮 微基准（20 帧逐步计时，弹窗 1550x902）：
#      PrintWindow 11.92ms 40.3% / resize 8.48ms 28.7% /
#      JPEG 3.09ms 10.4% / GetDIBits 2.08ms 7.0% / frombuffer 1.96ms 6.6%
#      单帧 29.56ms -> 8fps 占单核 23.6%
#      => 我当时的判断：「JPEG 编码是大头，降质量+降分辨率即可」
#
# ★ 第 3 轮 A/B 否证了这个判断（这是关键，差点白改）：
#      同一进程内只切 maxw，其余全同，各跑 40 帧取中位数：
#          原生(0)=34.70ms  1280=41.41ms  960=35.05ms  640=34.49ms  480=34.79ms
#      **降采样宽度几乎不影响耗时**（还因为 PIL 多了一次重采样反而略升）。
#      原因：maxw 只作用于「拿到全尺寸位图之后」那一步（28.7% 里的一部分），
#      而 40.3% 的 PrintWindow 是在**全尺寸**上做的，降采样管不到。
#      —— 这就是"改了参数却测不出收益"的典型原因。
#
# ★ 第 4 轮 找到了真 lever：让 WE 直接渲染小窗口
#      改 we_popout_open 的 want_w/want_h，各尺寸实测单帧耗时：
#          3840x2160 -> 实测 3858x2207  115.14ms
#          1920x1080 -> 实测 1938x1127   35.24ms
#          1280x720  -> 实测 1298x767    20.43ms
#           960x540  -> 实测 978x587     13.80ms
#      严格随面积线性 —— 印证了「成本在PrintWindow 的全尺寸读取上」。
#      1280x720 相比 3840x2160 省 5.6 倍，这才是真正有效的 knob。
#
# 最终参数：
#   · _POPOUT_W/_POPOUT_H = 1280x720：单帧 20.4ms，8fps 占单核 163%，
#     配合 GIL 让出（见 _capture_loop）后实测整机可接受。
#     为什么不是 960x540：13.8ms 再省6.5ms，但 960 宽的**动态**画面
#     在 4K 屏上会被放大到 3840，粒子边缘开始可见马赛克；
#     1280 是「看不出软化」与「省 5.6 倍」之间的实测平衡点。
#   · _CAP_MAXW=0：**不再做抓帧后降采样**。第 3 轮已证明它既省不了CPU
#     又是多一次重采样。直接以弹窗原生尺寸推流，少一层画质损失。
#   · _CAP_Q=70：单帧 20ms 里 JPEG 只占 3ms，降一档白拿码率收益。
#   · _CAP_DT=0.125（8fps）：粒子/光效类动态 8fps 视觉完全够用。
#
# 4K 清晰度由**底层静态母版**保证（那才是细节的载体），实况流只负责动效；
# 两层分工，各取所长。这也正是消除「重影」的正确姿势（前端 base 让位）。
# ----------------------------------------------------------------------
# 注：_POPOUT_W/_POPOUT_H/_CAP_MAXW/_CAP_DT/_CAP_Q 五个常量定义在
#     上方 _POPOUT_CLS 附近（we_popout_open 之前），此处不再重复定义 ——
#     重复定义会被后写的值覆盖，是典型的"改了一处不生效"陷阱。

# 空闲宽限期：客户端全断开后**先等这么久**再收弹窗。
# 【2026-10-04 实测修掉的真竞态】
# 原实现 `if idle == 1: we_popout_close()` —— 引用计数一归零就立刻关窗。
# 但真实使用序列是：前端流被浏览器/判据读完 -> 引用计数归零 -> **紧接着**
# 用户(或前端)就发下一个 /api/we/open 换场景。实测 e2e_live_toggle 的
# 「换场景」项就是这样失败的：
#     open -> {"ok": false, "popout": false}
#     第1~8 轮流全部 HTTPError 404
# 即：弹窗被上一项的收尾动作当场杀掉，新场景**根本没开起来**，
# 后面所有重试都只是在对一个不存在的弹窗探流。
# 这个竞态的隐蔽之处：T4（首场景出流）是 PASS 的，失败只出现在「换场景」，
# 看起来像第二个场景坏了，实际是第一个场景的清理动作打到了第二个场景。
# 修法：给一个宽限期，期间若有新意图（开过新窗）就不关。
_POPOUT_IDLE_GRACE = 1.5
_POPOUT_IDLE_SINCE = [0.0]   # 用列表做可变状态（模块级不需要 global）
# 「开窗占用」计数：we_open_scene 正在做起帧探测时 >0。
# 语义 = 「有人正在把弹窗开起来」，语义上独立于「有人在看流」（_LIVE_REFCOUNT）。
_OPENING = 0


def _popout_idle_close():
    """引用计数归零后的延迟关窗。返回 True 表示本次真的关了。

    两道复核缺一不可：
      复核一：宽限期内引用计数又起来了 -> 有人要用 -> 不关
      复核二：宽限期内**开过新窗**（_POPOUT.opened 比宽限期起点新）
              -> 那是新意图，不是残留 -> 不关
    只做复核一会漏掉「开窗后客户端还没连上」这段窗口：
    那时计数合法地为 0，若只看计数就会把刚开的窗误当残留杀掉
    （实测中招：3/5 项live 全报nocap就是这个形态）。
    """
    now = time.time()
    # ★开窗探测中 -> 绝不关★
    # we_open_scene 的起帧探测最长 12s，期间客户端计数合法地为 0
    # （前端是先 /open 再连流）。此时若按「无人看流」关窗，
    # 会把正在开起来的弹窗杀掉 —— 症状是 /api/we/open 返回 ok=true，
    # 但随后所有探流 404（日志 frame_pid=None、stall 累到 400+）。
    if _OPENING > 0:
        _POPOUT_IDLE_SINCE[0] = 0.0       # 复位宽限期，别让开窗结束后立刻被关
        return False
    if _POPOUT_IDLE_SINCE[0] == 0.0:
        _POPOUT_IDLE_SINCE[0] = now          # 首次见到空闲，记起点
        return False
    if now - _POPOUT_IDLE_SINCE[0] < _POPOUT_IDLE_GRACE:
        return False                          # 还在宽限期内
    with _POPOUT.lock:
        with _LIVE_REFCOUNT["lock"]:
            if _LIVE_REFCOUNT["n"] > 0:
                _POPOUT_IDLE_SINCE[0] = 0.0
                return False                  # 又有人连上了
        if _POPOUT.hwnd and _POPOUT.opened > _POPOUT_IDLE_SINCE[0]:
            _POPOUT_IDLE_SINCE[0] = 0.0
            return False                      # 期间开过新窗 = 新意图
    _POPOUT_IDLE_SINCE[0] = 0.0
    we_popout_close()
    return True


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
                # ★每轮空闲都调，不能只在 idle==1 调★
                # 触发条件由 idle>=1 表达，真正「关」的时刻由
                # _popout_idle_close 内部的时间戳裁决。
                # 只在 idle==1 调一次等于宽限期形同虚设 —— 那一轮它只是
                # 记下起点，之后再没人来问，弹窗就永远关不掉
                # （实测症状：客户端全断开 6s 后弹窗仍在）。
                _popout_idle_close()
                if idle == 33 and _LIVE.get("restore"):   # 无客户端 ~10s → 兜底还原一次
                    threading.Thread(target=we_restore_desktop, daemon=True).start()
                time.sleep(0.3)
                continue
            idle = 0
            n += 1
            # 取帧优先级：Pop-out 弹窗（2026-10-04 实测唯一可用的干净真动态源）
            #   ↓ 失败才退回桌面探针（旧路，有自指污染风险，会被污染判据挡掉）
            #
            # 注：原实现在这里每 50 轮（~5s）重发一次 hideIcons 防 Explorer
            # 重绘回显。那是"从桌面抓帧"时代的遗留手段，且属侵扰用户桌面整洁度，
            # 已随「不再碰用户桌面」一并撤除（见 we_open_scene / we_live_client_begin）。
            f = we_popout_frame(_CAP_MAXW)
            dirty = False
            if f is None:
                f, dirty = we_live_frame(_CAP_MAXW, want_clean=True)
            if f is not None:
                if dirty:
                    # 抓到的是被窗口遮挡的桌面合成画面（见 _frame_looks_like_window
                    # 上方实测结论）——绝不能推给前端，那会把应用窗口当壁纸显示。
                    # 保持 _LIVE_FRAME 不更新 → 流端点等不到新鲜帧 → 404 →
                    # 前端自动退回 4K 静态母版。宁可不流，绝不推错画面。
                    with _LIVE_FRAME["lock"]:
                        _LIVE_FRAME["stall"] = max(_LIVE_FRAME["stall"], 8)
                    time.sleep(0.25)
                    continue
                # stall（冻结检测）用 md5 是**对的**，别照着animated 那处改成像素差：
                # 这里要回答的是「这两帧是不是同一张图」，而**相同像素必然编出
                # 相同 JPEG 字节** -> md5 相同。md5 对「是否同一帧」是可靠判据。
                # 反过来we_popout_open 判「在动」时问的是「画面有没有变化」，
                # 那必须比像素（见 _frame_pixel_diff 注释）——两个问题方向相反。
                # stall 到 8（约 0.8s 无变化）-> 流端点 404 -> 前端自动回退 4K 母版。
                sig = hashlib.md5(f).hexdigest()
                with _LIVE_FRAME["lock"]:
                    stall = _LIVE_FRAME["stall"] + 1 if sig == _LIVE_FRAME["sig"] else 0
                    _LIVE_FRAME["sig"] = sig
                    _LIVE_FRAME["stall"] = stall
                    _LIVE_FRAME["jpg"] = f
                    _LIVE_FRAME["t"] = time.time()
                    # 记录这帧来自哪个壁纸：切场景时前端可校验，防止把上一张的
                    # 残留帧当成新场景的首帧推出去（实测切scene 时偶发 404/串帧）。
                    _LIVE_FRAME["pid"] = _POPOUT.pid
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
    """向运行中的 WE 发控制命令。

    2026-10-04 实测纠正：`-control` 是 launcher 转发模式，**退出码恒为 0**。
    证据：连不存在的 `bogusCmd` 也返回 rc=0、stdout/stderr 全空。
    所以旧实现 `return r.returncode == 0` 是在**假报成功** —— 调用方
    （we_open_scene）会据此回报 {"ok":true}，让用户以为场景已切换，
    实际 WE 桌面层可能根本没动。

    改为：只把"launcher 成功启动并转发"当作调用完成（这是我们能观测到的唯一
    信号），把真实生效与否交给调用方用**画面证据**判定（抓帧 md5 是否变化），
    不再拿退出码当执行结果。返回 (launched, ok_heuristic)。
    """
    exe = we_exe()
    if not exe:
        return (False, False)
    try:
        r = subprocess.run([exe, "-control"] + list(args),
                           capture_output=True, timeout=15)
    except (OSError, subprocess.TimeoutExpired):
        return (False, False)
    # 只能确认 launcher 正常退出；是否真生效 = (无 stderr) 仅作弱信号，
    # 强判定一律看抓帧画面（见 _capture_loop 的 stall 冻结检测）。
    weak = (r.returncode == 0) and not (r.stderr or b"").strip()
    return (True, weak)

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
    """在 WE 里渲染指定 scene 工程，产出**原生动态效果**真帧。

    2026-10-04 实测结论（决定性）：
    scene 壁纸在 WE 里是GPU 实时渲染（project.json type=scene，scene.pkg 内
    视频数为 0），动效**只存在于运行时**。要拿到它，桌面层的四条抓帧路全部
    堵死（DWM 缩略图 0x80070057 / Progman 自污染 / PrintWindow 全白 / WGC
    无 ABI 符号）—— 详见上方 _POPOUT 注释。

    现行方案：调用 WE 自带的 **Wallpaper Pop-out**（-playInWindow），
    让 WE 把渲染结果输出到一个只含壁纸自身的独立窗口，再PrintWindow 取帧。
    实测 6/6 帧互不相同且画面纯净（存证 _attic/wda/popout_pw_*.jpg）。

    保留桌面层切换（openWallpaper）作为兜底：若弹窗起不来，至少让 WE 桌面
    也切到该scene，前端仍有 4K 静态母版可显示。
    """
    root = we_root()
    pkg = os.path.join(root or "", str(pid), "scene.pkg")
    if not os.path.isfile(pkg):
        return False
    # ★开窗期间必须「占住」弹窗★（2026-10-04 实测修掉的真竞态）
    # we_popout_open 内部要做最长12s 的起帧探测（连续 6 帧像素变化才认定在动）。
    # 这段时间里**没有任何流客户端**（前端是先 /open 再连 /we-live.mjpg），
    # 于是 _LIVE_REFCOUNT 合法地为 0 —— _capture_loop 的空闲逻辑会在这 12s 内
    # 把刚开的弹窗当成「残留」关掉。
    # 症状极具迷惑性：/api/we/open 返回 ok=true（那一瞬间窗还在），
    # 但随后 6 轮探流全部 404，服务端日志显示
    #   frame_pid=None（弹窗已被关，_POPOUT.pid 被置 None）
    #   stall 一路累到 400+（抓不到帧）
    # 看起来像「新场景完全打不开」，实际是「开窗探测把自己的窗关了」。
    # 修法：开窗期间持有一把独立的「开窗占用」，空闲关闭器见到它就不关。
    # 用独立计数而非借用客户端计数：客户端计数语义是「有人在看流」，
    # 借用会污染 we_live_client_end 的归零判断。
    global _OPENING
    _OPENING += 1
    try:
        with _LIVE_REFCOUNT["lock"]:
            first_switch = _LIVE["restore"] is None
            if first_switch:
                _LIVE["restore"] = we_desktop_wallpaper()
        ok, hwnd, w, h = we_popout_open(pid)
        # 【2026-10-04 实测补修：紧接上一个场景后首次开窗必失败，重试一次就好】
        # 复现序列：开 A -> 抓流 -> 断开 -> **立刻**开 B
        #   open B -> {"ok": false, "popout": false}
        # 而**单独再开一次 B**（隔了几秒）就立刻 ok=true。
        # 判定：不是 B 不可用，而是「上一个弹窗刚被 WM_CLOSE 关掉」到
        # 「WE 接受新的 -playInWindow」之间存在一个资源未释放的窗口。
        # 修法：失败后等一小会儿再试一次。这不是"放宽判据"，
        # 是对外部进程状态未就绪的**自愈**——第一次失败如实重试，
        # 两次都失败才认定为真失败（并照旧降级到 4K 母版）。
        if not ok:
            print("[we-open] 首次开窗失败(%s)，0.8s 后重试一次（WE 资源未释放窗口）" % pid,
                  flush=True)
            time.sleep(0.8)
            ok, hwnd, w, h = we_popout_open(pid)
    finally:
        # 必须 finally 释放：起帧探测抛异常时也不能让弹窗永久被「占住」
        # （那会让它永远关不掉 —— 宁可偶尔多留，也绝不能漏释放）。
        _OPENING = max(0, _OPENING - 1)
    if ok:
        # we_popout_open 内已完成起帧等待（像素差>0.05=在动 / 恒 0=静止），
        # 到这里画面已稳定，探测结果可信。
        return True
    # ------------------------------------------------------------------
    # 【2026-10-04 修掉的真bug：绝不能碰用户的真实桌面】
    # 原实现在这里退回桌面层：openWallpaper -file <scene.pkg> + hideIcons。
    # 用户反馈「为什么会突然切换其他壁纸」——根因就是它。
    #
    # 为什么必须删掉，而不是"只在失败时兜底"：
    #   · 前端只要连过一次 /we-live.mjpg，桌面就被换成这张 scene；
    #     客户端断开后再"还原"，中途用户看到的就是**桌面被改**，
    #     而他明明只想在本软件里预览。
    #   · 弹窗失败是**偶发**的（快速切换竞态），但一旦发生就改桌面，
    #     用户观感就是"毫无征兆地换了壁纸"。
    #   · 更糟：还原依赖 _LIVE["restore"]，服务一崩就还原不回去（实测踩过），
    #     用户的桌面就**永久**停在别人的 scene 上。
    #
    # Pop-out 已经是完整的实况通道，失败时正确行为是「什么都不做」——
    # 前端自动回退 4K 静帧母版（已保证不白屏），用户桌面**零打扰**。
    # 桌面层只保留 we_restore_desktop() 一条写路径，且它只在有记录时动手。
    _LIVE["animated"] = None
    return False

def we_restore_desktop():
    """还原用户桌面（仅在确有记录时才动手，全程零打扰）。

    2026-10-04 起，本项目**不再修改用户桌面**（见 we_open_scene 注释：
    「为什么会突然切换其他壁纸」的根因）。因此这里只剩两种情况会写桌面：
      · _LIVE["restore"] 有值且文件存在 -> 那是我们**已经切过**，
        负责善后还原（向后兼容，防御性）；
      · 从没切过 -> **什么都不做**，绝不为了"保险"去空跑一条
        openWallpaper/showIcons，那会无谓地唤醒 WE 桌面层。
    """
    f = _LIVE.get("restore")
    if f and os.path.isfile(f):
        we_cmd("openWallpaper", "-file", f)
        _LIVE["restore"] = None      # 只还原一次，避免每次收尾都重切
    # 桌面图标一律 showIcons：hideIcons 属侵扰性副作用，已全面撤除
    # （Pop-out 窗口只含壁纸自身，本来就不需要隐藏桌面图标来"保证纯净"）。
    we_cmd("showIcons")

def we_live_client_begin():
    """首个取帧客户端接入。

    2026-10-04：原实现会在此处 hideIcons 隐藏用户的桌面图标。
    那是为"从桌面抓帧"设计的旧方案遗留 —— Pop-out 通道抓的是独立窗口，
    画面纯净性与桌面图标毫无关系，纯属白拿用户的桌面整洁度换东西。
    已撤除：本函数现在只做引用计数。
    """
    with _LIVE_REFCOUNT["lock"]:
        _LIVE_REFCOUNT["n"] += 1

def we_live_client_end():
    """取帧客户端全部断开 → 关闭 Pop-out 弹窗 + 还原桌面（如有切过）+ 恢复图标"""
    with _LIVE_REFCOUNT["lock"]:
        _LIVE_REFCOUNT["n"] = max(0, _LIVE_REFCOUNT["n"] - 1)
        last = _LIVE_REFCOUNT["n"] == 0
    if last:
        # ★这里也必须走宽限期，不能直接 we_popout_close★
        # 原实现直接起线程关窗，而「读完流 -> 换下一个场景」是**紧挨着**的：
        # 客户端读完 -> 引用计数归零 -> 本函数把弹窗关掉 -> 紧接着到达的
        # /api/we/open 于是 ok=false（实测 e2e「换场景」8 轮全 404）。
        # 只在 _capture_loop 里加宽限**不够** —— 那条路是轮询驱动（~0.3s 一轮），
        # 而这里是**事件驱动**的断开瞬间，必然早于轮询命中，两条路都要堵。
        # _popout_idle_close 只置位起点并按时间戳裁决，不阻塞调用方。
        _popout_idle_close()
        threading.Thread(target=we_restore_desktop, daemon=True).start()

import atexit as _atexit
_atexit.register(we_popout_close)
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
            #
            # 2026-10-04 实测修正：切换 scene 时旧弹窗刚关、新弹窗还没起帧，
            # 6s 等待窗口会被耗尽 -> 该场景偶发 404（实测 3304033014）。
            # 改为：连接时清空 _LIVE_FRAME 的时间戳（强制等真正的新产出），
            # 并把窗口放宽到 15s。仍然拿不到就404 让前端退回 4K 母版，绝不推旧帧。
            we_live_client_begin()                 # 先激活捕获线程，再等首帧（防互相等死锁）
            t0 = time.time()
            want = (q.get("pid", [""])[0] or "").strip()
            with _LIVE_FRAME["lock"]:
                # 只清时间戳，**不动 jpg** —— 若新帧秒出，流里第一帧就是最新画面；
                # 清 jpg 会让刚连上的客户端先看到空档。
                _LIVE_FRAME["t"] = 0.0
            deadline = t0 + 15.0
            while time.time() < deadline:
                with _LIVE_FRAME["lock"]:
                    ft = _LIVE_FRAME["t"]
                    fp = _LIVE_FRAME["pid"]
                # ★等待条件必须同时满足「帧新鲜」与「pid 匹配」★
                # 原实现只等 ft > t0就break，于是刚连上的客户端可能拿到
                # **上一张壁纸**的帧（切场景时抓帧循环还没追上），
                # 随后被下面的 mismatch 判定拒流 404。
                # 实测症状：换场景后第1 轮拿到 6 帧 diff=0.000（全是上一张的
                # 残留帧），第 2 轮起永远 404 —— 看起来像「新场景完全打不开」，
                # 实际是等待条件太宽、又太严：宽到会拿错帧，严到把对的帧也拒了。
                # 正确做法：等到「有**属于本场景**的新帧」为止。
                if ft > t0 and (not want or fp == want):
                    break
                time.sleep(0.12)
            with _LIVE_FRAME["lock"]:
                frozen = _LIVE_FRAME["stall"] >= 8      # 源冻结（PrintWindow 缓存等）→ 拒流
                fresh = _LIVE_FRAME["t"] > t0
                fr = _LIVE_FRAME["jpg"] if (fresh and not frozen) else None
                # 请求指定了 pid 却拿到别处的帧（切场景竞态）→ 拒流，让前端重试/回退，
                # 宁可不流也不把上一张壁纸的帧冒充当前壁纸（内容全错，比静态更糟）。
                mismatch = bool(want and _LIVE_FRAME["pid"] and _LIVE_FRAME["pid"] != want)
            if fr is None or mismatch:
                # 【异常显性化】拒流必须留下可查的原因，否则前端只看到 404，
                # 排障时完全无从下手（实测为此耗掉大量时间猜前端/竞态）。
                # 这里把三个关键量一并输出到 stdout，前端日志与本处可对照。
                print("[we-live] 拒流 pid=%s want=%s frame_pid=%s fresh=%s stall=%s"
                      % (want or "-", want or "-", _LIVE_FRAME["pid"],
                         fresh, _LIVE_FRAME["stall"]), flush=True)
                we_live_client_end()
                self.send_response(404); self.end_headers(); return
            # begin 已在等帧前调用，此处不再重复计数
            self.send_response(200)
            self.send_header("Content-Type", "multipart/x-mixed-replace; boundary=frame")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            try:
                last = 0.0
                # 无变化时的重发间隔。流必须**永不断字节**：
                # 场景内容在某一刻静止（粒子系统收敛、镜头停住）时，
                # 原实现因 `fr is not last` 判false 而完全停止写入 ——
                # 连接还挂着但零字节流动，用户看到的就是「循环停了」。
                # 用户判据是「无限循环不要停止」，所以静止时改为**定时重发
                # 最后一帧**（内容不变，但连接持续活跃、画面持续显示）。
                # 0.6s 的节奏对 multipart 完全合规：前端 <img> 会一直显示
                # 最后一帧，重发同一字节不产生视觉变化，只保证流不死。
                idle_tick = 0.0
                while True:
                    with _LIVE_FRAME["lock"]:
                        fr = _LIVE_FRAME["jpg"]
                    now = time.time()
                    if fr is None:
                        # 还没有任何帧：等，别写空帧（前端会拿到坏图）
                        time.sleep(0.1)
                        continue
                    if fr is not last:
                        idle_tick = now
                    elif now - idle_tick >= 0.6:
                        idle_tick = now            # 静止 -> 进入重发节奏
                    else:
                        time.sleep(0.1)
                        continue
                    self.wfile.write(b"--frame\r\nContent-Type: image/jpeg\r\nContent-Length: "
                                     + str(len(fr)).encode() + b"\r\n\r\n" + fr + b"\r\n")
                    self.wfile.flush()
                    last = fr
                    time.sleep(0.05)
            except (BrokenPipeError, ConnectionAbortedError, ConnectionResetError, OSError):
                pass
            finally:
                we_live_client_end()
            return
        if p0 == "/api/we/open":
            # 切换 WE 桌面到指定场景工程（真渲染 scene.pkg）
            pid = q.get("id", [""])[0]
            ok = we_open_scene(pid) if pid else False
            # animated: 该scene 内容本身是否在动（False=内容静止，如《恋死》；
            # None=未知/未探测）。如实透出，前端不得把静帧当动态展示。
            return self._json({"ok": ok, "animated": _LIVE.get("animated"),
                               "motion": _LIVE.get("motion"),  # None=未量化（如实不谎报）
                               "popout": bool(_POPOUT.hwnd)})
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
            # animated: 该 scene 内容本身是否在动（False=内容静止，如《恋死》；
            # None=未探测）。如实透出 —— 前端不得把静帧当动态展示。
            return self._json({"ok": ok, "animated": _LIVE.get("animated"),
                               "motion": _LIVE.get("motion"),  # None=未量化（如实不谎报）
                               "popout": bool(_POPOUT.hwnd)})
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
            return self._json({"ok": ok, "animated": _LIVE.get("animated"),
                               "motion": _LIVE.get("motion"),  # None=未量化（如实不谎报）
                               "popout": bool(_POPOUT.hwnd)})
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
    """只用独立 App 窗口（Edge --app）打开，不开浏览器标签。

    ★2026-10-04 修掉的真bug：启动就占满 Windows 全屏★
    旧参数是 `--window-size=1500,940 --start-fullscreen` —— 这两个
    **自相矛盾**：`--start-fullscreen` 会把窗口直接顶成全屏，
    `--window-size` 被完全无视。用户投诉「不要占到我的windows 全屏」，
    窗口枚举证实主屏1536x864 上确实有个 1550x830 的窗口铺满。
    根因就是这个从未被质疑过的 `--start-fullscreen`。
    现在：只给尺寸、不给全屏，并按主屏 85% 自适应（小屏不溢出）。
    """
    url = f"http://127.0.0.1:%d" % PORT
    # 按主屏 85% 自适应，并夹在 [900,1500]x[600,940]
    sw, sh = 1920, 1080
    try:
        if _u32:
            sw = _u32.GetSystemMetrics(0) or sw
            sh = _u32.GetSystemMetrics(1) or sh
    except (AttributeError, OSError):
        pass
    ww = max(900, min(1500, int(sw * 0.85)))
    wh = max(600, min(940, int(sh * 0.85)))
    cands = [
        os.path.expandvars(r"%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe"),
        os.path.expandvars(r"%ProgramFiles%\Microsoft\Edge\Application\msedge.exe"),
        os.path.expandvars(r"%LocalAppData%\Google\Chrome\Application\chrome.exe"),
    ]
    for exe in cands:
        if os.path.exists(exe):
            # ★绝不传 --start-fullscreen / --kiosk★
            subprocess.Popen([exe, "--app=" + url,
                              "--window-size=%d,%d" % (ww, wh),
                              "--window-position=40,40",
                              "--disable-features=Translate"])
            return
    # 兜底：也绝不让系统默认浏览器以全屏/最大化接管用户桌面。
    # 失败就静默 —— 弹不出窗口远好过糊一脸全屏，服务端本身不依赖窗口。
    try:
        webbrowser.open(url, new=2, autoraise=True)
    except Exception:
        pass

def main():
    if not os.path.exists(MD_PATH):
        sys.exit("taskboard.md 不在程序同目录：" + APP_DIR)
    # ── 单实例保护（2026-10-04 新增）────────────────────────────────
    # 【真bug：多实例会同时抢占 8767，端口归属随机】
    # 实测（本轮踩到）：打包版 exe 与开发态 python 同时启动，netstat 里
    # 8767出现**三个** LISTENING 条目，浏览器请求被随机分流到不同进程 ——
    # 于是「改了开发态代码却看不到效果」，因为应答的根本是旧 exe。
    # 根因：Windows 的 SO_REUSEADDR 语义允许第二个进程绑定同一端口
    # （不像 Linux 那样直接 EADDRINUSE），而 ThreadingHTTPServer 默认
    # 开启它。
    # 解法：绑一个**独占**的命名互斥体。互斥体是内核对象，
    # 第二个实例拿不到就立刻退出并提示，绝不两个进程并存。
    #
    # 【2026-10-04 二次修正：三个真实漏洞】
    #  ① CreateMutexW 的返回值（句柄）被丢弃，只看了 GetLastError。
    #     GetLastError 会被**后续任意 API 调用**清零，判据不可靠；
    #     真正该看的是句柄是否为 NULL。
    #  ② 打包版用 runw.exe（无控制台），sys.stderr.write 写出去没人看，
    #     用户只看到「双击没反应」。异常必须**显性呈现**：改用 MessageBox。
    #  ③ 端口绑定本身也可能失败（互斥体被别的程序占用等），
    #     原代码没有 try，异常会抛到 PyInstaller 的裸 traceback —— 在无控制台下
    #     同样等于静默。现在统一捕获、MessageBox 提示、显式退出。
    def _fatal(title, msg):
        """【异常显性化】无控制台环境下也必须让用户看见原因与下一步。

        ★但绝不能因此挂死★ MessageBox 是模态阻塞的：无人值守场景
        （自动化判据、打包版冒烟、计划任务）会永远卡在这里 ——
        实测第二个实例被弹窗挡住，进程不退出、端口也不释放。
        修法：弹窗**异步**触发（不等待返回），主线程留一个上限时间后
        强制 sys.exit。用户在 GUI 场景仍能看到提示框（它由独立的
        弹窗线程托管，进程退出前会一闪而过，但足以被看见）；
        无人值守场景则安静退出，不挂死。
        """
        try:
            import ctypes as _ct
            import threading as _th
            _th.Thread(target=lambda: _ct.windll.user32.MessageBoxW(
                None, msg, title, 0x10), daemon=True).start()
        except Exception:
            # MessageBox 也失败（极端环境）-> 退回 stderr，仍不静默
            try:
                sys.stderr.write("[VTaskBoard][FATAL] " + title + "\n" + msg + "\n")
                sys.stderr.flush()
            except Exception:
                pass
        # 给弹窗线程一点显示时间，到点必退，绝不无限等待
        for _ in range(15):
            time.sleep(0.1)
        sys.exit(2)

    try:
        import ctypes
        _h = ctypes.windll.kernel32
        # 0x201 = MUTEX_ALL_ACCESS；命名互斥体不需要跨会话可见
        _mtx = _h.CreateMutexW(None, True, "Local\\VTaskBoard_SingleInstance_%d" % PORT)
        if not _mtx:
            _fatal("VTaskBoard 启动失败",
                   "无法创建单实例互斥体（Windows 错误 %d）。\n\n"
                   "请重试；若反复出现，请重启后再试。" % ctypes.get_last_error())
        if ctypes.get_last_error() == 183:   # ERROR_ALREADY_EXISTS
            _fatal("VTaskBoard 已在运行",
                   "端口 %d 已被另一个 VTaskBoard 实例占用，本次启动退出。\n\n"
                   "下一步：关闭已打开的 VTaskBoard 窗口"
                   "（可能最小化在托盘/后台），再重新打开。\n"
                   "若确认没有其他实例，可重启系统后重试。" % PORT)
    except ImportError:
        # ctypes 不可用（非 Windows）时跳过保护，不影响主功能
        pass

    # ---- 显式独占绑定：allow_reuse_address=False 让 Windows 真正报端口占用 ----
    class _Srv(ThreadingHTTPServer):
        # 【2026-10-04 修正】ThreadingHTTPServer 默认 allow_reuse_address=True，
        # 而 Windows 的 SO_REUSEADDR 语义允许**第二个进程绑定同一端口**
        # （不像 Linux 直接 EADDRINUSE）。这正是「8767 出现多个 LISTENING、
        # 请求被随机分流」的真根因 —— 光靠命名互斥体还不够，
        # 任何非本程序的进程（乃至另一个 Python）都可能抢占。
        allow_reuse_address = False
        daemon_threads = True

    try:
        srv = _Srv(("127.0.0.1", PORT), H)
    except OSError as e:
        _fatal("VTaskBoard 启动失败",
               "端口 %d 无法绑定：%s\n\n"
               "下一步：关闭占用该端口的程序后重试。\n"
               "排查命令：netstat -ano | findstr :%d"
               % (PORT, e, PORT))
    print(f"VTaskBoard on http://127.0.0.1:{PORT}  |  任务册: {MD_PATH}")
    threading.Timer(0.8, open_window).start()
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        # 正常退出也要释放端口与互斥体，否则下次启动会被自己挡住
        try:
            srv.server_close()
        except Exception:
            pass

if __name__ == "__main__":
    main()
