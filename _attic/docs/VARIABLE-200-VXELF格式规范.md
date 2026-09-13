# VARIABLE-200 · VXELF 可执行格式规范（一页纸）

> AI-02 交付物 · 对应 **F026 / F027**
> 落点：`kernel/varix/src/exec.rs`（格式定义与加载器）
> 状态：格式版本 `1`，ABI 版本 `1`
>
> VXELF = **V**arix e**X**ecutable + ELF 结构。保留 ELF 的「头 + 程序头表 + 段」
> 三段式（工具链改造量最小），但换掉魔数、丢掉动态链接的复杂度、
> 把 W^X 与 ASLR 写进格式而不是写进策略。

---

## 1. 字节序与对齐

| 项 | 值 |
| --- | --- |
| 字节序 | 小端（唯一支持） |
| 指针宽度 | 64 位 |
| 页大小 | `4096` |
| 文件/内存对齐 | 段内 `offset`、`vaddr`、`align` 三者同余 |

---

## 2. 文件头（`VxHeader`，固定 64 字节）

| 偏移 | 大小 | 字段 | 说明 |
| --- | --- | --- | --- |
| `0x00` | 4 | `magic` | `7F 56 58 45` = `\x7fVXE`，**与 ELF 的 `\x7fELF` 不同** |
| `0x04` | 2 | `version` | 格式版本，当前 `1` |
| `0x06` | 2 | `abi_version` | 目标 ABI 版本，当前 `1` |
| `0x08` | 8 | `entry` | 入口虚地址（必须落在某个 `PT_LOAD` 的可执行范围内） |
| `0x10` | 8 | `phoff` | 程序头表文件偏移（必须 ≥ `64`） |
| `0x18` | 2 | `phnum` | 程序头数量（`1..=8`） |
| `0x1A` | 2 | `_pad` | 置零 |
| `0x1C` | 4 | `_pad` | 置零 |
| `0x20` | 8 | `image_len` | 全文件长度，用于越界校验 |
| `0x28` | 8 | `flags` | `bit0` = 已签名（F037） |
| `0x30` | 8 | `preferred_base` | ASLR 熵为 0 时的回退基址 |
| `0x38` | 8 | `_pad` | 置零 |

`magic` 不同是刻意的：**Linux 加载器不会误判我们的镜像，我们也不会误判它的**。
校验失败即 `ENOEXEC`（等价），不做任何"尽力而为"的猜测。

---

## 3. 程序头（`VxPhdr`，固定 56 字节）

| 偏移 | 大小 | 字段 | 说明 |
| --- | --- | --- | --- |
| `0x00` | 4 | `p_type` | 段类型 |
| `0x04` | 4 | `flags` | `PF_R=4` · `PF_W=2` · `PF_X=1` |
| `0x08` | 8 | `offset` | 文件偏移（页对齐） |
| `0x10` | 8 | `vaddr` | 目标虚地址（页对齐） |
| `0x18` | 8 | `filesz` | 文件内字节数 |
| `0x20` | 8 | `memsz` | 内存字节数（≥ `filesz`，差额零填充） |
| `0x28` | 8 | `align` | 对齐要求（0 或 2 的幂） |

### 段类型

| 值 | 名称 | 处理 |
| --- | --- | --- |
| `1` | `PT_LOAD` | 加载；权限与 `flags` 一致 |
| `7` | `PT_TLS` | 建立 TLS 模板（F033），`vaddr` 指向模板像 |
| `0x6474E551` | `PT_GNU_STACK` | 声明栈需求；**若带 `PF_X` 则整镜像被拒** |
| `2` | `PT_DYNAMIC` | 自修复重定位（F034）允许；否则拒绝 |
| `3` | `PT_INTERP` | **明确拒绝**（`ENOTSUP`，无动态链接器） |
| `4` | `PT_NOTE` | 忽略 |

---

## 4. 不可协商的纪律

### 4.1 W^X（F029）

映射时刻强制：**没有任何页面可以同时 `PF_W` 与 `PF_X`**。
违规即 `EPERM`，并且**在写入任何页表项之前**返回。
`PT_GNU_STACK` 带 `PF_X` 同样被拒（不留"可执行栈"这个后门）。

### 4.2 静态链接优先（F034）

