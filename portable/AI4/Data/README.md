# AI-4 Data 目录模板

这是第 3.2 / 8.4 章 `D:\Data` 的目录树模板。实际部署时由 `Data-Init.ps1 -DataDrive D:` 在 U盘/Data 分区创建同一结构，内容如下：

```
D:\Data\
├── Apps\        绿色软件实体（C 盘仅符号链接）
├── MSIX\        MSIX/AppX 包 + Mount 子目录
├── Plugins\     用户插件 DLL（热加载）
├── User\        用户数据（云同步主体）
├── Exchange\    受控摆渡通道（Defender 强制扫描）
├── Cache\       可清理缓存（云同步排除 *.tmp）
├── Dumps\       崩溃 dmp
├── Config\      permissions.json / shortcuts.json / plugin-market.json
├── Env\         path.env（随盘环境变量）
├── Registry\    User.dat（合法最小 Hive, regf 头; Core/Config-Runtime reg load 挂载）
├── Backup\      User.vhdx 每日备份
├── Security\    BitLocker 恢复密钥（需另存离线）
├── Sync\        rclone.conf / 云同步配置
├── Benchmark\   压测 payload / 结果
└── Compat\      兼容回退日志
```

- `.gitkeep` 仅用于保留目录结构，部署时无需复制到 `D:\Data`。
- 脚本约定路径：`D:\Data`（可由 `-DataDrive` 改）。
- `Registry\User.dat` 由 `Data-Init.ps1` 用 `reg save` 生成**合法最小 Hive**（文件头 `regf`），`reg load` / `RegLoadKey` 可直接挂载；空的占位文件无法被挂载。
