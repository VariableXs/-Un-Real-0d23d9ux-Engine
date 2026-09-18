#!/usr/bin/env python3
"""追加 2026-09-18 上午波次记忆（三大根因 + 视觉验收）"""
import io

MEM = r"""

## 2026-09-18 上午（第2波 37/41/48 + 视觉验收）

### 任务37（vfsguard 热更新仲裁）——内核戒律双实证
- RuleBook/Arbiter trait/deny 语义/冲突矩阵 10 组/热更新竞态，vfsguard 14 测试绿。
- **戒律实证①**：探针 4×RuleSet（各≈8.5KiB）栈变量=34KiB 打穿内核栈 → #DF@memset（HMP xp 栈回溯定位）。
- **戒律实证②**：改 Box 又踩 **kheap MAX_ALLOC=4KiB**（mem/heap.rs:25，单次分配上限）→ alloc panic 静默 halt（与 kvsrv 同款）。
- **最终合规**：static .bss SpinProtected<ProbeScratch> 单例（const 初始化）+ RuleBook::reload 原地 clear+parse 零栈物化。
- **实测顺序**：探针大对象放置优先级 = 栈(≤64KB) → 堆(≤4KiB/次) → static .bss（>4KiB 大物化唯一合法去处）。

### 任务48（picflow）——戒律三禁齐触
- FrameChannel 4×Frame(160KB)=640KB + [0u8;FRAME_MAX] 全在探针栈上 → 栈踩页表 → framebuffer 映射失效（put_byte 写 fb 页 fault，cr2 恒定=0xfd3e5800）。
- 修复：PIC_SCRATCH static .bss（ch+full+win 全进），实机全链 PASS（locked→filled→push1→blit1→verdict，surface=1280x800 full/win blit 全 true）。

### 任务41（notepad）——生成器回填顺序 bug
- 实机 iretq→user 后 #GP（rip 非 canonical）。HMP 现场 R13=entry+7。
- llvm-objdump -d notepad.pe → **所有 call *(%rip)/mov (%rip) rel32 全 0**。
- 根因：make-pe-notepad.py 回填 relocs 写 img 在 `img[TEXT_RAW:]=code` **之前** → 被拷贝整体冲掉。调序修复。
- **教训**：手写 PE 生成器必须 `llvm-objdump -d` 静态核对机器码后再上实机（一条命令省两轮 QEMU 排障）。

### 视觉验收（V 线 GUI，agent-browser + dev Tauri mock）
- 绕过 OOBE：`--init-script` 注入 `__TAURI_INTERNALS__` mock（get_all_settings oobeDone="1" + app_bootstrap + boot_replay 事件回放推满进度 + plugin:event|listen 返回 id）；stub 检测位 `__variableDevStub` 不设 → isTauriRuntime()=true。
- 开机剧场 100% 后 ESC 跳过（<30% 拒绝跳过）。设置打开 = Ctrl+,。
- 白名单管理页：渲染✓ 添加/删除交互✓；越权审计页：审计流捕获增删✓ 路径筛选✓；安全工作台：渲染✓。截图 5 张归档 docs/acceptance/2026-09-18-视觉验收-白名单UI与审计页/。
- **坑**：agent-browser open 阻塞（HMR 长连接）→ 后台跑+另会话 eval；CLI 单实例锁（并发命令挂死）；eval 引号嵌套用 JSON.stringify。

### 门禁基线
- tsc 0 错；vitest 2730；后端 275 passed；ktest 2976（vfsguard 14 + winsrv 7 + picflow 5 新增）；kcheck 0。
- 管道退出码坑再现：`cargo kbuild | grep -c error && ...` grep 0 匹配 exit 1 会断链/或 tail 吞掉 kbuild 失败——**长链一律 set -o pipefail**。
"""
p = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\.workbuddy\memory\2026-09-17.md"
with io.open(p, "a", encoding="utf-8", newline="\n") as f:
    f.write(MEM)
print("memory appended")
