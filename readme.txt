# VX11 云端通道备忘

- 云端主机：AutoDL 4090D（ssh -p 通道见 _tools/vx11_ssh.py）
- probe 工程：/root/varix-work/probe（main.rs + checks.rs + mods/）
- 每卡流程：本地写 8 文件 → uploadcard → regen → cargo check/run/test
- 工具：vx11_ssh.py（密码走 VX11_SSH_PASS 环境变量）
- regen 脚本：vx11_regen_main.sh（扫 mods/ 生成 main.rs）→ 上传为 /root/varix-work/regen.sh
