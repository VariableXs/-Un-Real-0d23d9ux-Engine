//! UNX-F8521 · PT_INTERP 路径越界拒档账 · 诊断段 0xB560
//! 验收锚点：UNX-F8521-J1 正向：段界内合法解释器路径通过（B04 判据承接）；
//! 反向：路径越段界/缺 NUL 终止注入全拒，双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB560;

/// PT_INTERP 段：文件内 [offset, offset+filesz)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InterpSeg {
    pub offset: u64,
    pub filesz: u64,
}

/// 文件模型。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileModel {
    pub size: u64,
}

/// 解释器路径判档：段界内 + NUL 终止 + 路径合法（'/' 开头非空）。
pub fn validate_interp(seg: &InterpSeg, file: &FileModel, bytes: &[u8]) -> Result<(), &'static str> {
    // 段本身须在文件内
    if seg.offset + seg.filesz > file.size {
        return Err("seg_beyond_file");
    }
    if bytes.len() as u64 > seg.filesz {
        return Err("path_over_seg");
    }
    // 缺 NUL 终止
    if !bytes.contains(&0) {
        return Err("no_nul");
    }
    // 路径体：'/' 开头，NUL 前非空
    let nul_pos = bytes.iter().position(|b| *b == 0).unwrap();
    if nul_pos == 0 {
        return Err("empty_path");
    }
    if bytes[0] != b'/' {
        return Err("not_absolute");
    }
    Ok(())
}

/// 拒档器。
pub struct InterpGate {
    pub passed: u32,
    pub rejected: u32,
    pub last_reject: Option<&'static str>,
}

impl InterpGate {
    pub fn new() -> Self {
        InterpGate { passed: 0, rejected: 0, last_reject: None }
    }
    pub fn check(&mut self, seg: &InterpSeg, file: &FileModel, bytes: &[u8]) -> bool {
        match validate_interp(seg, file, bytes) {
            Ok(()) => {
                self.passed += 1;
                true
            }
            Err(e) => {
                self.rejected += 1;
                self.last_reject = Some(e);
                false
            }
        }
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0xB560 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8521_B560;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("合法-ld-linux路径过", validate_interp(&InterpSeg { offset: 0x318, filesz: 0x1C }, &FileModel { size: 4096 }, b"/lib64/ld-linux-x86-64.so.2\0").is_ok()));
    out.push(("缺NUL-拒", validate_interp(&InterpSeg { offset: 0, filesz: 32 }, &FileModel { size: 4096 }, b"/no/nul").is_err()));
    out.push(("门-空账双0", InterpGate::new().passed == 0 && InterpGate::new().rejected == 0));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8521_checks：本单判据聚合（诊断段 0xB560）。
pub fn run_unx8521_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8521 PT_INTERP 路径越界拒档");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    let mut g = InterpGate::new();
    let file = FileModel { size: 8192 };

    // 正向：段界内合法解释器路径通过（B04 承接）
    let seg_ok = InterpSeg { offset: 0x318, filesz: 0x1C };
    let path_ok = b"/lib64/ld-linux-x86-64.so.2\0".to_vec();
    let p1 = g.check(&seg_ok, &file, &path_ok);
    s.add("正向-合法解释器路径通过", p1, if p1 { "" } else { "合法路径误拒" });

    // 正向边界：路径恰填满段（含 NUL）
    let seg_edge = InterpSeg { offset: 0x400, filesz: 5 };
    let path_edge = b"/ld\0".to_vec();
    let p2 = g.check(&seg_edge, &file, &path_edge);
    s.add("边界-路径恰填满段通过", p2, if p2 { "" } else { "边界误拒" });

    // 反向：路径越段界
    let seg_small = InterpSeg { offset: 0x500, filesz: 8 };
    let path_long = b"/lib64/ld-linux.so.2\0".to_vec();
    let r1 = g.check(&seg_small, &file, &path_long);
    let cause1 = g.last_reject == Some("path_over_seg");
    s.add("反向-路径越段界拒", !r1 && cause1, if !r1 { "" } else { "越界漏网" });

    // 反向：缺 NUL 终止
    let seg_n = InterpSeg { offset: 0x600, filesz: 32 };
    let path_nonul = b"/lib64/no-nul-path".to_vec();
    let r2 = g.check(&seg_n, &file, &path_nonul);
    let cause2 = g.last_reject == Some("no_nul");
    s.add("反向-缺NUL终止拒", !r2 && cause2, if !r2 { "" } else { "缺 NUL 漏网" });

    // 反向：段本身越出文件尾
    let seg_over = InterpSeg { offset: 8000, filesz: 400 };
    let r3 = g.check(&seg_over, &file, b"/x\0");
    s.add("反向-段越文件尾拒", !r3, if !r3 { "" } else { "段越界漏网" });

    // 反向：空路径 / 相对路径
    let r4 = !g.check(&seg_n, &file, b"\0");
    let r5 = !g.check(&seg_n, &file, b"lib/rel.so\0");
    s.add("反向-空路径/相对路径全拒", r4 && r5, if r4 && r5 { "" } else { "路径形态漏网" });

    // 双判据缺一红灯：反向 10/10 注入
    let mut g2 = InterpGate::new();
    let mut n2 = 0u32;
    for i in 0..10u32 {
        let seg = InterpSeg { offset: 0x700, filesz: 4 };
        let path = format!("/lib64/very-long-name-{}.so.2\0", i).into_bytes();
        if !g2.check(&seg, &file, &path) {
            n2 += 1;
        }
    }
    s.add("双判据-反向10/10全拒", n2 == 10, if n2 == 10 { "" } else { "反向缺口" });

    // 正向 10/10
    let mut g3 = InterpGate::new();
    let mut n3 = 0u32;
    for i in 0..10u32 {
        let seg = InterpSeg { offset: 0x100 + i as u64 * 0x40, filesz: 32 };
        let path = format!("/lib/ld-{}.so\0", i).into_bytes();
        if g3.check(&seg, &file, &path) {
            n3 += 1;
        }
    }
    s.add("双判据-正向10/10全过", n3 == 10, if n3 == 10 { "" } else { "正向缺口" });

    // FNV 路径指纹稳定
    let h1 = fnv1a(&path_ok);
    let h2 = fnv1a(b"/lib64/ld-linux-x86-64.so.2\0");
    s.add("FNV-路径指纹稳定", h1 == h2, if h1 == h2 { "" } else { "指纹漂移" });

    s
}
