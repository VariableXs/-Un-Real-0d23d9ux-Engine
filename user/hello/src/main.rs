//! 任务14 · 用户态 hello（内核第一次跑上"别人的代码"）。
//!
//! 独立 crate，与内核零链接：只有裸 syscall 约定（rax=号，rdi/rsi/rdx
//! =参，返回 rax）。演示双入口一致性：
//! - 第一次 write 走 `int 0x80`（interrupt gate，iretq 返回）；
//! - 第二次 write 走 `syscall` 指令（LSTAR 入口，sysretq 返回）；
//! - exit 走 `syscall` 指令（内核侧不返回——回收进程槽后停机）。
//! 两路 write 必须输出逐字节相同的串，这就是实机上双入口一致性的证据。

#![no_std]
#![no_main]

use core::arch::asm;

/// 与内核 `proc::syscall` 号表逐字一致（STABLE_SYSCALL_BASE=0）。
const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 2;

const MSG: &[u8] = b"hello from ring3\n";

/// write(1, buf, len) —— int 0x80 通道。
fn write_int80(buf: &[u8]) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "int 0x80",
            inlateout("rax") SYS_WRITE as i64 => ret,
            in("rdi") 1u64,
            in("rsi") buf.as_ptr() as u64,
            in("rdx") buf.len() as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// write(1, buf, len) —— syscall 指令通道。
fn write_syscall(buf: &[u8]) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") SYS_WRITE as i64 => ret,
            in("rdi") 1u64,
            in("rsi") buf.as_ptr() as u64,
            in("rdx") buf.len() as u64,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// exit(0) —— syscall 指令通道；内核侧不返回。
fn exit_syscall(code: u64) -> ! {
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") SYS_EXIT as i64 => _,
            in("rdi") code,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    unreachable!("kernel must not return from exit")
}

/// 读 CS：sysretq/iretq 弹回后的用户代码段自检。
fn read_cs() -> u16 {
    let cs: u16;
    unsafe {
        asm!("mov {0:x}, cs", out(reg) cs, options(nomem, nostack));
    }
    cs
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let n1 = write_int80(MSG);
    let n2 = write_syscall(MSG);
    debug_assert_eq!(n1, MSG.len() as i64);
    debug_assert_eq!(n2, MSG.len() as i64);
    // CS 自检：sysretq 弹回后立即验证用户代码段。实测 QEMU 11.1 的
    // sysretq 曾把 CS 弹成 0x2b（TSS 选择子|RPL3）——若复现，走 int80
    // 报告后再退，内核串口即可看到实锤。
    if read_cs() != 0x1b {
        write_int80(b"[CS!=1b]\n");
    }
    exit_syscall(0);
}

#[panic_handler]
fn ph(_info: &core::panic::PanicInfo) -> ! {
    // 用户态没有输出通道可用（write 可能正是坏的那条）——停住自己。
    loop {
        core::hint::spin_loop();
    }
}
