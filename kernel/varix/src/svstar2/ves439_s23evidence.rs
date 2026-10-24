//! ves439_s23evidence — VE-F4199 · T 域归档与证据三件套
//! 归档：T 域收官证据三件套——验收证据包（评分+走查+联测）/冻结证据包（三基线哈希）/纪律证据包（变更+承诺）
//! 封装+哈希+公示位登记；包缺→补封；哈希错→重算+立案；公示缺→补登；
//! 证据包含多语言无障碍终审证据专卷（域本色）；包内证据脱敏终版。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x13A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvCode {
    Ok = 0,
    PackMissing = SEG_BASE + 0x01,  // 0x13A1
    HashWrong = SEG_BASE + 0x02,    // 0x13A2
    PublicityMissing = SEG_BASE + 0x03, // 0x13A3
}

/// 三件套。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvidencePack { Acceptance = 0, Freeze = 1, Discipline = 2 }

impl EvidencePack {
    pub const ALL: [EvidencePack; 3] = [EvidencePack::Acceptance, EvidencePack::Freeze, EvidencePack::Discipline];
    pub fn name(self) -> &'static str {
        match self {
            EvidencePack::Acceptance => "acceptance", EvidencePack::Freeze => "freeze",
            EvidencePack::Discipline => "discipline",
        }
    }
}

/// 证据包（内容件 + 脱敏终版 + a11y 专卷标记——域本色 + 哈希）。
pub struct Pack {
    pub kind: EvidencePack,
    pub contents: Vec<u32>,
    pub sanitized_final: bool,
    pub a11y_volume: bool, // 多语言无障碍终审证据专卷
    pub hash: u32,
    pub filed: bool,       // 哈希错立案位
}

impl Pack {
    pub fn fnv(contents: &[u32]) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for c in contents {
            h ^= *c;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }
    pub fn seal(&mut self) {
        self.hash = Self::fnv(&self.contents);
        self.filed = false;
    }
    pub fn hash_ok(&self) -> bool { self.hash == Self::fnv(&self.contents) }
}

/// 证据三件套册 + 公示位。
pub struct EvidenceBook {
    pub packs: Vec<Pack>,
    pub publicity: Vec<EvidencePack>, // 公示位登记
    pub reseals: u32,
    pub registrations: u32,
}

impl EvidenceBook {
    pub fn new() -> Self { EvidenceBook { packs: Vec::new(), publicity: Vec::new(), reseals: 0, registrations: 0 } }
    pub fn put(&mut self, p: Pack) {
        self.packs.retain(|x| x.kind != p.kind);
        self.packs.push(p);
    }
    pub fn pack(&self, k: EvidencePack) -> Option<&Pack> { self.packs.iter().find(|p| p.kind == k) }
    /// 归档核验：三包齐+全脱敏终版+全 a11y 专卷+哈希一致+公示登记。
    pub fn archive(&self) -> Result<(), EvCode> {
        for k in EvidencePack::ALL {
            let p = match self.pack(k) { Some(p) => p, None => return Err(EvCode::PackMissing) };
            if !p.sanitized_final || !p.a11y_volume { return Err(EvCode::PackMissing); }
            if !p.hash_ok() { return Err(EvCode::HashWrong); }
            if !self.publicity.contains(&k) { return Err(EvCode::PublicityMissing); }
        }
        Ok(())
    }
    /// 哈希错→重算+立案。
    pub fn reseal_wrong(&mut self, k: EvidencePack) -> bool {
        for p in self.packs.iter_mut() {
            if p.kind == k && !p.hash_ok() {
                p.seal();
                p.filed = true;
                self.reseals += 1;
                return true;
            }
        }
        false
    }
    pub fn register_publicity(&mut self, k: EvidencePack) {
        if !self.publicity.contains(&k) {
            self.publicity.push(k);
            self.registrations += 1;
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves439_checks() -> CheckSet {
    let mut set = CheckSet::new("ves439");
    let mut bk = EvidenceBook::new();

    // C439-01 三包缺→补封码。
    chk(&mut set, "C439-01", bk.archive() == Err(EvCode::PackMissing), "包缺补封");

    // C439-02 封三包（验收含评分/走查/联测证据；冻结含三基线哈希证据；纪律含变更+承诺证据）。
    let mut p1 = Pack { kind: EvidencePack::Acceptance, contents: vec![9301, 9302, 9303], sanitized_final: true, a11y_volume: true, hash: 0, filed: false };
    p1.seal();
    let mut p2 = Pack { kind: EvidencePack::Freeze, contents: vec![1200, 5000, 4001], sanitized_final: true, a11y_volume: true, hash: 0, filed: false };
    p2.seal();
    let mut p3 = Pack { kind: EvidencePack::Discipline, contents: vec![5001, 5002], sanitized_final: true, a11y_volume: true, hash: 0, filed: false };
    p3.seal();
    bk.put(p1); bk.put(p2); bk.put(p3);
    chk(&mut set, "C439-02", bk.archive() == Err(EvCode::PublicityMissing), "公示缺补登");

    // C439-03 公示位登记后归档全过。
    for k in EvidencePack::ALL { bk.register_publicity(k); }
    chk(&mut set, "C439-03", bk.archive().is_ok() && bk.registrations == 3, "公示登记归档过");

    // C439-04 篡改检出（改内容→哈希错）。
    bk.packs[0].contents[0] = 9999;
    chk(&mut set, "C439-04", bk.archive() == Err(EvCode::HashWrong), "篡改检出");

    // C439-05 哈希错→重算+立案留痕。
    chk(&mut set, "C439-05", bk.reseal_wrong(EvidencePack::Acceptance)
        && bk.pack(EvidencePack::Acceptance).unwrap().filed
        && bk.reseals == 1, "重算立案");

    // C439-06 重算后归档复通。
    chk(&mut set, "C439-06", bk.archive().is_ok(), "重算复通");

    // C439-07 a11y 专卷缺→补封码（域本色）。
    bk.packs[1].a11y_volume = false;
    chk(&mut set, "C439-07", bk.archive() == Err(EvCode::PackMissing), "专卷缺拒");

    // C439-08 补专卷复通。
    bk.packs[1].a11y_volume = true;
    chk(&mut set, "C439-08", bk.archive().is_ok(), "专卷补齐");

    // C439-09 公示登记幂等。
    bk.register_publicity(EvidencePack::Freeze);
    chk(&mut set, "C439-09", bk.registrations == 3 && bk.publicity.len() == 3, "公示幂等");

    // C439-10 三包名单单源。
    chk(&mut set, "C439-10", EvidencePack::ALL.len() == 3
        && EvidencePack::ALL[0].name() == "acceptance"
        && EvidencePack::ALL[2].name() == "discipline", "三包单源");

    // C439-11 FNV 哈希可复现。
    chk(&mut set, "C439-11", Pack::fnv(&[1, 2, 3]) == Pack::fnv(&[1, 2, 3])
        && Pack::fnv(&[1, 2]) != Pack::fnv(&[1, 2, 3]), "哈希可复现");

    // C439-12 脱敏终版全真断言。
    chk(&mut set, "C439-12", bk.packs.iter().all(|p| p.sanitized_final), "脱敏终版");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves439_smoke() {
        let set = run_ves439_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves439 red: {:?}", red);
    }
}
