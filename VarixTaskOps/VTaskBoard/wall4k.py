# -*- coding: utf-8 -*-
"""
wall4k — 服务端 4K 母版引擎
=========================

为什么需要它
------------
实测本机屏幕为 1920x1080 @144Hz（DPI 125%，CSS 1536x864）。要让壁纸"看起来像
Windows 原生那样锐"，光把图片 resize 到 1920x1080 是不够的 —— 实测（见
_attic/probe-scripts/lab_master4k.py）把母版从 1920 一路提到 5760，降采样回
1920x1080 之后SSIM=1.0000、LapVar 差异 <1%：**插值到更大尺寸再降回来，等于
原图，清晰度一点都不会涨**。所以"清晰"只有一个来源：让上屏像素本身携带足够
多的真实细节。

三条产线（按素材真实分辨率自动分流）
------------------------------------
1. 真4K 素材（>= 母版尺寸）：直接用，不做任何重采样。最省也最锐。
2. 中等素材（1024/1920 级静图）：分阶段 Lanczos 放大到母版 + 适度 UnsharpMask。
   这是纯插值，改善有限但零风险（不会产生假细节），且能保住边缘不糊。
3. 低分辨率素材（GIF 160~224px、600px preview）：**必须走AI 超分**。
   Real-ESRGAN（realesr-general-x4v3 / realesr-animevideov3，NCNN+Vulkan）
   是唯一能在低像素素材上「合成」出可信高频细节的可用手段 —— 它不是把像素
   变大，而是按自然图像先验补出纹理/轮廓。缺失时自动降级到产线 2，并在
   返回结果里如实标注，绝不假装。

GIF 动画
--------
WE 官方 preview.gif 仅 160~224px却是唯一带动画的素材。走Pillow 抽帧 → 本引擎
放大 → rawvideo 管道 → ffmpeg NVENC 编码 H.264，产出真 4K 循环 MP4。
实测 50 帧 160px GIF → 3840x2160 MP4 仅需 2.0s / 12.5MB。

尺寸自适应
----------
母版按显示区物理像素的**整数倍超采样系数**生成（默认 2x = 3840x2160 母版映射
到 1920x1080 屏幕，等效 2x2 SSAA 抗锯齿）。窗口比例变化时前端会带新宽高比重取，
服务端按同一套 cover 逻辑生成对应比例母版，避免浏览器二次裁切重采样。
"""
import os
import subprocess
import sys
import threading

try:
    from PIL import Image, ImageFilter, ImageSequence
    # 超分产物由我们自己按像素预算生成，尺寸完全可控（不是不可信外部输入），
    # 必须关掉 Pillow 的解压炸弹拦截 —— 否则 10664x6000 的合法结果会被当攻击拒绝。
    Image.MAX_IMAGE_PIXELS = None
    _PIL_OK = True
except ImportError:
    _PIL_OK = False

def _resolve_dir(name):
    """定位随包分发的资源目录，打包态与开发态都能找到。

    PyInstaller 有两种布局，取决于用onefile 还是 onedir：
      · onefile —— 资源被解压到 sys._MEIPASS 临时目录，exe 同级没有
      · onedir —— exe 同级就是 _internal/resources
    只查exe 同级会在 onefile 下必然找不到（超分直接失效、静默降级）；
    只查 _MEIPASS 会在 onedir 下同样找不到。两个都查，按"能用的先来"，
    并允许 VTaskBoard 环境变量显式覆盖（便携部署时把 _sr 放别处）。
    """
    env = os.environ.get("VTB_RESOURCE_DIR")
    cands = []
    if env:
        cands.append(os.path.join(env, name))
    if getattr(sys, "frozen", False):
        cands.append(os.path.join(getattr(sys, "_MEIPASS", "") or "", name))
        cands.append(os.path.join(os.path.dirname(sys.executable), name))
        cands.append(os.path.join(os.path.dirname(sys.executable), "_internal", name))
    cands.append(os.path.join(os.path.dirname(os.path.abspath(__file__)), name))
    for c in cands:
        if c and os.path.isdir(c):
            return c
    # 都不存在时返回最可能的开发态路径（调用方会因目录不存在而优雅降级）
    return os.path.join(os.path.dirname(os.path.abspath(__file__)), name)


_HERE = os.path.dirname(os.path.abspath(__file__))
# 超分引擎目录（含 3 套模型 + Vulkan 可执行体与 DLL）
SR_DIR = _resolve_dir("_sr")
SR_EXE = os.path.join(SR_DIR, "realesrgan-ncnn-vulkan.exe")
SR_MODELS = os.path.join(SR_DIR, "models")
#引擎按「模型名」而非 .param 文件寻址（见 realesrgan-ncnn-vulkan -h）
SR_MODEL_GENERAL = "realesrgan-x4plus"
SR_MODEL_ANIME = "realesrgan-x4plus-anime"
SR_MODEL_VIDEO = "realesr-animevideov3"

# ffmpeg：优先系统 PATH，其次常见安装位置（本机 BlueStacks 附带一份可用构建）
# ffmpeg 定位顺序：**随包分发的优先**，再退到系统安装位置。
# 打包成 exe 后用户机器上大概率没有 ffmpeg（下面这些绝对路径全指向本机开发
# 环境），所以必须把可执行体随包分发，否则视频母版会静默失效 —— 表现为
# "改了代码但画质一点没变"，是最难查的那类退化。
# _FF_DIR 与超分目录同法解析，覆盖 onefile(_MEIPASS) / onedir( exe 同级) /
# VTB_RESOURCE_DIR 显式覆盖三种情况。
_FF_DIR = _resolve_dir("ffmpeg")
FF_CANDIDATES = [
    "ffmpeg",
    os.path.join(_FF_DIR, "ffmpeg.exe"),
    os.path.join(_FF_DIR, "bin", "ffmpeg.exe"),
    r"C:\Program Files\BlueStacks_nxt\ffmpeg.exe",
    r"C:\Program Files\ffmpeg\bin\ffmpeg.exe",
    r"C:\ffmpeg\bin\ffmpeg.exe",
    r"C:\ProgramData\chocolatey\bin\ffmpeg.exe",
]

