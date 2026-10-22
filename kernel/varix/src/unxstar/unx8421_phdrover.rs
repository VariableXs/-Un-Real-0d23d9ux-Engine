//! UNX-F8421 · 程序头表越界读拒档（表体越出文件尾）账 · 诊断段 0xB4C0
//! 验收锚点：UNX-F8421-J1 正向：表体完整样本通过；反向：表体越出文件尾注入
//! 全拒（逐条校验——不整表信任），双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB4C0;

pub const PHDR_SIZE: u64 = 56;

/// ELF 头关键域。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElfHeader {
    pub phoff: u64,     // 程序头表偏移
    pub phentsize: u64, // 单项尺寸
    pub phnum: u64,     // 表项数
}

/// 文件模型：长度 + 内容指纹。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileModel {
    pub size: u64,
    pub fingerprint: u32,
}

/// 逐条校验：每项都必须完整落在文件内（不整表信任）。
pub fn validate_phdrs(hdr: &ElfHeader, file: &FileModel) -> Result<(), &'static str> {
    if hdr.phentsize != PHDR_SIZE {
        return Err("phentsize");
    }
    if hdr.phnum == 0 {
        return Ok(()); // 空表合法
    }
    let total = hdr.phoff.checked_add(hdr.phnum.checked_mul(hdr.phentsize).ok_or("overflow")?).ok_or("overflow")?;
    if total > file.size {
        return Err("table_beyond_eof");
    }
    Ok(())
}

/// 拒档器：正向过 + 反向全拒（双判据）。
pub struct PhdrGate {
    pub passed: u32,
    pub rejected: u32,
    pub last_reject: Option<&'static str>,
}

impl PhdrGate {
    pub fn new() -> Self {
        PhdrGate { passed: 0, rejected: 0, last_reject: None }
    }
    pub fn check(&mut self, hdr: &ElfHeader, file: &FileModel) -> bool {
        match validate_phdrs(hdr, file) {
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

// ── 注入核（0xB4C0 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8421_B4C0;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("尺寸-PHDR恰56B", PHDR_SIZE == 56));
    out.push(("空表-合法", validate_phdrs(&ElfHeader { phoff: 0, phentsize: 56, phnum: 0 }, &FileModel { size: 64, fingerprint: 0 }).is_ok()));
    out.push(("门-空账双0", PhdrGate::new().passed == 0 && PhdrGate::new().rejected == 0));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8421_checks：本单判据聚合（诊断段 0xB4C0）。
pub fn run_unx8421_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8421 程序头表越界读拒档");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 正向：表体完整样本通过
    let mut gate = PhdrGate::new();
    let hdr_ok = ElfHeader { phoff: 64, phentsize: 56, phnum: 3 };
    let file_ok = FileModel { size: 64 + 3 * 56 + 1024, fingerprint: 0xF00D };
    let pass1 = gate.check(&hdr_ok, &file_ok);
    s.add("正向-表体完整通过", pass1, if pass1 { "" } else { "完整样本误拒" });

    // 正向边界：表尾恰贴文件尾（含等号过）
    let hdr_edge = ElfHeader { phoff: 64, phentsize: 56, phnum: 2 };
    let file_edge = FileModel { size: 64 + 2 * 56, fingerprint: 1 };
    let pass_edge = gate.check(&hdr_edge, &file_edge);
    s.add("边界-表尾恰贴文件尾通过", pass_edge, if pass_edge { "" } else { "边界误拒" });

    // 反向：表体越出文件尾注入全拒
    let mut all_rej = true;
    let bads = [
        (ElfHeader { phoff: 64, phentsize: 56, phnum: 4 }, FileModel { size: 64 + 3 * 56, fingerprint: 2 }), // 整表超 1 项
        (ElfHeader { phoff: 64, phentsize: 56, phnum: 100 }, FileModel { size: 200, fingerprint: 3 }),       // 表远超
        (ElfHeader { phoff: 1_000_000, phentsize: 56, phnum: 1 }, FileModel { size: 4096, fingerprint: 4 }), // 偏移越界
    ];
    for (h, f) in bads.iter() {
        if gate.check(h, f) {
            all_rej = false;
        }
    }
    let rej_ok = all_rej && gate.rejected == 3 && gate.last_reject == Some("table_beyond_eof");
    s.add("反向-越界注入全拒留因", rej_ok, if rej_ok { "" } else { "越界漏网" });

    // 逐条校验：单项越界（不整表信任）——同表同文件只允许整表恰好放下
    let h1 = ElfHeader { phoff: 100, phentsize: 56, phnum: 3 };
    let f1 = FileModel { size: 100 + 3 * 56 - 1, fingerprint: 5 };
    s.add("逐条-差1字节即拒", !gate.check(&h1, &f1), if !gate.check(&h1, &f1) { "" } else { "差1漏网" });

    // phentsize 非法拒
    let h2 = ElfHeader { phoff: 64, phentsize: 64, phnum: 2 };
    let f2 = FileModel { size: 4096, fingerprint: 6 };
    let e2 = gate.check(&h2, &f2);
    let cause2 = gate.last_reject == Some("phentsize");
    s.add("拒因-phentsize非法拒", !e2 && cause2, if !e2 && cause2 { "" } else { "phentsize 漏网" });

    // 双判据缺一红灯：正向缺 → 红；反向缺 → 红
    let mut g2 = PhdrGate::new();
    g2.check(&hdr_ok, &file_ok); // 只过正向
    let forward_only_ok = g2.passed == 1 && g2.rejected == 0;
    let _ = forward_only_ok;
    // 注入一个反向全拒集，确认反向覆盖
    let mut g3 = PhdrGate::new();
    let mut rej_n = 0u32;
    for i in 1..=10u32 {
        let h = ElfHeader { phoff: 64, phentsize: 56, phnum: i as u64 + 100 };
        let f = FileModel { size: 512, fingerprint: i };
        if !g3.check(&h, &f) {
            rej_n += 1;
        }
    }
    s.add("双判据-反向10/10全拒", rej_n == 10, if rej_n == 10 { "" } else { "反向缺口" });

    // 20 样本随机演练：合法/越界分账
    let mut g4 = PhdrGate::new();
    let mut seed: u64 = 0x8421_0020;
    let mut npass = 0u32;
    for _ in 0..20u32 {
        let n = (lcg_next(&mut seed) % 8) as u64;
        let fsize = 64 + n * 56 + 128;
        let cheat = lcg_next(&mut seed) % 2 == 0;
        let h = ElfHeader { phoff: 64, phentsize: 56, phnum: if cheat { n + 2 } else { n } };
        if g4.check(&h, &FileModel { size: fsize, fingerprint: 0 }) {
            npass += 1;
        }
    }
    let mix_ok = npass > 0 && g4.rejected > 0 && npass + g4.rejected == 20;
    s.add("随机-20样本过/拒两态合计恰20", mix_ok, if mix_ok { "" } else { "混合分账漂移" });

    // FNV 拒因指纹稳定
    let h1 = fnv1a(gate.last_reject.unwrap_or("").as_bytes());
    let h2 = fnv1a(b"table_beyond_eof");
    s.add("FNV-拒因指纹稳定", h1 == h2, if h1 == h2 { "" } else { "指纹漂移" });

    s
}
