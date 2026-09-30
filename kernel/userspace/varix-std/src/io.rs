//! F056 / F061 / F062 · 描述符与字节流包装.

use crate::abi::{FD_STDERR, FD_STDOUT, RIGHT_READ, RIGHT_WRITE, SYS_CLOSE, SYS_DUP, SYS_OPEN, SYS_PIPE, SYS_READ, SYS_WRITE};
use crate::error::{invoke, Error, Result};

/// F056 · `read(fd, buf)` —— 返回实际读到的字节数，0 表示流结束。
pub fn read(fd: u32, buf: &mut [u8]) -> Result<usize> {
    let n = unsafe {
        invoke(
            SYS_READ,
            [fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0],
        )?
    };
    Ok(n as usize)
}

/// F056 · `write(fd, buf)` —— 返回实际写入的字节数（可能短写，调用方需循环）。
pub fn write(fd: u32, buf: &[u8]) -> Result<usize> {
    let n = unsafe {
        invoke(
            SYS_WRITE,
            [fd as u64, buf.as_ptr() as u64, buf.len() as u64, 0, 0, 0],
        )?
    };
    Ok(n as usize)
}

/// 把整段缓冲写完，容忍短写。返回总字节数。
pub fn write_all(fd: u32, mut buf: &[u8]) -> Result<usize> {
    let mut total = 0usize;
    while !buf.is_empty() {
        match write(fd, buf) {
            Ok(0) => return Err(Error::Io),
            Ok(n) => {
                total += n;
                buf = &buf[n..];
            }
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

/// `print!` 的底层：stdout 上是 `write_all`，出错时静默（不能因为打印失败
/// 而让业务逻辑走上错误分支）。
pub fn print(s: &str) {
    let _ = write_all(FD_STDOUT, s.as_bytes());
}

/// 同上，走 stderr。
pub fn eprint(s: &str) {
    let _ = write_all(FD_STDERR, s.as_bytes());
}

pub fn println(s: &str) {
    print(s);
    print("\n");
}

/// F056 · 打开一个内核资产。`path` 必须是有效的 UTF-8。
pub fn open(path: &str, flags: u32) -> Result<u32> {
    let p = path.as_ptr();
    let fd = unsafe {
        invoke(
            SYS_OPEN,
            [p as u64, path.len() as u64, flags as u64, 0, 0, 0],
        )?
    };
    Ok(fd as u32)
}

pub fn close(fd: u32) -> Result<()> {
    unsafe { invoke(SYS_CLOSE, [fd as u64, 0, 0, 0, 0, 0])? };
    Ok(())
}

/// F061 · `pipe()` → `(读端, 写端)`。任一端分配失败时内核保证两端都不留下。
pub fn pipe() -> Result<(u32, u32)> {
    let mut out = [0u64; 2];
    unsafe {
        invoke(SYS_PIPE, [out.as_mut_ptr() as u64, 0, 0, 0, 0, 0])?;
    }
    Ok((out[0] as u32, out[1] as u32))
}

/// F062 · `dup(fd, target)` —— 复制到指定号；`target` 上原有的描述符先关闭。
pub fn dup(fd: u32, target: u32) -> Result<u32> {
    let v = unsafe { invoke(SYS_DUP, [fd as u64, target as u64, 0, 0, 0, 0])? };
    Ok(v as u32)
}

/// F062 · 权限收窄辅助：绝不允许放大。
pub const fn narrow_rights(available: u32, requested: u32) -> u32 {
    available & requested
}

/// 常用权限组合，方便调用方表达意图。
pub const R_ONLY: u32 = RIGHT_READ;
pub const W_ONLY: u32 = RIGHT_WRITE;
pub const RW: u32 = RIGHT_READ | RIGHT_WRITE;
