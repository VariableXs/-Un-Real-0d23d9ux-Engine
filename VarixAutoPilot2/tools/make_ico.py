"""生成 Windows .ico（多尺寸，手写 ICO 容器格式）。

为什么不用 Pillow 的 `append_images`：
实测它只把第一个尺寸写进 .ico（`im.info['sizes']` 仅 `[(16,16)]`），
因为 ICO 容器格式与 PNG 流的拼接规则不被 Pillow 正确处理。
而 Tauri 的 Windows 资源编译需要**真正的多尺寸 .ico**。

ICO 容器结构（简化，只用 PNG 压缩块，Vista+ 全部支持）：
  ICONDIR (6B)  : reserved(2)=0 type(2)=1 count(2)=N
  ICONDIRENTRY (16B × N): width(1) height(1) colorCount(1) reserved(1)
                         planes(2) bitCount(2) bytesInRes(4) imageOffset(4)
  图片数据       : 每张为完整 PNG 文件流（width/height=0 表示 256）

width/height 字段是**单字节**：0 → 256，n → n。
"""

import io
import sys
from pathlib import Path

from PIL import Image

SIZES = [16, 24, 32, 48, 64, 128, 256]


def build_ico(sources: dict[int, Path], out: Path) -> list[int]:
    """sources: {尺寸: 源图路径}。返回实际写入的尺寸列表。"""
    entries = []          # (w, h, png_bytes)
    for s in SIZES:
        src = sources.get(s) or sources.get(max(k for k in sources if k >= s))
        im = Image.open(src).convert("RGBA")
        # 4K 素材缩小到目标尺寸用 LANCZOS，保证边缘不糊
        im = im.resize((s, s), Image.LANCZOS)
        buf = io.BytesIO()
        im.save(buf, format="PNG", optimize=True)
        entries.append((s, s, buf.getvalue()))

    n = len(entries)
    # ICONDIR
    icondir = b"\x00\x00" + (1).to_bytes(2, "little") + n.to_bytes(2, "little")
    # 目录项（16 字节/项）
    offset = 6 + 16 * n
    dir_bytes = b""
    for (w, h, data) in entries:
        dir_bytes += bytes([
            0 if w >= 256 else w,
            0 if h >= 256 else h,
            0,        # colorCount：0 = 真彩
            0,        # reserved
        ])
        dir_bytes += (1).to_bytes(2, "little")   # planes
        dir_bytes += (32).to_bytes(2, "little")  # bitCount
        dir_bytes += len(data).to_bytes(4, "little")
        dir_bytes += offset.to_bytes(4, "little")
        offset += len(data)

    out.write_bytes(icondir + dir_bytes + b"".join(d for (_, _, d) in entries))
    return [w for (w, _, _) in entries]


if __name__ == "__main__":
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    icons = root / "src-tauri" / "icons"
    sources = {256: icons / "icon256.png", 512: icons / "icon512.png"}
    sizes = build_ico(sources, icons / "icon.ico")

    # 自检：读回确认多尺寸真的进去了
    im = Image.open(icons / "icon.ico")
    got = sorted(im.info.get("sizes", []))
    print(f"icon.ico 写入尺寸: {sizes}")
    print(f"读回尺寸: {got}")
    if len(got) < 5:
        print("✗ 多尺寸未生效")
        sys.exit(1)
    print("✓ 多尺寸 ICO 已生成")
