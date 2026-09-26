# AI-09 文件操作组 完成报告

**交付范围**：Z-29…Z-35、M-19…M-27 共 16 项文件操作与文件管理器能力
**日期**：2026-09-08

## 一、交付清单

### 文件操作四工具（VWM 窗口 + 开始菜单入口）
| 任务 | 说明 | 实现 |
|---|---|---|
| Z-32 | 批量重命名（预览/冲突检测/可撤销） | `src/system/tools/RenameApp.tsx` + 后端 `batch_rename_preview/apply/undo` |
| Z-33 | 重复文件报告（只读，三级确认） | `src/system/tools/DupeApp.tsx` + 后端 `dupe_scan`（大小→抽样哈希→全量） |
| Z-34 | 空间分析（目录树占用） | `src/system/tools/SpaceApp.tsx` + 后端 `space_scan` |
| M-21 | 校验和（MD5/SHA-1/SHA-256/BLAKE3） | `src/system/tools/ChecksumApp.tsx` + 后端 `checksum/cancel`（进度事件流） |

### 文件管理器增强（ExplorerWindow）
| 任务 | 说明 |
|---|---|
| Z-31 | 侧栏网络驱动器分组（在线检测 + 不可用态） |
| Z-35 | 发送到（系统 + 自定义目录，复制语义） |
| Z-29 | 快速预览格式扩展（压缩包 zip/7z/tar 浏览） |
| M-22 | 预览键导航（← → 切换文件） |
| M-20 | 批量选择（Ctrl+Click）+ 多选汇总（N 项 · 总大小） |
| M-24 | 书签条（当前路径父级面包屑） |
| M-25 | 文件占用查看（Restart Manager，内核级占用如实提示） |
| M-26 | 删除档位（环境回收站/每次询问/直删，默认=现状） |
| M-19 | 右键菜单自定义（显隐+排序注册表，仅注册表内安全动作） |

### 设置页与后台
| 任务 | 说明 |
|---|---|
| Z-30 | 打开方式管理（扩展名→第三方应用登记） |
| M-27 | 目录监控哨兵（notify 上限 5 目录，事件推送通知） |
| M-19 | 设置页「文件管理器」tab：`src/features/settings/FilesTab.tsx` |

### 后端
`src-tauri/src/shell/fileops.rs`：全部命令实现 + 命令注册（lib.rs）+ Cargo 依赖（md5/sha1/blake3/zip 等）。
`src/lib/ipc.ts`：全部 IPC 包装与类型定义。

## 二、验证结果

- **cargo test fileops**：13/13 通过（哈希已知向量、重命名规则管线、哨兵去重、zip-slip 防御、去重抽样边界、免打扰跨午夜）
- **vitest**：ctxMenu.test.ts 7/7、vwm.test.ts 3/3 通过
- **tsc**：AI09 相关文件零错误（仓库中其余错误属其他 AI 模块，未触碰）
- **cargo check**：通过（仅既有 warning）
- 全量 cargo test 中 print/sysmaint/versions 等 6 个失败测试属其他 AI 模块，未处理

## 三、修复记录（本次收尾）
1. `md5_known_vector`：测试期望值笔误（31 位十六进制），修正为正确的 MD5("abc") = `900150983cd24fb0d6963f7d28e17f72`
2. `rename_number_rule`：pad=3 下期望值与 photo001.jpg 用例矛盾，修正为 `noext001`
3. `vwm.ts`：并发合并导致的 `focusVwmWin` 重复定义，保留 Z-36 置顶感知版本
4. `ctxMenu.test.ts`：`CtxConfig` 类型显式标注

## 四、已知限制（如实声明）
- 压缩包浏览仅支持 zip（7z/tar 受后端依赖限制，UI 如实提示）
- 重复文件报告只读不删除；释放空间需用户手动处理
- 文件占用查看依赖 Windows Restart Manager，内核级占用无法披露具体进程
- 哨兵不支持网络目录；删除档位「每次询问」在资源管理器删除动作时弹选择
- 恢复（undo）仅支持最近一次批量重命名