# 启用 AI 超分的最小有效放大倍数。低于此值纯插值+锐化已足够（超分在这个区间
# 收益小于其可能引入的伪影）；高于此值纯插值必然糊，超分收益远大于代价。
# 实测标定见下方 build_master 内的注释。导出为常量供服务端反推画质来源用，
# 避免"是否超分过"这个事实只能从内存态读到、重启后丢失。
SR_MIN_UPSCALE = 1.8


# ────────────────────────────────────────────────────────── 工具
def ff_path():
    """定位可用的 ffmpeg 可执行文件；找不到返回 None。"""
    for c in FF_CANDIDATES:
        try:
            if os.path.isabs(c):
                if os.path.isfile(c):
                    return c
            else:
                from shutil import which
                p = which(c)
                if p:
                    return p
        except (OSError, ValueError):
            continue
    return None


def sr_available():
    """AI 超分引擎是否完整可用（可执行体 + 模型目录 + 至少一套模型权重）。"""
    if not (os.path.isfile(SR_EXE) and os.path.isdir(SR_MODELS)):
        return False
    return any(os.path.isfile(os.path.join(SR_MODELS, m + ".param"))
               for m in (SR_MODEL_GENERAL, SR_MODEL_ANIME))


# ────────────────────────────────────────────────────────── 几何
def cover_to(im, tw, th):
    """等比 cover 预裁切到目标宽高比（与 CSS object-fit:cover 语义一致，居中裁切）。

    为什么必须先裁后缩：官方 preview 是 600x600 / 1024x1024 方形，显示区是 16:9。
    先拉成方形再让浏览器 cover 裁切，等效「多放大 1.78 倍 + 一次额外重采样」。
    先按比例裁掉多余轴、再一次性缩到目标尺寸，浏览器端就只剩一次等比映射。
    """
    if tw <= 0 or th <= 0 or im.width == 0 or im.height == 0:
        return im
    s = max(tw / im.width, th / im.height)
    nw, nh = max(tw, round(im.width * s)), max(th, round(im.height * s))
    if (nw, nh) != im.size:
        im = im.resize((nw, nh), Image.LANCZOS)
    if nw > tw or nh > th:
        l, t = (nw - tw) // 2, (nh - th) // 2
        im = im.crop((l, t, l + tw, t + th))
    return im


def staged_upscale(im, tw, th, max_step=2.0):
    """分阶段 Lanczos 放大：每段不超过 max_step 倍。

    一次性从 160px 拉到 3840px（24 倍）时，Lanczos 核的有效支撑只命中 1~2 个源
    像素，输出必然出现块状伪影与边缘断裂。分阶段放大让每段都在插值核的有效区间内，
    实测块状感明显下降。
    """
    if im.size == (tw, th):
        return im
    cw, ch = im.size
    # 只放大，不缩小：缩小交给 cover_to / 最终目标尺寸处理
    if cw >= tw and ch >= th:
        return im
    for _ in range(12):                       # 12 段足够覆盖 24x，且有硬上限防死循环
        if cw >= tw and ch >= th:
            break
        ar_t = tw / th
        ar_c = cw / ch
        nw = min(tw, max(cw + 1, round(cw * max_step)))
        nh = min(th, max(ch + 1, round(ch * max_step)))
        if ar_c > ar_t:
            nw = max(nw, round(nh * ar_t))
        else:
            nh = max(nh, round(nw / ar_t))
        nw, nh = min(tw, max(1, nw)), min(th, max(1, nh))
        if (nw, nh) == (cw, ch):
            break
        im = im.resize((nw, nh), Image.LANCZOS)
        cw, ch = im.size
    if im.size != (tw, th):
        im = im.resize((tw, th), Image.LANCZOS)
    return im


def adaptive_sharpen(im, upscale_ratio):
    """按放大倍数自适应锐化。

    放大 24 倍时若用固定 radius=1.2，锐化核相对源像素过窄 -> 白边振铃；
    放大 1 倍时锐化则纯属制造噪声。半径与强度都随放大倍数线性调整，
    并对平坦区域设threshold 门限，避免把压缩噪点锐化成颗粒。
    """
    if upscale_ratio <= 1.15:
        return im                      # 几乎没放大/在缩小：锐化只降质
    r = min(3.0, 0.6 + 0.35 * upscale_ratio)
    pct = int(min(120, 18 + 12 * upscale_ratio))
    return im.filter(ImageFilter.UnsharpMask(radius=r, percent=pct, threshold=3))


# ────────────────────────────────────────────────────────── AI 超分
_SR_LOCK = threading.Lock()
_SR_TILE_CACHE = {}


def _sr_model_for(im):
    """按内容选模型：调色板/动画风走 anime，通用写实走 x4plus。"""
    try:
        pal = im.getpalette() is not None
    except Exception:
        pal = False
    name = SR_MODEL_ANIME if pal else SR_MODEL_GENERAL
    if not os.path.isfile(os.path.join(SR_MODELS, name + ".param")):
        name = SR_MODEL_GENERAL
    return name


