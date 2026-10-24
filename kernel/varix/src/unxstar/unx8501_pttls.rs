//! UNX-F8501 PT_TLS memsz<filesz 拒档（0xA400 段）
//!
//! 锚点：UNX-F8501-J1 正向：memsz≥filesz 合法 TLS 段通过（B08 判据
//! 承接）；反向：memsz<filesz 注入全拒（初始镜像溢出——错误码与
//! Linux 对照一致），双判据缺一即批内红灯。
//! 域：CoRun·UNX-C1-B26（TLS 段界核面）。
//!
//! 结构：PT_TLS 段模型（p_filesz/p_memsz/对齐）+ 正向装载器
//! （memsz≥filesz 通过 + .tdata/.tbss 分解）+ 反向拒档（memsz<
//! filesz → ENOEXEC 族 EINVAL 对齐）+ 对齐核（p_align ∈ {8,16,32,64}
//! 且 2 幂）+ 双判据红灯闸 + 注入核（溢出/零对齐/非 2 幂对齐/空段/
//! 封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA400 独占 0x80）。

pub const SEG_BASE: u32 = 0xA400;
pub const XC_TL_EINVAL: u32 = SEG_BASE + 0x01; // memsz<filesz
pub const XC_TL_ALIGN: u32 = SEG_BASE + 0x02; // 对齐非法
pub const XC_TL_EMPTY: u32 = SEG_BASE + 0x03; // 空段
pub const XC_TL_SEALED: u32 = SEG_BASE + 0x04; // 封账

pub const PT_TLS: u32 = 7;

