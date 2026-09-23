import os, sys, datetime, traceback, subprocess, ctypes, ctypes.wintypes as wt

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-ldmdiag.rpt"
def log(m=""):
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(str(m) + "\n")

try:
    log("== LDM/dynamic-disk diag " + datetime.datetime.now().isoformat())

    def run(cmd, timeout=120):
        try:
            r = subprocess.run(cmd, capture_output=True, text=True, errors="replace", timeout=timeout)
            return (r.stdout or "") + (("\nERR:" + r.stderr) if r.stderr.strip() else "")
        except Exception as ex:
            return "EXC " + repr(ex)

    # 1. disk/partition layout via PowerShell CIM
    ps = (
        "Get-Disk | Format-List Number,FriendlyName,PartitionStyle,Guid,IsBoot,IsSystem,DiskId,IsReadOnly,OperationalStatus | Out-String; "
        "Get-Partition | Format-Table DiskNumber,PartitionNumber,DriveLetter,@{n='GB';e={[math]::Round($_.Size/1GB,1)}},GptType,Guid -AutoSize | Out-String"
    )
    out = run(["powershell", "-NoProfile", "-Command", ps], 180)
    log("-- Get-Disk/Get-Partition --\n" + out[:4000])

    # 2. dynamic disk markers via diskpart script (read-only: list disk shows Dyn)
    script = os.path.join(os.environ.get("TEMP", "."), "vx_dp.txt")
    with open(script, "w") as f:
        f.write("list disk\nlist volume\n")
    r = run(["diskpart", "/s", script], 180)
    log("-- diskpart list --\n" + r[:3000])

    # 3. BCD osdevice raw elements (store on VARIX-ESP)
    k32 = ctypes.WinDLL("kernel32")
    Y = None
    for L in "CDEFGHIJKLMNOPQRSTUVWXYZ":
        p = L + ":\\"
        nm = ctypes.create_unicode_buffer(300)
        if k32.GetVolumeInformationW(p, nm, 300, None, None, None, None, 0) and nm.value == "VARIX-ESP":
            Y = L + ":"
    log("Y(ESP)=" + str(Y))
    if Y:
        bcd = Y + "\\Boot\\BCD"
        out = run(["bcdedit", "/store", bcd, "/enum", "{default}", "/v"], 60)
        log("-- BCD default --\n" + out[:2500])
    log("done")
except Exception:
    log("EXC " + traceback.format_exc()[:1200])
    log("done")
