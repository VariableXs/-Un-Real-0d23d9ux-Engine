"""stdlib-only Windows input control. Usage:
  inp.py click X Y | dclick X Y | rclick X Y | move X Y | scroll N [X Y] | key "ctrl+s" | text "hello" | drag X1 Y1 X2 Y2
"""
import ctypes, sys, time
user32 = ctypes.windll.user32
user32.SetProcessDPIAware()
# screenshots are viewed downscaled to 1080 wide; scale coords accordingly
SX = user32.GetSystemMetrics(0)/1080.0
SY = user32.GetSystemMetrics(1)/610.0
def S(x,y): return (int(x*SX), int(y*SY))

MOUSEEVENTF = {"move":0, "leftdown":2, "leftup":4, "rightdown":8, "rightup":16, "wheel":0x800, "abs":0x8000}

def set_cursor(x,y): x,y=S(x,y); user32.SetCursorPos(x,y)

def click(x,y,dbl=False,right=False):
    set_cursor(x,y); time.sleep(0.08)
    f_dn,f_up=(8,16) if right else (2,4)
    user32.mouse_event(f_dn,0,0,0,0); user32.mouse_event(f_up,0,0,0,0)
    if dbl:
        time.sleep(0.06)
        user32.mouse_event(f_dn,0,0,0,0); user32.mouse_event(f_up,0,0,0,0)

def drag(x1,y1,x2,y2):
    set_cursor(x1,y1); time.sleep(0.1)
    user32.mouse_event(2,0,0,0,0); time.sleep(0.1)
    steps=20
    for i in range(1,steps+1):
        set_cursor(x1+(x2-x1)*i/steps, y1+(y2-y1)*i/steps); time.sleep(0.01)
    time.sleep(0.1); user32.mouse_event(4,0,0,0,0)

VK={ "back":8,"tab":9,"enter":13,"shift":16,"ctrl":17,"alt":18,"esc":27,"space":32,
     "end":35,"home":36,"left":37,"up":38,"right":39,"down":40,"del":46,
     "win":91,"f4":115,"f5":116}

def key(combo):
    parts=[p.lower() for p in combo.split("+")]
    mods=[p for p in parts if p in ("ctrl","alt","shift","win")]
    main=parts[-1]
    for m in mods:
        user32.keybd_event(VK[m],0,0,0); time.sleep(0.03)
    if len(main)==1:
        c=main.upper()
        vk=user32.VkKeyScanW(ord(c))
        if vk!=-1:
            vk=vk&0xFF
            sh=user32.VkKeyScanW(ord(c))>>8
            if sh&1: user32.keybd_event(16,0,0,0)
            user32.keybd_event(vk,0,0,0); user32.keybd_event(vk,0,2,0)
            if sh&1: user32.keybd_event(16,0,2,0)
    elif main.isdigit():
        user32.keybd_event(0x30+int(main),0,0,0); user32.keybd_event(0x30+int(main),0,2,0)
    elif main in VK:
        user32.keybd_event(VK[main],0,0,0); user32.keybd_event(VK[main],0,2,0)
    for m in reversed(mods):
        user32.keybd_event(VK[m],0,2,0)

def type_text(s):
    import ctypes as c
    for ch in s:
        if ch=="\n":
            key("enter"); continue
        arr=(c.c_ushort*1)(ord(ch))
        # use SendInput unicode
        class KEYBDINPUT(c.Structure):
            _fields_=[("wVk",c.c_ushort),("wScan",c.c_ushort),("dwFlags",c.c_uint),
                      ("time",c.c_uint),("dwExtraInfo",c.POINTER(c.c_ulong))]
        class INPUT(c.Structure):
            class _U(c.Union): _fields_=[("ki",KEYBDINPUT),("pad",c.c_ulong*8)]
            _anonymous_=("u",); _fields_=[("type",c.c_uint),("u",_U)]
        def send(flags,wscan):
            i=INPUT(); i.type=1; i.ki.wVk=0; i.ki.wScan=wscan; i.ki.dwFlags=flags
            user32.SendInput(1,c.byref(i),c.sizeof(INPUT))
        send(4,ord(ch)); send(2|4,ord(ch)); time.sleep(0.01)

if __name__=="__main__":
    cmd=sys.argv[1].lower(); a=[int(v) for v in sys.argv[2:] if v.lstrip("-").isdigit()]
    if cmd=="click": click(a[0],a[1])
    elif cmd=="dclick": click(a[0],a[1],dbl=True)
    elif cmd=="rclick": click(a[0],a[1],right=True)
    elif cmd=="move": set_cursor(a[0],a[1])
    elif cmd=="drag": drag(*a[:4])
    elif cmd=="scroll":
        x,y=S(a[1],a[2]) if len(a)>2 else (user32.GetSystemMetrics(0)//2, user32.GetSystemMetrics(1)//2)
        set_cursor(x,y); user32.mouse_event(0x800,0,0,int(a[0])*120,0)
    elif cmd=="key": key(sys.argv[2])
    elif cmd=="text": type_text(sys.argv[2].replace("\\n","\n"))
    print("ok",cmd)