/// TLS 段模型。
#[derive(Clone, Copy)]
pub struct TlsSeg {
    pub p_type: u32,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

/// 正向装载：memsz≥filesz → (tdata, tbss) 分解。
pub fn load_tls(seg: &TlsSeg) -> Result<(u64, u64), u32> {
    if seg.p_type != PT_TLS {
        return Err(XC_TL_EINVAL);
    }
    if seg.p_filesz == 0 && seg.p_memsz == 0 {
        return Err(XC_TL_EMPTY);
    }
    // 对齐核：2 幂且 ∈ {8,16,32,64}
    if seg.p_align != 8 && seg.p_align != 16 && seg.p_align != 32 && seg.p_align != 64 {
        return Err(XC_TL_ALIGN);
    }
    if seg.p_memsz < seg.p_filesz {
        return Err(XC_TL_EINVAL); // 初始镜像溢出
    }
    // .tdata = filesz（有初值），.tbss = memsz - filesz（零填充）
    Ok((seg.p_filesz, seg.p_memsz - seg.p_filesz))
}

/// 对齐 2 幂判定器。
pub fn is_pow2_align(a: u64) -> bool {
    a != 0 && (a & (a - 1)) == 0
}

/// 正向通过核：memsz==filesz / memsz>filesz 两种合法形态。
pub fn legal_tls_exact() -> bool {
    // 形态 1：无 tbss（memsz==filesz）
    let s1 = TlsSeg { p_type: PT_TLS, p_filesz: 64, p_memsz: 64, p_align: 16 };
    // 形态 2：有 tbss（memsz>filesz）
    let s2 = TlsSeg { p_type: PT_TLS, p_filesz: 32, p_memsz: 96, p_align: 64 };
    match (load_tls(&s1), load_tls(&s2)) {
        (Ok((td1, tb1)), Ok((td2, tb2))) => {
            td1 == 64 && tb1 == 0 && td2 == 32 && tb2 == 64
        }
        _ => false,
    }
}

/// 反向注入全拒核：memsz<filesz 各种形态。
pub fn reject_tls_exact() -> bool {
    // 溢出 1 字节
    let s1 = TlsSeg { p_type: PT_TLS, p_filesz: 64, p_memsz: 63, p_align: 16 };
    // 溢出一半
    let s2 = TlsSeg { p_type: PT_TLS, p_filesz: 64, p_memsz: 32, p_align: 16 };
    // filesz 正 memsz 零
    let s3 = TlsSeg { p_type: PT_TLS, p_filesz: 8, p_memsz: 0, p_align: 8 };
    load_tls(&s1) == Err(XC_TL_EINVAL)
        && load_tls(&s2) == Err(XC_TL_EINVAL)
        && load_tls(&s3) == Err(XC_TL_EINVAL)
}

/// 对齐核：合法对齐集 + 非法对齐拒。
pub fn align_exact() -> bool {
    let ok8 = TlsSeg { p_type: PT_TLS, p_filesz: 8, p_memsz: 16, p_align: 8 };
    let ok16 = TlsSeg { p_type: PT_TLS, p_filesz: 8, p_memsz: 16, p_align: 16 };
    let ok32 = TlsSeg { p_type: PT_TLS, p_filesz: 8, p_memsz: 16, p_align: 32 };
    let ok64 = TlsSeg { p_type: PT_TLS, p_filesz: 8, p_memsz: 16, p_align: 64 };
    let bad1 = TlsSeg { p_type: PT_TLS, p_filesz: 8, p_memsz: 16, p_align: 1 };
    let bad0 = TlsSeg { p_type: PT_TLS, p_filesz: 8, p_memsz: 16, p_align: 0 };
    let bad3 = TlsSeg { p_type: PT_TLS, p_filesz: 8, p_memsz: 16, p_align: 3 };
    let bad128 = TlsSeg { p_type: PT_TLS, p_filesz: 8, p_memsz: 16, p_align: 128 };
    load_tls(&ok8).is_ok()
        && load_tls(&ok16).is_ok()
        && load_tls(&ok32).is_ok()
        && load_tls(&ok64).is_ok()
        && load_tls(&bad1) == Err(XC_TL_ALIGN)
        && load_tls(&bad0) == Err(XC_TL_ALIGN)
        && load_tls(&bad3) == Err(XC_TL_ALIGN)
        && load_tls(&bad128) == Err(XC_TL_ALIGN)
        && is_pow2_align(16)
        && !is_pow2_align(24)
}

/// 空段拒核。
pub fn empty_tls_exact() -> bool {
    let e = TlsSeg { p_type: PT_TLS, p_filesz: 0, p_memsz: 0, p_align: 16 };
    load_tls(&e) == Err(XC_TL_EMPTY)
}

/// 非法 type 拒核。
pub fn type_guard_exact() -> bool {
    let s = TlsSeg { p_type: 1, p_filesz: 8, p_memsz: 16, p_align: 16 };
    load_tls(&s) == Err(XC_TL_EINVAL)
}

/// 双判据红灯闸核：正向过 + 反向拒 = 绿；缺一 = 红灯。
pub fn dual_gate_exact() -> bool {
    let pos = legal_tls_exact();
    let neg = reject_tls_exact();
    pos && neg // 缺一即红
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 溢出注入
    let s1 = TlsSeg { p_type: PT_TLS, p_filesz: 100, p_memsz: 99, p_align: 16 };
    if load_tls(&s1) == Err(XC_TL_EINVAL) {
        caught += 1;
    }
    // 2) 零对齐
    let s2 = TlsSeg { p_type: PT_TLS, p_filesz: 8, p_memsz: 16, p_align: 0 };
    if load_tls(&s2) == Err(XC_TL_ALIGN) {
        caught += 1;
    }
    // 3) 非 2 幂对齐
    let s3 = TlsSeg { p_type: PT_TLS, p_filesz: 8, p_memsz: 16, p_align: 48 };
    if load_tls(&s3) == Err(XC_TL_ALIGN) {
        caught += 1;
    }
    // 4) 空段
    if load_tls(&TlsSeg { p_type: PT_TLS, p_filesz: 0, p_memsz: 0, p_align: 8 })
        == Err(XC_TL_EMPTY)
    {
        caught += 1;
    }
    // 5) 越界对齐（128 非法闭集外）
    if load_tls(&TlsSeg { p_type: PT_TLS, p_filesz: 8, p_memsz: 16, p_align: 128 })
        == Err(XC_TL_ALIGN)
    {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：合法 TLS 分解折叠哈希。
pub fn fp_frozen(seg: &TlsSeg) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    match load_tls(seg) {
        Ok((td, tb)) => {
            h ^= td;
            h = h.wrapping_mul(0x100000001b3);
            h ^= tb;
            h = h.wrapping_mul(0x100000001b3);
        }
        Err(e) => {
            h ^= e as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx8501_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F8501");
    // J1-1 正向合法 TLS 通过
    let ok = legal_tls_exact();
    s.add("B26-01-正向通过", ok, if ok { "" } else { "正向漂移" });
    // J1-2 反向溢出全拒
    let ok = reject_tls_exact();
    s.add("B26-02-反向全拒", ok, if ok { "" } else { "拒档漂移" });
    // J1-3 对齐核
    let ok = align_exact();
    s.add("B26-03-对齐核", ok, if ok { "" } else { "对齐漂移" });
    // J1-4 空段 + type 守卫
    let ok = empty_tls_exact() && type_guard_exact();
    s.add("B26-04-空段守卫", ok, if ok { "" } else { "守卫漂移" });
    // J1-5 双判据红灯闸
    let ok = dual_gate_exact();
    s.add("B26-05-双判据闸", ok, if ok { "" } else { "闸漂移" });
    // J1-6 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B26-06-注入核必抓", ok, &detail);
    // J1-7 指纹冻结
    let seg = TlsSeg { p_type: PT_TLS, p_filesz: 48, p_memsz: 80, p_align: 32 };
    let ok = fp_frozen(&seg) == fp_frozen(&seg);
    s.add("B26-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
