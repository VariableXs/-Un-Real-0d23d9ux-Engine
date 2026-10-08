# -*- coding: utf-8 -*-
"""wetex.py —— Wallpaper Engine scene.pkg 原生素材提取器

为什么必须有这个模块（2026-10-03 实机取证）
------------------------------------------------------------------
用户实机截图里大量壁纸"很糊"，第一反应是母版超分不够强。实测才发现根因是
**源素材本身就只有几百像素**：

  1646702957「流浪地球」preview.jpg  600x600   scene.pkg 内 原生纹理 4096x2160
  3304033014「恋死」    preview.jpg 1024x1024  scene.pkg 内 原生纹理 3840x2160
  2688582860「极光」    preview.jpg 1024x1024  scene.pkg 内 原生纹理 2560x1440
  3799672625「午后闲暇」preview.gif  160x160   scene.pkg 内 原生纹理 4096x2160

preview.* 只是创意工坊给商店页看的缩略图，**不是壁纸的真实素材**。真实素材打包在
`scene.pkg` 里（Wallpaper Engine 的 PKGV 容器），以 `.tex` 形式存放。��此前所有
"把 600px preview 拉到 4K"的努力，本质是在放大缩略图 —— 再强的超分也补不回
不存在的细节。**改用 .tex 原生纹理作源，才是画质根治。**

PKGV 容器格式（实测逆向，字节级）
------------------------------------------------------------------
    off 0   : u32          前置字段（实测恒为 8）
    off 4   : char[8]      魔数 "PKGV" + 4 位版本号
                           （实测同批素材里有 0002/0011/0015/0019/0021/0024
                             等多个版本，目录表结构完全一致）
    off 12  : u32          条目数
    off 16  : 条目表，每条：
                 u32      文件名长度 N
                 char[N]  文件名（UTF-8，不含结尾 \\0）
                 u32      数据区偏移
                 u32      数据长度
    数据区起点 = 条目表结束处

注意：条目名长度 N 覆盖的是「路径去掉前缀目录后」的字节数（实测表现为名字左侧被
截断，例如 "materials/流浪地球全景.tex" 显示成 "…/流浪地球全景.tex" 前缀缺失）。
这只影响可读性，**不影响 offset/size 正确性**，故按长度原样读出即可。

TEX 容器格式（实测逆向，字节级）
------------------------------------------------------------------
    off 0   : char[9]     "TEXV0005\\0"
    off 9   : char[9]     "TEXI0001\\0"
    off 18  : u32         恒为 0
    off 22  : u32         格式标识（实测 2）
    off 26  : u32         纹理最大边长（实测 4096）
    off 30  : u32         深度/层数相关
    off 34  : u32         **width**（实测 2560 / 3840 / 4096）
    off 38  : u32         **height**（实测 1440 / 2160）
    off 42  : u32         校验/标志位
    off 46  : char[9]     "TEXB0003\\0"   数据块开始
    off 55  : u32         块版本（实测 1）
    off 59  : u32         块计数相关（实测 2）
    off 63  : u32         负载类型码。**实测不稳定，不可作为唯一判据**：
                           TEXB0003 块里 MP4 码为 1，
                           TEXB0004 块里同一份 MP4 码却为 0。
                           故代码改用 ftyp box 魔数嗅探来判别 MP4。
    off 67  : u32         块宽（与 @34 一致）
    off 71  : u32         块高（与 @38 一致）
    off 75  : u32         0
    off 79  : u32         0
    off 82  : u32         附加字段（含义随块版本变化）
    off 86± : 负载        JPEG/PNG 以 FF D8 / 89 50 4E 47 起始
                           MP4    以 [u32 size=32]['ftyp'] 起始（实测 87 或 91）
    之后    : 后续块（mipmap 小图），本次只取第一块满分辨率

**负载类型 1（MP4）是本模块最有价值的发现**
------------------------------------------------------------------
3373269635「Minecraft new Year night」和 3799672625「午后闲暇」这两个项目，
商店页 preview 只有 1024x1024 静图 / 160x160 GIF，但 scene.pkg 里存的是
**完整 MP4 视频**（实测前者 1920x1080 / 30fps / 3 分 00 秒 / 5925kb/s）。
这正是用户在 Windows 原生 Wallpaper Engine 里看到的"原生动态效果"的真实素材。
换用它们，动态壁纸才谈得上"原生"。

设计要点
------------------------------------------------------------------
* 纯只读解析，不修改 Steam 任何文件。
* 对未知像素格式做 JPEG/PNG 魔数嗅探兜底，不因格式码变化就整体失效。
* 提取物落地到可写缓存目录（exe 同级 `_wetexture`），开发态/打包态一致，
  避免每次启动都重新解包 8~136MB 的 .tex。
"""

import os
import struct

__all__ = ["read_pkgv", "tex_first_payload", "tex_first_image", "iter_tex",
           "best_native_source", "extract_project_textures", "TexError"]


class TexError(Exception):
    """PKGV / TEX 结构解析失败。"""


# ---------------------------------------------------------------- PKGV

