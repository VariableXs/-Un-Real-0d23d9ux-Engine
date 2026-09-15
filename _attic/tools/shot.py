"""Capture screen and save as PNG using only stdlib (ctypes GDI + zlib)."""
import ctypes, sys, struct, zlib
from ctypes import wintypes

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

def capture_png(path, x=0, y=0, w=None, h=None):
    user32.SetProcessDPIAware()
    if w is None:
        w = user32.GetSystemMetrics(0)
        h = user32.GetSystemMetrics(1)
    hdc = user32.GetDC(0)
    mem = gdi32.CreateCompatibleDC(hdc)
    bmp = gdi32.CreateCompatibleBitmap(hdc, w, h)
    gdi32.SelectObject(mem, bmp)
    gdi32.BitBlt(mem, 0, 0, w, h, hdc, x, y, 0x00CC0020)
    class BMIH(ctypes.Structure):
        _fields_=[("biSize",wintypes.DWORD),("biWidth",ctypes.c_long),("biHeight",ctypes.c_long),
                  ("biPlanes",wintypes.WORD),("biBitCount",wintypes.WORD),("biCompression",wintypes.DWORD),
                  ("biSizeImage",wintypes.DWORD),("biXPelsPerMeter",ctypes.c_long),("biYPelsPerMeter",ctypes.c_long),
                  ("biClrUsed",wintypes.DWORD),("biClrImportant",wintypes.DWORD)]
    bmi=BMIH(); bmi.biSize=ctypes.sizeof(BMIH); bmi.biWidth=w; bmi.biHeight=-h
    bmi.biPlanes=1; bmi.biBitCount=32
    buf=ctypes.create_string_buffer(w*h*4)
    gdi32.GetDIBits(mem,bmp,0,h,buf,ctypes.byref(bmi),0)
    raw=buf.raw
    # build PNG raw scanlines: filter 0 per row, RGB (drop X)
    stride=w*4
    lines=bytearray()
    for row in range(h):
        s=row*stride
        line=bytearray(b"\x00")
        px=raw[s:s+stride]
        # BGRX -> RGB
        rgb=bytearray(stride//4*3)
        for i,j in zip(range(0,stride,4),range(0,len(rgb),3)):
            rgb[j]=px[i+2]; rgb[j+1]=px[i+1]; rgb[j+2]=px[i]
        line+=bytes(rgb)
        lines+=line
    def chunk(tag,data):
        c=struct.pack(">I",len(data))+tag+data
        return c+struct.pack(">I",zlib.crc32(tag+data)&0xffffffff)
    ihdr=struct.pack(">IIBBBBB",w,h,8,2,0,0,0)
    png=b"\x89PNG\r\n\x1a\n"+chunk(b"IHDR",ihdr)+chunk(b"IDAT",zlib.compress(bytes(lines),6))+chunk(b"IEND",b"")
    with open(path,"wb") as f: f.write(png)
    gdi32.DeleteObject(bmp); gdi32.DeleteDC(mem); user32.ReleaseDC(0,hdc)

if __name__=="__main__":
    path=sys.argv[1]
    args=[int(v) for v in sys.argv[2:6]] if len(sys.argv)>2 else []
    capture_png(path,*args)
    print("saved",path)
