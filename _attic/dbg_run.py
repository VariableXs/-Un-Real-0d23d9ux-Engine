import subprocess, sys
PS = "powershell.exe"
args = [PS, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"] + sys.argv[1:]
r = subprocess.run(args, capture_output=True)
open("_attic/dbg_out.txt", "w", encoding="utf-8").write(
    "STDOUT:\n" + r.stdout.decode("utf-8", "replace") + "\nSTDERR:\n" + r.stderr.decode("utf-8", "replace"))
