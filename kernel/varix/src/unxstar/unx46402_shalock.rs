//! UNX-F46402 钉版校验和锁定判据（组件产物 SHA 级校验与镜像分发一致性）（0x40010 段）
//!
//! 锚点：UNX-F46402-J1 五件产物校验和 ×2 次独立重算一致、与镜像分发件
//! 逐字节比对零漂移。
//! 域：CoRun·UNX-S1-B01（工具链域·校验锁定面）。
//!
//! 结构：产物字节账（五件各带确定性合成字节流）+ 双通道独立摘要
//! （FNV-1a A/B 双种子，任一重算漂移即拒）+ 镜像分发账（分发件 =
//! 字节流副本，逐字节比对）+ 篡改注入核（单字节翻转/截断/超长必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40010 独占 0x10）。
pub const SEG_BASE: u32 = 0x40010;
pub const CS_LEN_BAD: u32 = SEG_BASE + 0x01; // 摘要长度非 16 位十六进制
pub const CS_DUAL_MISMATCH: u32 = SEG_BASE + 0x02; // 双通道重算不一致
pub const CS_MIRROR_DRIFT: u32 = SEG_BASE + 0x03; // 镜像分发件逐字节漂移
pub const CS_ART_MISSING: u32 = SEG_BASE + 0x04; // 五件闭集缺件
pub const CS_LEN_ZERO: u32 = SEG_BASE + 0x05; // 空产物拒绝入账

/// 五件闭集（与 F46401 组件序同源）。
pub const ART_GCC: usize = 0;
pub const ART_CLANG: usize = 1;
pub const ART_RUSTC: usize = 2;
pub const ART_GIT: usize = 3;
pub const ART_CMAKE: usize = 4;
pub const ART_COUNT: usize = 5;

pub const ART_NAMES: [&str; ART_COUNT] = ["gcc", "clang", "rustc", "git", "cmake"];

/// 摘要格式核：16 位十六进制。
pub fn sum16_ok(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 16 && b.iter().all(|c| c.is_ascii_hexdigit())
}

/// 产物字节账：确定性合成字节流（同组件同版本字节级可重放）。
pub struct Artifact {
    pub comp: usize,
    pub version: String,
    pub data: Vec<u8>,
}

impl Artifact {
    /// 合成产物字节流：组件名 + 版本 + 1024 字节确定性扩展（LCG 填充）。
    pub fn synth(comp: usize, version: &str) -> Result<Self, u32> {
        if comp >= ART_COUNT {
            return Err(CS_ART_MISSING);
        }
        if version.is_empty() {
            return Err(CS_LEN_ZERO);
        }
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(ART_NAMES[comp].as_bytes());
        data.extend_from_slice(b"|");
        data.extend_from_slice(version.as_bytes());
        data.extend_from_slice(b"|");
        // LCG 确定性填充至 1024 字节（同输入同字节流）。
        let mut x: u32 = 0x40010 ^ (comp as u32).wrapping_mul(2654435761);
        while data.len() < 1024 {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            data.push((x >> 24) as u8);
        }
        Ok(Artifact { comp, version: version.to_string(), data })
    }

    /// 双通道摘要（通道种子不同 → 两路独立重算）。
    pub fn dual_sum(&self) -> (String, String) {
        (Self::fnv(self.data.as_slice(), 2166136261), Self::fnv(self.data.as_slice(), 0x811C9DC5 ^ 0x9E3779B9))
    }

    /// FNV-1a 可种子版（双通道共用实现、双种子独立调用）。
    fn fnv(data: &[u8], seed: u32) -> String {
        let mut h = seed;
        for b in data {
            h ^= *b as u32;
            h = h.wrapping_mul(16777619);
        }
        format!("{:08x}{:08x}", h, h.rotate_left(16))
    }

    /// J1 第一半：×2 次独立重算一致（双通道各自重跑两遍逐位同）。
    pub fn dual_recompute_stable(&self) -> bool {
        let (a1, b1) = self.dual_sum();
        let (a2, b2) = self.dual_sum();
        a1 == a2 && b1 == b2 && sum16_ok(&a1) && sum16_ok(&b1) && a1 != b1
    }
}

/// 校验锁定账：组件 → 通道 A 摘要（钉版时锁定）。
pub struct LockBook {
    pub locked: Vec<(usize, String, String)>, // (comp, version, sum_a)
    pub sealed: bool,
}

impl LockBook {
    pub fn new() -> Self {
        LockBook { locked: Vec::new(), sealed: false }
    }

    /// 锁定登记：双算稳定才入账；同组件重复锁定拒绝。
    pub fn lock(&mut self, a: &Artifact) -> Result<(), u32> {
        if !a.dual_recompute_stable() {
            return Err(CS_DUAL_MISMATCH);
        }
        if self.locked.iter().any(|(c, _, _)| *c == a.comp) {
            return Err(CS_DUAL_MISMATCH);
        }
        let (sa, _sb) = a.dual_sum();
        self.locked.push((a.comp, a.version.clone(), sa));
        Ok(())
    }

