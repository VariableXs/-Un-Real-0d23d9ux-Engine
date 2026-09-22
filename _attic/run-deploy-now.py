# 部署脚本语法预验 + 发起部署（BOM→预检→提权→轮询，esp-deploy-switch.py 已有完整流程）
import subprocess, sys
r = subprocess.run([sys.executable, r"_attic\esp-deploy-switch.py"], capture_output=True)
out = (r.stdout or b"") + (r.stderr or b"")
print(out.decode("gbk", "replace"))
