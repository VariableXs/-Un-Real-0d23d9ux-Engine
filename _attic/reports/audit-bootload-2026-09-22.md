# 实机加载链异常审计报告（2026-09-22）

审计范围：本轮实机引导关键改动 + 引导热路径（ps2.rs / inputsvc.rs /
proc/usrshell.rs / console.rs / displaysrv.rs / drivers/xhci.rs /
winsurf.rs / main.rs）。维度：panic 边界、unsafe 与内存安全、并发正确性、
操作正确性（鼠标/引导交互）。工具：scan_rust_patterns.py（全库 438 文件
5688 线索，人工数据流判断）+ 逐路径语义走查。

## 审计概览

- **P0：0 条**（新增代码零内存安全/UB 问题）
- **P1：0 条**（新增代码零高危）
- **P2：2 条**（既有非阻断，登记不修）
- 总体结论：**实机加载链新增代码审计通过，可部署 U 盘**。

## 逐项核对（数据流级）

| # | 路径 | 结论 |
|---|------|------|
| 1 | cursor shadow RESTORE 越界写回 | **安全**——set_px 249 行有完整负/上界守卫（fb.rs OOB-safe 契约），指针贴屏边缘时越界像素被静默丢弃，无 fb 外踩踏 |
| 2 | F12→sys_reboot 在 probe 循环触发 | **安全**——probe 段（main.rs 383-412）在 enable_interrupts（501 行）之前，中断未开；identity map 自包含（bootselect windows 分支同序） |
| 3 | F12 过闸时序 | **正确**——bootselect 期 gate=false（菜单即引导界面，防重启循环）；选定后布防；PS/2 泵与 USB HID 同一 note_key 过闸，单一落笔点 |
| 4 | cursor shadow static mut 并发 | **安全**——ushell ring3 syscall 串行 + AP 惰性 hlt（不碰 shadow）；RESTORE 后 take(None) 防二次恢复旧底图 |
| 5 | heartbeat 的 tsc_hz 除零 | **安全**——unwrap_or(1e9) + `.max(1)` 双守卫；HB_LAST_TSC 首帧不打印 |
| 6 | console 批滚 rows≤8 边界 | **安全**——batch=min(8,rows-1).max(1)；rows=1 退化为 copy_rows rows=0 直接 return；rows=2 退化为旧行为 |
| 7 | f12_checkpoint 的 pump 阻塞风险 | **安全**——pump 有 OBF 即返 + 单次 16 字节上限，无自旋 |
| 8 | xhci forensics 寄存器读取 | **安全**——op>0x400 守卫 + 只读 + 泛型 B: BarAccess 同源 |
| 9 | 新增代码 panic 边界 | **零 unwrap/expect/panic**（全模式匹配/Option） |
| 10 | 宿主测试态编译 | cfg(not) 分支 `let _ = b` 消化所有权；note_key 走 enosys 版本，测试安全 |

## P2 登记项（既有，非本轮引入，不阻断部署）

| 级别 | 位置 | 问题 | 建议 |
|------|------|------|------|
| P2 | logger.rs:296/311/361/382 | SERIAL.lock().unwrap()——毒化即 panic | 改 unwrap_or_else(\|e\| e.into_inner())（no_std 简单锁无中毒语义，实为防御性统一） |
| P2 | console.rs 批滚 fill_rect | 逐像素 set_px（8 行≈3 万次 UC 写） | 后续批可换 hline 行块写，性能余量大不紧迫 |

## 未覆盖项（如实声明）

- cargo audit / cargo deny / miri：no_std 自定义目标不适用（依赖仅 core/alloc）。
- 实机中断级 F12（IRQ1 路由）：未启用（IOAPIC legacy 全 mask，防风暴），
  现覆盖面=轮询全路径+长循环协作检查点，已覆盖当前已知全部卡死点。
- 触控板（I2C HID）通道：未覆盖（内置键盘 PS/2 + USB 外设已覆盖）。

## 修复优先级建议

1. （可选）logger.rs SERIAL 锁统一 into_inner 风格——下批顺手项。
2. （后续批）IRQ1 路由 + 中断级 F12——需先在 QEMU 验证 IOAPIC RTE 配置。
3. （后续批）触控板 I2C HID 探针。