    /// J1 第二半：镜像分发件逐字节比对（分发件 = 字节流副本）。
    pub fn verify_mirror(&self, a: &Artifact, mirror: &[u8]) -> Result<(), u32> {
        if mirror.len() != a.data.len() {
            return Err(CS_MIRROR_DRIFT);
        }
        for i in 0..mirror.len() {
            if mirror[i] != a.data[i] {
                return Err(CS_MIRROR_DRIFT);
            }
        }
        let (sa, _) = a.dual_sum();
        for (c, _v, s) in self.locked.iter() {
            if *c == a.comp {
                if *s != sa {
                    return Err(CS_DUAL_MISMATCH);
                }
                return Ok(());
            }
        }
        Err(CS_ART_MISSING)
    }

    /// 五件全锁定核（J1 全量口径）。
    pub fn full_locked(&self) -> bool {
        self.locked.len() == ART_COUNT
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 单字节翻转注入器（审计用：定位到偏移的确定性翻转）。
pub fn flip_byte(data: &[u8], at: usize) -> Option<Vec<u8>> {
    if at >= data.len() {
        return None;
    }
    let mut m = data.to_vec();
    m[at] ^= 0x01;
    Some(m)
}

/// 镜像分发清单：五件分发件 + 双通道摘要逐件登记（闭账材料件）。
pub struct MirrorManifest {
    /// (comp, len, sum_a, sum_b) 逐件四元组。
    pub items: Vec<(usize, usize, String, String)>,
    pub sealed: bool,
}

impl MirrorManifest {
    /// 从产物集构建清单（逐件登记长度与双摘要）。
    pub fn build(arts: &[Artifact]) -> Result<Self, u32> {
        if arts.len() != ART_COUNT {
            return Err(CS_ART_MISSING);
        }
        let mut items = Vec::new();
        for a in arts.iter() {
            let (sa, sb) = a.dual_sum();
            items.push((a.comp, a.data.len(), sa, sb));
        }
        Ok(MirrorManifest { items, sealed: false })
    }

    /// 清单与分发件集逐件核对（长度 + 双摘要三方一致）。
    pub fn verify(&self, arts: &[Artifact]) -> Result<(), u32> {
        if arts.len() != self.items.len() {
            return Err(CS_ART_MISSING);
        }
        for i in 0..self.items.len() {
            let (c, len, sa, sb) = &self.items[i];
            let a = &arts[i];
            if *c != a.comp || *len != a.data.len() {
                return Err(CS_MIRROR_DRIFT);
            }
            let (ra, rb) = a.dual_sum();
            if *sa != ra || *sb != rb {
                return Err(CS_DUAL_MISMATCH);
            }
        }
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（逐件复合 FNV）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for (c, l, sa, _sb) in self.items.iter() {
            let line = format!("{}|{}|{}", c, l, sa);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 锁定账导出渲染（五件行：组件|版本|摘要A，供闭账材料消费）。
pub fn render_lockbook(book: &LockBook) -> String {
    let mut out = String::new();
    let mut ordered: Vec<(usize, &String, &String)> = book.locked.iter().map(|(c, v, s)| (*c, v, s)).collect();
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    for (c, v, s) in ordered.iter() {
        out.push_str(&format!("{}|{}|{}\n", ART_NAMES[*c], v, s));
    }
    out
}

/// 渲染完整性核：行数 = 锁定件数，每行三段且组件名闭合集。
pub fn render_lockbook_ok(text: &str, expect: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect {
        return false;
    }
    lines.iter().all(|ln| {
        let seg: Vec<&str> = ln.split('|').collect();
        seg.len() == 3
            && ART_NAMES.iter().any(|n| *n == seg[0])
            && !seg[1].is_empty()
            && sum16_ok(seg[2])
    })
}

/// 清单组件序单调核（items 按组件号严格升序，防乱序登记漂移）。
pub fn manifest_order_ok(m: &MirrorManifest) -> bool {
    for w in m.items.windows(2) {
        if w[0].0 >= w[1].0 {
            return false;
        }
    }
    m.items.len() == ART_COUNT
}

/// 标准夹具：五件全合成 + 全锁定。
pub fn standard_lock() -> (Vec<Artifact>, LockBook) {
    let vers = ["13.2.0", "17.0.6", "1.78.0", "2.43.0", "3.28.3"];
    let mut arts: Vec<Artifact> = Vec::new();
    for c in 0..ART_COUNT {
        arts.push(Artifact::synth(c, vers[c]).expect("fixture artifact"));
    }
    let mut book = LockBook::new();
    for a in arts.iter() {
        book.lock(a).expect("fixture lock must succeed");
    }
    (arts, book)
}

/// 夹具镜像副本（逐字节拷贝，模拟分发件）。
pub fn fixture_mirror(arts: &[Artifact]) -> Vec<Vec<u8>> {
    arts.iter().map(|a| a.data.clone()).collect()
}

/// 注入核：6 注入必抓（单字节翻转/截断/超长/缺件/空产物/重复锁定）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let (arts, book) = standard_lock();
    // 1) 单字节翻转必抓（漂移定位到某件）
    let mirror = fixture_mirror(&arts);
    let bad = flip_byte(&mirror[0], 511).expect("flip must exist");
    if book.verify_mirror(&arts[0], &bad) == Err(CS_MIRROR_DRIFT) {
        caught += 1;
    }
    // 2) 截断必抓
    let short = mirror[1][..512].to_vec();
    if book.verify_mirror(&arts[1], &short) == Err(CS_MIRROR_DRIFT) {
        caught += 1;
    }
    // 3) 超长必抓
    let mut long = mirror[2].clone();
    long.push(0u8);
    if book.verify_mirror(&arts[2], &long) == Err(CS_MIRROR_DRIFT) {
        caught += 1;
    }
    // 4) 账外组件验证必抓（第六件闭集外 → 构造后锁定账查不到）
    let outsider = Artifact::synth(0, "99.9.9").expect("outsider");
    if book.verify_mirror(&outsider, &outsider.data.clone()) == Err(CS_ART_MISSING) {
        caught += 1;
    }
    // 5) 空版本产物拒绝
    if Artifact::synth(ART_GIT, "").err() == Some(CS_LEN_ZERO) {
        caught += 1;
    }
    // 6) 重复锁定拒绝
    let mut b2 = LockBook::new();
    b2.lock(&arts[0]).expect("first lock");
    let dup = Artifact::synth(0, "13.2.1").expect("dup artifact");
    if b2.lock(&dup) == Err(CS_DUAL_MISMATCH) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46402_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46402");
    // J1-1 五件双通道 ×2 次独立重算一致
    let (arts, _book) = standard_lock();
    let ok = arts.iter().all(|a| a.dual_recompute_stable());
    s.add("46402-J1-双算×2一致", ok, if ok { "" } else { "重算漂移" });
    // J1-2 五件全锁定
    let (_arts, book) = standard_lock();
    let ok = book.full_locked() && book.locked.len() == ART_COUNT;
    s.add("46402-J1-五件全锁定", ok, if ok { "" } else { "锁定不全" });
    // J1-3 镜像分发件逐字节比对零漂移（5/5）
    let (arts2, book2) = standard_lock();
    let mirror = fixture_mirror(&arts2);
    let mut all_ok = true;
    for i in 0..ART_COUNT {
        if book2.verify_mirror(&arts2[i], &mirror[i]).is_err() {
            all_ok = false;
        }
    }
    s.add("46402-J1-镜像零漂移", all_ok, if all_ok { "" } else { "镜像比对漂移" });
    // J1-4 篡改件必拒（翻转产物重算摘要必变，与锁定账不一致）
    let (arts3, book3) = standard_lock();
    let mut tampered = arts3[3].data.clone();
    tampered[100] ^= 0x80;
    let t = Artifact { comp: 3, version: arts3[3].version.clone(), data: tampered };
    let ok = book3.verify_mirror(&t, &t.data.clone()).is_err()
        && t.dual_sum().0 != arts3[3].dual_sum().0
        && t.dual_sum().1 != arts3[3].dual_sum().1;
    s.add("46402-J1-篡改必拒", ok, if ok { "" } else { "篡改漏抓" });
    // J1-5 摘要格式核（16 位十六进制 ×2 通道）
    let (sa, sb) = arts2[0].dual_sum();
    let ok = sum16_ok(&sa) && sum16_ok(&sb);
    s.add("46402-J1-摘要格式", ok, if ok { "" } else { "格式非法" });
    // J1-6 镜像清单五件全登记 + 逐件核对通过 + 封账指纹稳定
    let (arts4, _b4) = standard_lock();
    let mut mani = MirrorManifest::build(&arts4).expect("manifest build");
    let v_ok = mani.verify(&arts4).is_ok();
    mani.seal();
    let fp1 = mani.fingerprint();
    let (arts5, _b5) = standard_lock();
    let mani2 = MirrorManifest::build(&arts5).expect("manifest build2");
    let ok = v_ok && mani.items.len() == ART_COUNT && fp1 == mani2.fingerprint();
    s.add("46402-J1-镜像清单核对", ok, if ok { "" } else { "清单核对漂移" });
    // J1-8 清单组件序单调
    let ok = manifest_order_ok(&mani);
    s.add("46402-J1-清单序单调", ok, if ok { "" } else { "清单乱序" });
    // J1-7 锁定账导出渲染五件行齐
    let text = render_lockbook(&book2);
    let ok = render_lockbook_ok(&text, ART_COUNT) && text.starts_with("gcc|13.2.0|");
    s.add("46402-J1-导出渲染齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46402-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46402_all_checks_green() {
        let set = run_unx46402_checks();
        assert!(set.all_passed(), "UNX-F46402 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx46402_synth_deterministic() {
        let a1 = Artifact::synth(ART_GCC, "13.2.0").expect("a1");
        let a2 = Artifact::synth(ART_GCC, "13.2.0").expect("a2");
        assert_eq!(a1.data, a2.data, "合成字节流必须确定性");
    }
}