# 魔数是 "PKGV" + 4 位版本号。实测同一批创意工坊里同时存在
# PKGV0002 / 0011 / 0015 / 0019 / 0021 / 0024 等多个版本，
# 目录表结构完全一致，故只校验前缀 "PKGV"。
PKGV_MAGIC = b"PKGV"
PKGV_PREFIX = b"PKGV"


def read_pkgv(path):
    """解析 scene.pkg，返回 (数据区起点, [(name, off, size), ...])。"""
    try:
        with open(path, "rb") as f:
            d = f.read()
    except OSError as e:
        raise TexError("scene.pkg 打不开: %r" % (e,))
    if len(d) < 16 or d[4:8] != PKGV_PREFIX:
        raise TexError("不是 PKGV 容器 (magic=%r)" % (d[4:12],))
    try:
        cnt = struct.unpack_from("<I", d, 12)[0]
    except struct.error:
        raise TexError("头部截断")
    if not (0 < cnt < 100000):
        raise TexError("条目数离谱: %r" % (cnt,))
    pos = 16
    out = []
    for _ in range(cnt):
        try:
            nl = struct.unpack_from("<I", d, pos)[0]
            pos += 4
            if nl > 1024:
                raise TexError("条目名长度离谱: %r" % (nl,))
            name = d[pos:pos + nl].decode("utf-8", "replace")
            pos += nl
            off, size = struct.unpack_from("<II", d, pos)
            pos += 8
        except struct.error:
            raise TexError("条目表截断于第 %d 项" % (len(out),))
        out.append((name, off, size))
    return pos, out


# ---------------------------------------------------------------- TEX

TEX_MAGICS = (b"\xff\xd8\xff", b"\x89PNG\r\n\x1a\n")
_TEX_HDR = 46          # "TEXV0005\0" + "TEXI0001\0" 之后
_TEX_BLOCK = 46        # "TEXB0003\0" 起点
_TEX_PAYLOAD = 86      # 负载区起点

# 负载类型（偏移 63）
TEX_PAYLOAD_MP4 = 1
TEX_PAYLOAD_JPEG = 4
TEX_PAYLOAD_JPEG_EXIF = 7

# MP4 的 ftyp box：4 字节大端长度(32) + 'ftyp'。实测起点恒为 87。
_FTYP_BOX = b"\x00\x00\x00\x20ftyp"


def tex_dims(blob):
    """从 TEX 负载头部读 (width, height)。解析失败返回 (0, 0)。"""
    if len(blob) < _TEX_PAYLOAD:
        return (0, 0)
    try:
        w = struct.unpack_from("<I", blob, 34)[0]
        h = struct.unpack_from("<I", blob, 38)[0]
    except struct.error:
        return (0, 0)
    # 合理性过滤：真实纹理尺寸不会超过 65536，也不会为 0
    if not (0 < w <= 65536 and 0 < h <= 65536):
        return (0, 0)
    return (w, h)


def _sniff_payload(blob, start=_TEX_PAYLOAD):
    """在负载区嗅探 JPEG/PNG 起点，返回 (magic, 绝对偏移)。找不到返回 (None, -1)。"""
    for mg in TEX_MAGICS:
        i = blob.find(mg, start, min(len(blob), start + 4096))
        if i >= 0:
            return (mg, i)
    return (None, -1)


def tex_first_payload(blob):
    """从 TEX 负载里取出第一块（满分辨率）内容。

    返回 dict: {"fmt","w","h","data","blockfmt"}；无法解析时返回 None。
    fmt 取值 "mp4" / "jpeg" / "png"。

    负载类型 1（MP4）走 ftyp box 定位；类型 4/7（JPEG）走 EOI 裁剪；
    未知类型则用魔数嗅探兜底。
    """
    if not blob or len(blob) < _TEX_PAYLOAD:
        return None
    # 头部版本可能是 TEXV0001..0005 等变体，实测结构一致；
    # 唯一可靠的判据是数据块头 "TEXB" 是否在位。
    if blob[_TEX_BLOCK:_TEX_BLOCK + 4] != b"TEXB":
        return None
    w, h = tex_dims(blob)
    try:
        blockfmt = struct.unpack_from("<I", blob, 63)[0]
    except struct.error:
        blockfmt = -1

    # --- 负载类型 1 / 0：完整 MP4 ---------------------------------------
    # 判据用 **ftyp box 魔数嗅探**，而不是硬编码负载类型码。实测同一个
    # "MP4 负载"在不同块版本下类型码并不一致：
    #   3373269635 的 TEXB0003 块：@63 = 1，ftyp 在 87
    #   3799672625 的 TEXB0004 块：@63 = 0，ftyp 在 91（多 4 字节标志）
    # 只认 @63==1 会漏掉后者，所以这里直接嗅探。
    off = blob.find(_FTYP_BOX, _TEX_PAYLOAD - 8, min(len(blob), 192))
    if off >= 0:
        return {"fmt": "mp4", "w": w, "h": h,
                "data": blob[off:], "blockfmt": blockfmt}

    # --- JPEG / PNG ------------------------------------------------------
    data = None
    mg, off = _sniff_payload(blob)
    if mg is not None:
        data = blob[off:]
        if mg == b"\xff\xd8\xff":
            # 尾部若是 mipmap 等其它块，裁到 EOI 为止
            eoi = data.rfind(b"\xff\xd9")
            if eoi > 0:
                data = data[:eoi + 2]
        kind = "jpeg"
    else:
        # 最后兜底：按声明长度切
        try:
            size = struct.unpack_from("<I", blob, 82)[0]
        except struct.error:
            size = 0
        if size > 0 and _TEX_PAYLOAD + size <= len(blob):
            cand = blob[_TEX_PAYLOAD:_TEX_PAYLOAD + size]
            if cand[:3] == b"\xff\xd8\xff":
                data, kind = cand, "jpeg"
            elif cand[:8] == b"\x89PNG\r\n\x1a\n":
                data, kind = cand, "png"
    if not data:
        return None
    return {"fmt": kind, "w": w, "h": h, "data": data, "blockfmt": blockfmt}