`classify_linkage` 只产出三种结论：

| 结论 | 条件 | 处理 |
| --- | --- | --- |
| `Static` | 无 `PT_DYNAMIC` / `PT_INTERP` | 直接加载 |
| `SelfFixing` | 仅 `PT_DYNAMIC`（自修复重定位） | 允许，用自带 `Rela` 表修补 |
| `Dynamic` | 存在 `PT_INTERP` | `ENOTSUP` |

### 4.3 ASLR（F030）

熵来自 `splitmix64`，**熵为 0 即完全可复现**（测试与验收依赖这一点）。

- 熵为 0：基址 = `preferred_base`（若它 ≥ 段最低链接地址），否则用链接地址；
- 熵 > 0：`slide = aslr_base - link_lo`，所有段整体平移。

栈顶同样由 `aslr_stack_top` 给出；`entropy = 0` 时栈顶固定。

### 4.4 失败全回收（F039）

`LoadTransaction` 记录已落地的每一项，任一阶段失败即按逆序回滚：
段 → 页 → 地址空间。**不允许留下半加载的镜像**。

---

## 5. 入口栈（F031）

SysV 形状，自 `rsp` 向上：

```
rsp+0x00   argc
rsp+0x08   argv[0] .. argv[argc-1]
           NULL
           envp[0] .. envp[n-1]
           NULL
           auxv[0].key, auxv[0].val, ...
           AT_NULL, 0
```

对齐规则：`auxv` 必须整体 16 字节对齐；字符串池自栈顶向下生长，
因此 `argv[0] > argv[1] > ...`（自检断言这一严格递减）。
`AT_RANDOM` 指向一块 16 字节的随机数据，紧邻字符串池之后。

## 6. auxv 完整性（F032）

六个必需项缺一不可（自检逐项断言）：`AT_ENTRY` `AT_PHDR` `AT_PHNUM`
`AT_PAGESZ` `AT_RANDOM` `AT_EXECFN`。其余：`AT_BASE` `AT_FLAGS` `AT_UID`
`AT_HWCAP` `AT_SECURE`。

---

## 7. 加载判定顺序（`plan_load`）

```
1. 头校验（魔数/版本/ABI/image_len/phoff/phnum）  → BadMagic/BadVersion/TooManySegments/...
2. 段表校验（对齐/大小/wx/重叠）                   → BadAlign/BadSize/WxViolation/SegmentOverlap
3. 链接方式分类（F034）                            → DynamicUnsupported
4. 基址与 slide（F030）
5. 布局（每段页对齐、零填充 memsz-filesz）         → NoSpace
6. TLS 模板（F033）                                → BadTls
7. 产出 LoadPlan（entry/stack_top/tls/auxv_entry）
```

每一步失败都映射到一个 `LoadError` 变体，`exit_code()` 统一为 **126**
（"找到了但无法执行"），与 `EACCES=126` 的 shell 约定一致。

---

## 8. 首个用户程序（F050）

`HELLO_IMAGE` 是一个手工构造的最小 VXELF 镜像，代码只有 12 字节：

```asm
mov eax, 2        ; SYS_WRITE
mov edi, 1        ; FD_STDOUT
syscall
```

三段式（`B8 02 00 00 00` / `BF 01 00 00 00` / `0F 05`）不依赖任何链接器，
用于**端到端验收**：镜像 → 加载计划 → 入口栈 → 调用帧 → 控制台输出。

---

## 9. 不变量清单（违反即为加载器缺陷）

1. 魔数不是 `\x7fELF`，且任何魔数不符的镜像在**读第二个字节之前**被拒。
2. `image_len` 之外的一切文件偏移都被拒（不允许读取越界内容）。
3. W^X 在**页表项写入之前**判定。
4. `PT_GNU_STACK` 带 `PF_X` → 整镜像拒绝。
5. `PT_INTERP` 一定 `ENOTSUP`，不尝试"忽略它继续"。
6. `entropy = 0` 时必须逐字节可复现。
7. 失败回滚后，地址空间/页表帧计数必须回到加载前的值。
8. `entry` 必须落在某个可执行 `PT_LOAD` 内。
9. 入口栈 16 字节对齐，且字符串池严格自顶向下。
10. fuzz 输入下不得 panic、不得产出"半加载"的计划。