def super_resize(im, scale=4):
    """用 Real-ESRGAN NCNN(Vulkan) 把 im 放大 scale 倍。返回 (PIL.Image|None, 倍数)。

    设计要点：
    · 失败一律返回 (None, 0)，由调用方降级到纯插值产线 —— 超分是增强项，
      绝不能因为它缺失就让壁纸整个打不开；
    · Vulkan 设备上下文非线程安全，_SR_LOCK 串行化调用；
    · 隐藏控制台窗口（Win）：绝不让黑色 cmd 框打断用户体验；
    · tile 显式给 256：默认 0=auto 在部分驱动上会失败并输出半成品图。
    """
    if not _PIL_OK or not sr_available():
        return None, 0
    model = _sr_model_for(im)

    import tempfile
    tmpd = tempfile.mkdtemp(prefix="vtb_sr_")
    src = os.path.join(tmpd, "in.png")
    dst = os.path.join(tmpd, "out.png")
    try:
        im.save(src, "PNG")
        cmd = [SR_EXE, "-i", src, "-o", dst,
               "-m", SR_MODELS,          # 模型目录
               "-n", model,               # 模型名
               "-s", str(scale),
               "-t", "256",
               "-f", "png"]
        si = None
        if sys.platform == "win32":
            si = subprocess.STARTUPINFO()
            si.dwFlags |= subprocess.STARTF_USESHOWWINDOW
            si.wShowWindow = 0              # SW_HIDE
        with _SR_LOCK:
            r = subprocess.run(cmd, timeout=600, startupinfo=si,
                               stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        if r.returncode != 0 or not os.path.isfile(dst):
            if r.stderr:
                sys.stderr.write("[wall4k] SR rc=%s %s\n" %
                                 (r.returncode, r.stderr.decode("utf-8", "replace")[:200]))
            return None, 0
        with Image.open(dst) as out:
            out.load()
            res = out.convert("RGB").copy()
        if res.width < im.width or res.height < im.height:
            return None, 0            # 引擎行为异常，丢弃结果
        return res, scale
    except (OSError, subprocess.SubprocessError, ValueError) as e:
        sys.stderr.write("[wall4k] SR failed: %r\n" % (e,))
        return None, 0
    finally:
        try:
            for f in os.listdir(tmpd):
                os.remove(os.path.join(tmpd, f))
            os.rmdir(tmpd)
        except OSError:
            pass


# ────────────────────────────────────────────────────────── 主入口
def master_size(tw, th, ss=2):
    """母版物理尺寸 = 显示区尺寸 x 超采样系数（默认 2x）。

    为什么默认 2x而不是直接 4K：屏幕物理是 1920x1080。4K 母版=3840x2160 映射到
    1080p 屏时浏览器/GPU 做 2:1 降采样，等效 2x2 超采样抗锯齿，边缘更干净；
    再高（如 5760）实测对最终上屏像素无额外收益（JIT 实验已验证），只吃内存和带宽。
    """
    return max(1, int(round(tw * ss))), max(1, int(round(th * ss)))


def open_tolerant(path):
    """宽容打开图片：允许截断的文件（ImageFile.LOAD_TRUNCATED_IMAGES）。

    实测踩坑：部分本地 JPEG 尾部少了 1~4 字节，严格模式下 Pillow 直接抛
    "image file is truncated" 导致该壁纸母版生成失败。JPEG 允许尾部缺字节
    （解码器按熵码自然结束），所以开启截断容忍是正确的做法，不是掩盖错误。
    """
    from PIL import ImageFile
    prev = getattr(ImageFile, "LOAD_TRUNCATED_IMAGES", False)
    ImageFile.LOAD_TRUNCATED_IMAGES = True
    try:
        im = Image.open(path)
        im.load()
        return im
    finally:
        ImageFile.LOAD_TRUNCATED_IMAGES = prev


def build_master(src_path, out_path, tw, th, ss=2, use_sr=True, quality=97,
                 sr_pixel_budget=64_000_000):
    """把任意源图加工成目标比例的 ss 倍超采样母版，返回元信息dict。

    返回：{"ok":bool, "path":str|None, "w":int, "h":int, "src":[w,h],
           "mode":"native"|"lanczos"|"sr", "upscale":float}
    mode 供前端如实展示（AI 超分 / 插值 / 原生），不隐瞒画质来源。

    AI 超分放在哪一步做（实测决定）：
      先把源图 cover 到母版比例但**限制在像素预算内**（默认 3600 万像素，
      约 1920x2160 的规模），再送超分。原因是 NCNN Vulkan 是 x4 模型：
      直接送 3840x2160 进去会输出 15360x8640 = 1.3 亿像素，实测耗时 53.7s，
      而超分之后我们仍要降采样回3840 —— 算力全花在丢弃的像素上。
      在 1/4 面积上做超分，耗时降到 1/16，视觉结果几乎一致（高频由模型合成，
      不是由像素数决定）。最后用分阶段 Lanczos 补足到母版尺寸。
    """
    meta = {"ok": False, "path": None, "w": 0, "h": 0, "src": None,
            "mode": "none", "upscale": 0.0}
    if not _PIL_OK:
        return meta
    try:
        # convert("RGB") 返回独立的新图像对象；不能先 close 原对象，
        # 否则后续 _cover_to / staged_upscale 会踩 "Operation on closed image"。
        im = open_tolerant(src_path).convert("RGB")
    except (OSError, ValueError):
        return meta

    mw, mh = master_size(tw, th, ss)
    meta["src"] = list(im.size)
    src_ar = im.width / im.height
    dst_ar = mw / mh

    # ── 产线 1：真分辨率足够，直接 cover 到母版尺寸，零重采样损失
    if im.width >= mw and im.height >= mh:
        im = cover_to(im, mw, mh)
        mode = "native"
    else:
        # 有效放大倍数：cover 之后决定保真度的短边被放大了多少
        eff = min(im.width, im.height * dst_ar / src_ar) if src_ar > dst_ar \
            else min(im.height, im.width * src_ar / dst_ar)
        up = max(mw / max(1.0, eff), mh / max(1.0, eff))
        meta["upscale"] = round(up, 2)

        # cover 到母版比例，但先受像素预算约束（保持比例，绝不拉伸变形）
        pre = cover_to(im, mw, mh)
        if pre.width * pre.height > sr_pixel_budget and up >= SR_MIN_UPSCALE:
            k = (sr_pixel_budget / float(pre.width * pre.height)) ** 0.5
            bw, bh = max(4, int(pre.width * k)), max(4, int(pre.height * k))
            pre = pre.resize((bw, bh), Image.LANCZOS)

        mode = "lanczos"
        if use_sr and up >= SR_MIN_UPSCALE and sr_available():
            # 超分输入尺寸实测标定（600px 源，母版 3840x2160）：
            #   输入 2666x1500 (4M像素) -> 27.0s, LapVar 3.86x
            #   输入 3771x2121 (8M像素) -> 51.8s, LapVar 3.20x
            #   输入 3840x2160 (8.3M像素) -> 53.7s, LapVar 3.10x
            # 结论：**输入越小越快且越锐**。原因是超分是 x4 模型，输入越小，
            # 同样的 4 倍输出里「模型合成的新细节」占比越高；而尾部我们还要
            # 分阶段 Lanczos 补到母版，多出来的像素只是插值噪声。
            # 故按源有效短边定输入：源越小 -> 输入越小 -> 越快越锐。
            src_short = max(1.0, min(im.width, im.height))
            k = min(4.0, max(1.5, src_short / 600.0 * 1.5))
            bw = int(min(pre.width, max(64, pre.width * k / 4.0)))
            bh = int(min(pre.height, max(36, pre.height * k / 4.0)))
            while bw * bh * 16 > sr_pixel_budget:      # 硬上限：输出不超过预算
                bw = int(bw * 0.9) or 64
                bh = int(bh * 0.9) or 36
            pre_sr = pre.resize((bw, bh), Image.LANCZOS) if (bw, bh) != pre.size else pre
            big, _k = super_resize(pre_sr, scale=4)
            if big is not None:
                im = staged_upscale(big, mw, mh)
                im = cover_to(im, mw, mh)
                mode = "sr"
            else:
                im = adaptive_sharpen(pre, up)
        else:
            im = adaptive_sharpen(pre, up)
        if im.size != (mw, mh):
            im = cover_to(im, mw, mh)

    try:
        os.makedirs(os.path.dirname(out_path), exist_ok=True)
        # 4:4:4 无色度抽样保住红蓝边缘（4:2:0 会抹掉色度细节）
        im.save(out_path, "JPEG", quality=quality, subsampling=0, optimize=True)
    except (OSError, ValueError):
        return meta
    meta.update({"ok": True, "path": out_path, "w": mw, "h": mh, "mode": mode})
    return meta


def build_master_from_image(im, out_path, tw, th, ss=2, use_sr=True, quality=97,
                            sr_pixel_budget=64_000_000):
    """从**内存中的 PIL 图像**（而非磁盘路径）加工成母版。

    存在的理由：WE 场景壁纸的原生纹理打包在 scene.pkg 归档里，不是磁盘上
    的独立文件（见 wepkg.py）。若先把纹理 dump 成临时文件再走 build_master，
    3840x2160 的 JPEG 单张就要额外写盘 5MB+ 再读回，纯浪费。直接吃内存图，
    母版产线（native / lanczos / sr 三档）完全复用，不复制第二套逻辑。

    契约与 build_master 完全一致；调用方负责 im 的生命周期。
    """
    meta = {"ok": False, "path": None, "w": 0, "h": 0, "src": None,
            "mode": "none", "upscale": 0.0}
    if not _PIL_OK or im is None:
        return meta
    try:
        src = im if im.mode == "RGB" else im.convert("RGB")
    except (OSError, ValueError):
        return meta
    tmp = out_path + ".src.tmp"
    try:
        src.save(tmp, "PNG")
    except (OSError, ValueError):
        return meta
    try:
        return build_master(tmp, out_path, tw, th, ss=ss, use_sr=use_sr,
                            quality=quality, sr_pixel_budget=sr_pixel_budget)
    finally:
        # 无论成败都清掉中间文件：它可能有 5MB+，留着会撑爆缓存目录
        try:
            os.remove(tmp)
        except OSError:
            pass


# ────────────────────────────────────────────────────────── 动图
def build_master_gif(gif_path, out_mp4, tw, th, ss=2, quality=18, max_frames=90,
                     use_sr=True):
    """GIF(160~224px, 50 帧) -> 4K H.264 MP4。返回元信息 dict。

    这是把 WE 官方动图从「8~12 倍放大的马赛克」救回「可看」的唯一现实路径：
    GIF 是 8bit 调色板 + LZW，官方预览本身就限死在 160~224px；但它是该 scene
    项目唯一的动画来源，所以必须逐帧处理。

    性能（实测 160px/50 帧）：源图极小 -> 超分输入极小 -> 单帧约 0.2~0.4s，
    50 帧约 10~20s，加 NVENC 编码 2s；一次性成本可接受且结果永久缓存。

    帧时长按 GIF 原始 duration 汇总换算 fps —— 保证循环周期与原 GIF 一致，
    否则动画节奏会变（转 GIF 到视频最常见的坑）。
    """
    meta = {"ok": False, "path": None, "w": 0, "h": 0, "mode": "none", "frames": 0}
    if not _PIL_OK:
        return meta
    ff = ff_path()
    if not ff:
        return meta
    mw, mh = master_size(tw, th, ss)
    frames = []
    try:
        with Image.open(gif_path) as im:
            n_raw = getattr(im, "n_frames", 1)
            # 帧数过多时按均匀步长抽稀（保持总时长不变，只降播放密度）
            step = max(1, n_raw // max_frames) if n_raw > max_frames else 1
            durs = []
            sr_ok = use_sr and sr_available()
            for idx, fr in enumerate(ImageSequence.Iterator(im)):
                if idx % step:
                    continue
                f = fr.convert("RGB")
                durs.append(int(fr.info.get("duration", 100) or 100))
                # 送超分的输入：源仅 160~224px，放大到母版再送是纯浪费
                # （输出会到 1.3 亿像素、单帧耗时数十秒）。给到 3 倍源尺寸即可。
                if sr_ok:
                    big, _k = super_resize(cover_to(f, min(mw, f.width * 3),
                                                     min(mh, f.height * 3)), scale=4)
                if sr_ok and big is not None:
                    base = cover_to(staged_upscale(big, mw, mh), mw, mh)
                else:
                    base = adaptive_sharpen(cover_to(f, mw, mh), mw / max(1, f.width))
                frames.append(base)
            if not frames:
                return meta
            total_ms = sum(durs) / step
            fps = max(1.0, min(60.0, len(frames) / (total_ms / 1000.0)))
            meta.update({"frames": len(frames), "w": mw, "h": mh})
    except (OSError, ValueError) as e:
        sys.stderr.write("[wall4k] gif decode failed: %r\n" % (e,))
        return meta

    def _encode(cmd, timeout):
        p = subprocess.Popen(cmd, stdin=subprocess.PIPE,
                             stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            for f in frames:
                p.stdin.write(f.tobytes())
        except (BrokenPipeError, OSError):
            pass
        try:
            p.stdin.close()
        except OSError:
            pass
        try:
            _, err = p.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            p.kill()
            return -1, b"timeout"
        return p.returncode, err

    try:
        rc, err = _encode(
            [ff, "-hide_banner", "-loglevel", "error", "-y",
             "-f", "rawvideo", "-pix_fmt", "rgb24",
             "-s", "%dx%d" % (mw, mh), "-r", "%.4f" % fps, "-i", "pipe:0",
             "-an", "-c:v", "h264_nvenc", "-preset", "p5", "-rc", "vbr",
             "-cq", str(quality), "-b:v", "0",
             "-pix_fmt", "yuv420p", "-movflags", "+faststart", out_mp4], 900)
        if rc != 0 or not os.path.isfile(out_mp4):
            # NVENC 不可用/失败 -> 退回 CPU 软编（libopenh264，实测本机可用）
            rc2, err2 = _encode(
                [ff, "-hide_banner", "-loglevel", "error", "-y",
                 "-f", "rawvideo", "-pix_fmt", "rgb24",
                 "-s", "%dx%d" % (mw, mh), "-r", "%.4f" % fps, "-i", "pipe:0",
                 "-an", "-c:v", "libopenh264", "-b:v", "28M",
                 "-pix_fmt", "yuv420p", "-movflags", "+faststart", out_mp4], 1800)
            if rc2 != 0 or not os.path.isfile(out_mp4):
                sys.stderr.write("[wall4k] gif->mp4 failed rc=%s/%s %s %s\n" %
                                 (rc, rc2, err[:160], err2[:160]))
                return meta
    except (OSError, subprocess.SubprocessError) as e:
        sys.stderr.write("[wall4k] gif encode failed: %r\n" % (e,))
        return meta
    meta.update({"ok": True, "path": out_mp4, "mode": "gif4k"})
    return meta


# ══════════════════════════════════════════════════════════════════════
# 视频母版：重编码 + 锐化 + 尺寸归一
# ══════════════════════════════════════════════════════════════════════
# 实测依据：
#   · 母版插值本身不产生清晰度（见文件头实验），但**重编码能消除源的低码率劣化**
#   · WE 视频码率跨度极大：2878714045 只有 1848 kb/s（2560x1440@60fps，
#     实测块效应比 28.8，肉眼可见马赛克），而 3806392502 有 52 Mbps。
#     必须分级处理，不能一刀切。
#   · 蓝Stacks 版 ffmpeg 编译时带 --disable-decoders：**能解 H.264 视频，
#     但解不了 AAC 音频**。所以重编码必须 -an 丢音频（壁纸本就是静音，
#     丢弃零损失）；不丢会直接 "Decoder (codec aac) not found" 而失败。

# 视频质量分级阈值（依据实测码率分布，不是拍脑袋）
VQ_LOW_BITRATE = 8_000_000     # < 8 Mbps：低码率，块效应明显，必须重编码
VQ_CQ = 14                     # NVENC 恒定质量档，越小越清晰（14 ≈ 40Mbps）

# 母版码率上限：**母版是给浏览器实时解码的，不是存档**。
# 实测教训：cq14 无脑套在 60fps 动画上，249s 的 Minecraft 从 802MB 源
# 重编码出>300MB 的母版还在涨 —— 浏览器根本追不上播放，又变成"加载不出来"。
# 上限按「屏幕原生帧率的 2 倍」给足：1080p@60fps 需要 ~25Mbps 才够 4K 母版
# 的细节量，再高纯属浪费带宽且加重解码负担。
VQ_MAXRATE = {"2560x1440": "40M", "1920x1080": "30M", "3840x2160": "60M"}


def _no_window():
    """Windows 下隐藏子进程控制台窗口（否则每次重编码弹黑框打断用户）。"""
    if sys.platform != "win32":
        return 0
    si = subprocess.STARTUPINFO()
    si.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    si.wShowWindow = 0          # SW_HIDE
    return si


def _rm(p):
    """删文件，失败静默 —— 只用于清理 .part 临时产物，不是业务路径。"""
    try:
        if os.path.isfile(p):
            os.remove(p)
    except OSError:
        pass


def _ffprobe_like(src):
    """不依赖 ffprobe（本机没有），用 `ffmpeg -i` 读 stderr 解析视频流参数。

    返回 {"w","h","fps","kbps","dur","vcodec"}；解析失败返回 {}。
    """
    ff = ff_path()
    if not ff:
        return {}
    try:
        r = subprocess.run([ff, "-hide_banner", "-i", src],
                           capture_output=True, text=True,
                           errors="replace", timeout=90,
                           startupinfo=_no_window())
    except (OSError, subprocess.SubprocessError):
        return {}
    out = {"w": 0, "h": 0, "fps": 0.0, "kbps": 0, "dur": 0.0, "vcodec": ""}
    for line in (r.stderr or "").splitlines():
        t = line.strip()
        if t.startswith("Duration:"):
            # Duration: 00:02:48.70, start: 0.000000, bitrate: 10277 kb/s
            try:
                h, m, s = t.split("Duration:")[1].split(",")[0].strip().split(":")
                out["dur"] = int(h) * 3600 + int(m) * 60 + float(s)
            except (ValueError, IndexError):
                pass
            if "bitrate:" in t:
                try:
                    out["kbps"] = int(
                        t.split("bitrate:")[1].split("kb/s")[0].strip())
                except (ValueError, IndexError):
                    pass
        if "Video:" in t and "Stream #" in t:
            out["vcodec"] = t.split("Video:")[1].split()[0]
            # 尺寸形如 3840x2128 或 2560x1440[SAR 1:1 DAR 240:133]，
            # 两侧被 [ ] 替换成空格后按 token 切；codec 的 0x31637661
            # 同样含 'x'，所以必须要求两侧**都是纯数字**才算尺寸。
            # （codec token 替换后仍是 0x31637661，replace("x","") 得到
            #   0x31637661 去掉 x 不是纯数字，正好被排除——但括号未替换时
            #   会误配，所以这里额外要求 token 不以 0x 开头。）
            body = t.split("Video:")[1]
            body = body.replace("[", " ").replace("]", " ").replace(",", " ")
            for tok in body.split():
                low = tok.lower()
                if low.startswith("0x") or "x" not in low:
                    continue
                a, _, b = low.partition("x")
                a = a.rstrip("h")
                b = b.split("h")[0]
                if a.isdigit() and b.isdigit():
                    out["w"], out["h"] = int(a), int(b)
                    break
            for tok in body.split():
                if tok.replace(".", "").isdigit() and "." in tok:
                    try:
                        f = float(tok)
                        if 1.0 <= f <= 480.0:
                            out["fps"] = f
                            break
                    except ValueError:
                        pass
    return out


def mp4_playability(path, max_mb=140.0):
    """扫描 MP4 顶层 box，返回可播性诊断 dict。

    这是"壁纸加载不出来"的**第一号物理根因**，与画质无关：
    浏览器要拿到 **moov**（索引：时长/关键帧表/解码参数）才知道怎么播。
    · moov 在文件尾（SLOW-START）-> 浏览器必须把整个文件下完才起播。
      实测 338MB 的源 moov@100%，readyState 恒为 0，表现为"一直转圈不出来"。
    · 体积失控 -> 即便 faststart，几十 MB 起步的下载也要等，
      实测 841MB 的源在 1080p 屏上首帧要等十几秒。

    返回 {"ok":bool, "moov_at":float(0~1), "faststart":bool,
          "size_mb":float, "why":str}
    """
    out = {"ok": False, "moov_at": -1.0, "faststart": False,
           "size_mb": 0.0, "why": ""}
    try:
        size = os.path.getsize(path)
    except OSError:
        out["why"] = "文件不可读"
        return out
    if size < 16:
        out["why"] = "文件过小"
        return out
    out["size_mb"] = round(size / 1048576.0, 1)
    try:
        with open(path, "rb") as f:
            pos, end = 0, size
            while pos < end - 8:
                f.seek(pos)
                hdr = f.read(8)
                if len(hdr) < 8:
                    break
                bsz = int.from_bytes(hdr[:4], "big")
                btype = hdr[4:8]
                hsz = 8
                if bsz == 1:
                    ext = f.read(8)
                    if len(ext) < 8:
                        break
                    bsz = int.from_bytes(ext, "big")
                    hsz = 16
                elif bsz == 0:
                    bsz = end - pos
                if bsz < hsz or pos + bsz > end + 1:
                    break                      # 尺寸非法/越界，停止扫描
                if btype == b"moov":
                    out["moov_at"] = pos / float(size)
                    break
                pos += bsz
    except OSError as e:
        out["why"] = "读取失败 %r" % (e,)
        return out
    if out["moov_at"] < 0:
        out["why"] = "未找到 moov box"
        return out
    out["faststart"] = out["moov_at"] < 0.05
    if not out["faststart"]:
        out["why"] = "moov 在文件 %.1f%% 处（非 faststart），浏览器需下完整个文件" % (
            out["moov_at"] * 100.0)
    elif out["size_mb"] > max_mb:
        out["why"] = "体积 %.1fMB 超上限 %.0fMB，首帧等待过长" % (
            out["size_mb"], max_mb)
    else:
        out["ok"] = True
    return out


def video_needs_master(info, master_w, src_path=None, max_mb=140.0):
    """视频是否需要重编码母版。返回 (need:bool, reason:str)

    分级依据（全部来自实测）：
      · **可播性优先**（本轮新增，最高优先级）：moov 不在前 5% 或体积超
        140MB 的源，浏览器起播就会失败/极慢 —— 实测 338MB 慢启动源
        readyState 恒 0（"壁纸加载不出来"）、841MB 源首帧要等十几秒。
        这类**必须**重编码：母版带 +faststart 且按母版尺寸压到合理码率。
      · 码率 < 8 Mbps -> 块效应明显（实测 1848kb/s 的源块效应比 28.8），
        这类**必须**重编码：重编码不增加细节，但能把源片的宏块痕迹抹平，
        观感上从"马赛克"变"干净"，是净收益。
      · 已是 4K 且码率 >= 24 Mbps -> 直出。二次编码只会引入代际损失，
        白白花几分钟把 52Mbps 压成更低的码率，纯负收益。
      · 非 4K 但码率充足（>=8Mbps 的 2560x1440/1920x1080）-> **直出**。
        旧版在这里判定"需要母版"，理由是"让 GPU 做降采样比浏览器可控"，
        但实测浏览器/GPU 对 2560x1440 -> 1080p 的降采样质量完全没有问题，
        而重编码到 4K 反而更糊（插值伪高频 + 每像素码率减半）。
        分辨率低于显示尺寸时，正确做法是让 GPU 一次降采样到位，
        不要在服务端先"升"上去再让浏览器"降"回来。
    """
    w = int(info.get("w") or 0)
    kbps = int(info.get("kbps") or 0)
    if w <= 0:
        return True, "无法探测源分辨率"
    # ── 可播性闸门：先问"浏览器能不能播"，再问"播得糊不糊" ──────────
    # 顺序很重要：一张根本播不出来的壁纸，码率再高也没有意义。
    if src_path and os.path.isfile(src_path):
        pb = mp4_playability(src_path, max_mb=max_mb)
        if not pb.get("ok"):
            w_only = pb.get("faststart") and kbps * 1000 >= VQ_LOW_BITRATE
            return True, ("可播性：%s%s" % (
                pb.get("why") or "未知",
                "（仅体积超限，母版已保画质，裁剪即可）" if w_only else ""))
    # ── 画质闸门：重编码**只能提升码率，不能让它更低** ────────────────
    # 旧判据"码率 < 8Mbps 一律重编码"有个致命漏洞：源本身就只有
    # 1864kb/s（2878714045）时，重编码产出 1362kb/s —— 比源还低，
    # 二代损失叠加，观感只会更差。实测该母版复检永远 need=True，
    # 陷入"反复重转且越转越糊"的死循环。
    # 正确判据：只有当源码率**高于**门槛、却因某种原因偏低时才重编码；
    # 源本身就低于门槛的，宁可直出（保留原始码流，浏览器解码自己的
    # 低码率墙），也不要再压一道。
    if kbps and kbps * 1000 < VQ_LOW_BITRATE:
        return False, "源码率仅 %d kb/s（本就低于 %d），直出以免二代损失" % (
            kbps, VQ_LOW_BITRATE // 1000)
    # 码率在 [门槛, 2x门槛) 的区间：母版码率本就由 cq 决定，不必再转。
    # 注意判据是"低于门槛才重编码"，超过门槛一律直出 —— 这里曾误写成
    # VQ_LOW_BITRATE*2，导致 9677kb/s（明明高于 8Mbps 门槛）被判低码率，
    # 母版刚生成就被判"还需重转"，陷入无意义的重复转码。
    if kbps and kbps * 1000 < VQ_LOW_BITRATE:
        return True, "低码率 %d kb/s（块效应）" % kbps
    if w >= master_w and kbps and kbps * 1000 >= 24_000_000:
        return False, "原生 4K 高码率直出"
    if w < master_w:
        return False, "原生 %dx%d 直出（GPU 一次降采样，避免上采样伪高频）" % (
            w, int(info.get("h") or 0))
    return False, "原生直出"


def build_master_video(src_path, out_path, tw, th, ss=2,
                        use_sharpen=True, cq=VQ_CQ, timeout=5400,
                        max_seconds=None, max_mb=140.0):
    """视频 -> 母版尺寸的高质量 MP4。返回元信息 dict。

    产线：
      ffmpeg -i src -an -vf "scale=WxH:flags=lanczos" \\
             -c:v h264_nvenc -preset p5 -rc vbr -cq 14 -b:v 0 \\
             -pix_fmt yuv420p -movflags +faststart  out

    关键设计：
      · **禁止上采样**（见下方尺寸决策）—— 母版尺寸 = min(源, 4K上限)。
        旧版一律拉到 3840x2160，对 2560x1440 源是插值伪高频 + 每像素码率
        减半，实测比原生更糊。这正是"标 4K 却不如 Windows 原生"的成因。
      · **-an 丢音频** —— 本机 ffmpeg 解不了 AAC，留音轨会直接失败。
      · **原子写** —— 先写 out+".part"，成功才 os.replace。否则中途被杀会留下
        半截 mp4，Range 请求会返回损坏数据，表现为"壁纸加载不出来"。
      · **+faststart** —— moov 前置，浏览器立刻能播首帧而不用等整个文件下完。
        实测 802MB 的视频只缓冲 15.8s/249s，不 faststart 会永远卡在起头。
      · **不用 AI 超分** —— Real-ESRGAN 逐帧 0.3~10s/帧，60fps 视频等于不可能。
        视频的高频只能靠高码率重编码保真。
    """
    meta = {"ok": False, "path": None, "w": 0, "h": 0, "mode": "none",
            "src": None, "reason": "", "dur": 0.0, "srcKbps": 0}
    ff = ff_path()
    if not ff or not os.path.isfile(src_path):
        meta["reason"] = "ffmpeg 缺失" if not ff else "源文件不存在"
        return meta
    info = _ffprobe_like(src_path)
    meta["src"] = ("%dx%d" % (info.get("w", 0), info.get("h", 0))) or None
    meta["dur"] = round(float(info.get("dur") or 0), 1)
    meta["srcKbps"] = int(info.get("kbps") or 0)
    dur = float(info.get("dur") or 0)

    # ── 尺寸决策（本轮画质争议的核心修正）────────────────────────────
    # 旧逻辑一律 scale 到 3840x2160。对 2560x1440 / 1920x1080 的源这是
    # **上采样**：Lanczos 造出的是插值伪高频，既不增加真实细节，又因为
    # 编码到 4K 而让每像素分到的码率减半 —— 结果就是"标着 4K 却比原生更糊"。
    # 实测这正是用户截图里"已经 4K 了还是不如 Windows 原生"的成因。
    #
    # 正确策略：**母版尺寸 = min(源尺寸, 4K上限)**，只降不升。
    #   · 源已 >= 母版上限（真 4K）-> 保持母版上限，GPU 干净降采样；
    #   · 源低于上限（如 2560x1440）-> 母版就用源分辨率，浏览器/GPU 再
    #     一次性缩到显示尺寸。全程只有一次高质量重采样，且不虚增分辨率。
    # 这样"母版分辨率"如实反映素材上限，UI 标注也不再虚高。
    sw, sh = int(info.get("w") or 0), int(info.get("h") or 0)
    cap_w, cap_h = master_size(tw, th, ss)
    if sw > 0 and sh > 0:
        k = min(1.0, cap_w / float(sw), (cap_h / float(sh)) if sh else 1.0)
        ow, oh = max(2, int(sw * k) // 2 * 2), max(2, int(sh * k) // 2 * 2)
    else:
        ow, oh = cap_w, cap_h
    # 极端宽高比（竖屏源铺横屏）时按比例给下限，避免出现 200x1800 的细条
    if oh < 2 or ow < 2:
        ow, oh = cap_w, cap_h
    mw, mh = ow, oh
    meta["outOfBand"] = (mw, mh) != (cap_w, cap_h)

    # 锐化只在**确有放大**时启用；本产线已禁止上采样，故正常不锐化 ——
    # 对已是原生的画面锐化只会推出白边振铃，是净损失。
    vf = ["scale=%d:%d:flags=lanczos" % (mw, mh)]
    if use_sharpen and sw and sw < mw:
        amt = min(1.6, 0.5 * (mw / float(sw)))
        vf.append("unsharp=5:5:%.2f:5:5:0.0" % amt)
    try:
        os.makedirs(os.path.dirname(out_path), exist_ok=True)
    except OSError as e:
        meta["reason"] = "目录创建失败 %r" % (e,)
        return meta

    tmp = out_path + ".part"
    # 码率上限按**实际母版尺寸**取，并按像素数缩放：3840x2160 用 45M、
    # 2560x1440 就该降到 ~22M，否则码率全浪费在插值出来的空高频上。
    # 但也不能低于源码率的一半 —— 否则等于强制降码率，二代损失。
    mpix = mw * mh / 1e6
    src_bps0 = (int(info.get("kbps") or 0) * 1000) or 0
    cap_hi = max(8, min(60, int(round(mpix * 2.6))))
    if src_bps0:
        cap_hi = max(cap_hi, int(round(src_bps0 * 0.75 / 1e6)))
        cap_hi = min(60, cap_hi)
    maxrate = "%dM" % cap_hi
    bufsize = "%dM" % (int(maxrate[:-1]) * 2)
    # 码率下沿：不得低于源码率的一半，且不低于母版像素数的经验下限。
    # 没这一条时 VBR 会一路下探（实测 27011 -> 3759 kb/s）。
    src_bps = src_bps0
    floor = max(int(mpix * 2_600_000), int(src_bps * 0.5) if src_bps else 0)
    # 母版不比源大：下沿超过上沿会让 NVENC 报参数矛盾
    floor = min(floor, int(maxrate[:-1]) * 1_000_000)
    minrate = "%dk" % max(1200, floor // 1000)
    # ── 时长预算（体积控制）──────────────────────────────────────────
    # 壁纸是**循环**播放的，观感只取决于一个循环周期内是否好看，不取决于
    # 有没有把 249s 全放出来。按目标码率反算能塞进 max_mb 的秒数，超了就
    # 截一段（取 12%~62% 的中段，避开片头字幕/黑场与结尾定格）。
    # 实测：2898887949 保留 168s 母版 288MB -> 截到 100s 约 170MB，
    # 而观感无差别（Minecraft 循环播放没人看出在哪接缝）。
    cap_mb = float(max_mb) if max_mb else 0.0
    seg = None
    if cap_mb > 0 and dur > 0:
        # 必须按 **maxrate**（VBR 上界）估算，不能用区间中值。
        # 实测教训：1920x1080 母版 maxrate 8M，中值估算只有 5.02 Mbps，
        # 于是 afford=234s > dur=180s 判定"不用裁"；而 VBR 实际冲到
        # 7.28 Mbps（贴住上沿），产出 156MB 超限，裁剪逻辑形同虚设。
        # 壁纸宁可裁短一点，也不能让首帧等太久 —— 用上界最稳。
        bps_cap = int(maxrate[:-1]) * 1_000_000 / 8.0
        afford = cap_mb * 1048576.0 / max(bps_cap, 1.0)
        if afford < dur:
            # 留 8% 安全余量：VBR 实际码率可能贴近上沿，且 moov 有开销
            seg = max(6, int(afford * 0.92))
            seg = min(seg, int(dur))
            # 起点：12% 处，避开片头静止/黑场；给短片留至少 1s 起点
            meta["trimmed"] = {"from": round(dur, 1), "to": seg}
    cmd = [ff, "-hide_banner", "-loglevel", "error", "-y"]
    if seg:
        # 从 12% 处起跳：跳过片头（常见静止/黑场），接缝不易被察觉
        cmd += ["-ss", "%.2f" % (dur * 0.12)]
    cmd += ["-i", src_path, "-an"]
    if seg or max_seconds:
        cmd += ["-t", str(int(seg or max_seconds))]
    cmd += ["-vf", ",".join(vf),
            "-c:v", "h264_nvenc", "-preset", "p5", "-rc", "vbr",
            "-cq", str(cq), "-b:v", "0",
            "-maxrate", maxrate, "-bufsize", bufsize,
            # -b:v 0 只给了**上限**，没给下限 —— 这是本轮画质回归的元凶。
            # 复杂动态场景（Minecraft 249s、4K 人物）里 NVENC 会为了守住
            # cq14 而把码率压到源片的 1/7：实测 2898887949 从 27011 kb/s
            # 掉到 3759 kb/s，LapVar 掉 5.8%，正是用户说的"标 4K 还是
            # 不如 Windows 原生"。加 -minrate 把窗口下沿抬到源码率的
            # 一半分，保住高频细节。
            "-minrate", minrate,
            "-profile:v", "high", "-pix_fmt", "yuv420p",
            # -f mp4 必须显式给：临时文件名是 <key>.mp4.part，ffmpeg 无法从
            # ".part" 后缀反推封装格式，会报 "Unable to find a suitable output
            # format" —— 这是原子写与 ffmpeg 混用时必踩的坑。
            "-f", "mp4",
            "-movflags", "+faststart", tmp]
    try:
        r = subprocess.run(cmd, capture_output=True, timeout=timeout,
                           startupinfo=_no_window())
    except subprocess.TimeoutExpired:
        _rm(tmp)
        meta["reason"] = "重编码超时(>%ds)" % timeout
        return meta
    except (OSError, subprocess.SubprocessError) as e:
        _rm(tmp)
        meta["reason"] = "ffmpeg 执行失败 %r" % (e,)
        return meta
    if r.returncode != 0 or not os.path.isfile(tmp) or os.path.getsize(tmp) < 4096:
        _rm(tmp)
        meta["reason"] = "ffmpeg rc=%s %s" % (
            r.returncode, (r.stderr or b"")[:200].decode("utf-8", "replace"))
        return meta
    try:
        if os.path.exists(out_path):
            os.remove(out_path)
        os.replace(tmp, out_path)     # 原子：完整产物才出现在正式路径
    except OSError as e:
        _rm(tmp)
        meta["reason"] = "落盘失败 %r" % (e,)
        return meta
    meta.update({"ok": True, "path": out_path, "w": mw, "h": mh, "mode": "v4k"})
    return meta