# 向后兼容别名（旧调用点用名 tex_first_image）
tex_first_image = tex_first_payload


def iter_tex(pkg_path):
    """遍历 scene.pkg 里所有 .tex 条目，产出 (name, size, blob)。"""
    data_off, entries = read_pkgv(pkg_path)
    try:
        with open(pkg_path, "rb") as f:
            d = f.read()
    except OSError as e:
        raise TexError("scene.pkg 重读失败: %r" % (e,))
    for name, off, size in entries:
        low = name.lower()
        if not (low.endswith(".tex") or low.endswith(".png") or low.endswith(".jpg")):
            continue
        s = data_off + off
        if s < 0 or s + size > len(d) or size <= 0:
            continue
        yield (name, size, d[s:s + size])


# ------------------------------------------------------- 挑选最佳原生源

# 明显不是"壁纸画面"的纹理：法线图/遮罩/占位图等。
_REJECT_SUB = ("/masks/", "/effects/", "mask", "normal", "placeholder",
               "dock_icon", "lense", "lens.", "vignette")
_REJECT_EXACT = ("default_dock_icon.tex", "opacity_mask.tex")


def _score(name, w, h, size, fmt="jpeg"):
    """给候选原生素材打分，分高者优先。

    **动态优先**：MP4 负载（真动画）分数加成最高 —— 用户要的是 Windows 原生
    那种动态效果，静帧再好也替代不了会动的原生视频。
    """
    low = name.lower()
    for bad in _REJECT_EXACT:
        if low.endswith(bad):
            return -1
    for bad in _REJECT_SUB:
        if bad in low:
            return -1
    if w < 1280 or h < 720:
        return -1
    px = w * h
    ar = w / float(h) if h else 0
    ar_bonus = 1.25 if 1.5 <= ar <= 2.1 else 1.0
    base = px * ar_bonus
    if fmt == "mp4":
        # 视频：像素量权重更高，且体积越大说明帧数越多/时长越长
        return base * 4.0 + size * 0.05
    return base + size * 0.001


def best_native_source(pkg_path):
    """在 scene.pkg 中挑出最适合当壁纸源的那份原生素材。

    返回 (name, info_dict) 或 (None, None)。info_dict 与 tex_first_payload() 同构，
    且额外带 "name" 字段。
    """
    best = None
    best_s = -1.0
    for name, size, blob in iter_tex(pkg_path):
        info = tex_first_payload(blob)
        if not info or not info.get("data"):
            continue
        w, h = info["w"], info["h"]
        if not w or not h:
            continue
        s = _score(name, w, h, size, info.get("fmt", "jpeg"))
        if s > best_s:
            best_s = s
            info["name"] = name
            best = info
    if best is None:
        return (None, None)
    return (best.get("name", ""), best)


def extract_project_textures(pdir, out_dir, verbose=False):
    """把一个 WE 项目的原生素材落盘到 out_dir，返回落地文件路径列表。

    只提取「最佳原生源」一份（壁纸主画面），避免把 136MB 帧序列整包解出来。
    落地文件名带尺寸与类型，便于人工核对与缓存失效判断。
    """
    pkg = os.path.join(pdir, "scene.pkg")
    if not os.path.isfile(pkg):
        return []
    name, info = best_native_source(pkg)
    if not info:
        return []
    os.makedirs(out_dir, exist_ok=True)
    ext = {"mp4": "mp4", "jpeg": "jpg", "png": "png"}.get(info["fmt"], "bin")
    pid = os.path.basename(pdir.rstrip("\\/"))
    out = os.path.join(out_dir, "%s_%dx%d.%s" % (pid, info["w"], info["h"], ext))
    try:
        if os.path.isfile(out) and os.path.getsize(out) == len(info["data"]):
            return [out]
        tmp = out + ".part"
        with open(tmp, "wb") as f:
            f.write(info["data"])
        os.replace(tmp, out)
    except OSError as e:
        if verbose:
            print("[wetex] 落地失败 %s: %r" % (out, e))
        return []
    if verbose:
        print("[wetex] %s <- %s (%dx%d %s, %.1fMB)"
              % (os.path.basename(out), name, info["w"], info["h"], info["fmt"],
                 len(info["data"]) / 1048576.0))
    return [out]
