# -*- coding: utf-8 -*-
"""从 与你相恋到生命尽头.mp4 抽帧，挑与用户截图同景的一帧，转 RGB565 嵌入体。

产出：
  _attic/wallpaper-src/frame-candidates/*.png   候选帧
  _attic/wallpaper-src/wallpaper-rgb565.bin     1280x720 RGB565（内核嵌入）
  _attic/wallpaper-src/wallpaper-preview.png    预览（人工核对用）
"""
import imageio.v3 as iio
from PIL import Image
import os

VID = r"W:\SteamLibrary\steamapps\workshop\content\431960\3743127930\与你相恋到生命尽头.mp4"
OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\wallpaper-src"
CAND = os.path.join(OUT, "frame-candidates")
os.makedirs(CAND, exist_ok=True)

meta = iio.immeta(VID, plugin="pyav") if False else None
frames_at = [0, 1, 2, 4, 6, 8, 10, 14, 18, 24, 30, 40, 55, 70, 90, 120]
n = 0
for idx, frame in enumerate(iio.imiter(VID, plugin="pyav")):
    if idx in frames_at:
        Image.fromarray(frame).save(os.path.join(CAND, "f%03d.png" % idx))
        print("saved frame", idx, frame.shape)
        n += 1
    if idx > max(frames_at):
        break
print("candidates:", n)
