//! UNX-F8361 · 钉版样本协议（版本锁定与校验和）账 · 诊断段 0xB4A0
//! 验收锚点：UNX-F8361-J1 钉版协议成文（样本版本/校验和/来源三注记逐样本），
//! 版本漂移注入 10/10 显性暴露，协议与 C5 联签双认。


use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB4A0;

/// 样本三注记。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PinSample {
    pub name: &'static str,
    pub version: u32,   // 版本锁定
    pub checksum: u32,  // FNV 校验和
    pub source: &'static str,
}

impl PinSample {
    pub fn new(name: &'static str, version: u32, content: u64, source: &'static str) -> Self {
        let mut h: u32 = 0x811C_9DC5;
        for b in content.to_le_bytes().iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        PinSample { name, version, checksum: h, source }
    }
    /// 校验：同内容复算一致。
    pub fn verify(&self, content: u64) -> bool {
        let mut h: u32 = 0x811C_9DC5;
        for b in content.to_le_bytes().iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h == self.checksum
    }
}

/// 钉版协议账（C5 联签双认）。
pub struct PinProtocol {
    pub samples: Vec<PinSample>,
    pub c5_cosign: bool,
}

impl PinProtocol {
    pub fn new() -> Self {
        PinProtocol { samples: Vec::new(), c5_cosign: false }
    }
    pub fn pin(&mut self, s: PinSample) -> bool {
        if self.samples.iter().any(|x| x.name == s.name) {
            return false; // 重名钉版拒
        }
        self.samples.push(s);
        true
    }
    /// 版本漂移检出：同名样本版本偏移。
    pub fn drift_check(&self, name: &'static str, version: u32) -> bool {
        match self.samples.iter().find(|x| x.name == name) {
            Some(s) => s.version == version,
            None => false,
        }
    }
    /// 双认：C5 联签 + 三注记逐样本全满。
    pub fn dual_recognized(&self) -> bool {
        self.c5_cosign
            && self.samples.iter().all(|s| !s.source.is_empty() && s.version > 0 && s.checksum != 0)
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

// ── 注入核（0xB4A0 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8361_B4A0;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("校验-同内容复算一致", { let s = PinSample::new("a", 1, 0xDEAD, "src"); s.verify(0xDEAD) }));
    out.push(("校验-异内容不一致", { let s = PinSample::new("a", 1, 0xDEAD, "src"); !s.verify(0xBEEF) }));
    out.push(("双认-空账不认", !PinProtocol::new().dual_recognized()));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8361_checks：本单判据聚合（诊断段 0xB4A0）。
pub fn run_unx8361_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8361 钉版样本协议");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 六样本钉版：三注记逐样本
    let mut p = PinProtocol::new();
    let samples = [
        ("busybox", 1u32, 0xA001u64, "upstream-1.36"),
        ("coreutils", 2, 0xA002, "upstream-9.4"),
        ("musl-gcc", 1, 0xA003, "toolchain-r3"),
        ("ld-so", 1, 0xA004, "in-tree"),
        ("testsuite", 3, 0xA005, "ci-bundle"),
        ("initramfs", 1, 0xA006, "in-tree"),
    ];
    let mut all_pin = true;
    for (n, v, c, src) in samples.iter() {
        if !p.pin(PinSample::new(n, *v, *c, src)) {
            all_pin = false;
        }
    }
    let pin_ok = all_pin && p.samples.len() == 6;
    s.add("钉版-六样本全入账", pin_ok, if pin_ok { "" } else { "钉版缺口" });

    // 重名钉版拒
    let dup = !p.pin(PinSample::new("busybox", 2, 0xA999, "elsewhere"));
    s.add("钉版-重名拒收", dup, if dup { "" } else { "重名漏网" });

    // 校验和验证：六样本内容复算全过
    let mut v_ok = true;
    for (i, (_, _, c, _)) in samples.iter().enumerate() {
        if !p.samples[i].verify(*c) {
            v_ok = false;
        }
    }
    s.add("校验-六样本复算全一致", v_ok, if v_ok { "" } else { "校验缺口" });

    // 内容篡改检出
    s.add("校验-内容篡改检出", !p.samples[0].verify(0xA001 ^ 1), if !p.samples[0].verify(0xA001 ^ 1) { "" } else { "篡改漏检" });

    // 版本漂移注入 ×10 显性暴露
    let mut drift_hits = 0u32;
    for i in 0..10u32 {
        let name = samples[(i % 6) as usize].0;
        let wrong_v = samples[(i % 6) as usize].1 + 1;
        if !p.drift_check(name, wrong_v) {
            drift_hits += 1;
        }
    }
    s.add("漂移-版本偏移10/10显性暴露", drift_hits == 10, if drift_hits == 10 { "" } else { "漂移漏检" });

    // 正版版本核对 10/10 全过
    let mut good = 0u32;
    for i in 0..10u32 {
        let name = samples[(i % 6) as usize].0;
        let v = samples[(i % 6) as usize].1;
        if p.drift_check(name, v) {
            good += 1;
        }
    }
    s.add("正版-版本核对10/10全过", good == 10, if good == 10 { "" } else { "正版误拒" });

    // 未知样本漂移检查也拒
    s.add("漂移-未知样本拒", !p.drift_check("ghost", 1), if !p.drift_check("ghost", 1) { "" } else { "未知误过" });

    // C5 联签双认
    p.c5_cosign = true;
    s.add("双认-C5联签+三注记全满", p.dual_recognized(), if p.dual_recognized() { "" } else { "双认缺口" });

    // FNV 样本指纹稳定
    let mut acc: u32 = 0x811C_9DC5;
    for smp in &p.samples {
        acc ^= smp.checksum;
        acc = acc.wrapping_mul(0x0100_0193);
    }
    s.add("FNV-样本指纹链非零", acc != 0, if acc != 0 { "" } else { "指纹异常" });

    s
}
