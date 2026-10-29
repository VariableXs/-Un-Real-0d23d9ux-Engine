//! UNX-F11278 musl 版本钉定与跟随账（0x1f4e0 段）
//!
//! 钉定账记三件事：**版本**（major.minor.patch 压成 0xMMmmpp）、**哈希**
//! （SHA-256 拆成四段 u64，逐段比对，任何一段差一位都算漂移）、**许可**
//! （SPDX 码，钉死 MIT）。三件事之外再记两条跟随义务：上游 tag 必须是
//! v1.2.5（长度 6，缺 tag 记 0），以及 F11219 的联动条目号必须非零。
//! 跟随 SLA 取 30 天为基准、90 天为硬上限；0 天（没排期）与超上限都判失约。
//! 判据函数按固定序：版本 > 哈希 > 许可 > SLA > tag > 联动，只报第一条。
//! 每次登记会实际走完四段哈希（记 bytes_checked += 4），故账本多一张
//! 「比对字节段数」表：replay 要求 bytes_checked == 登记条数 × 4，
//! 少走一段就 EBALANCE——这防止「只看版本就放行」的假绿。
//! 拒绝面只有 id 越界、顺序倒退、同 id 重复、封册后写账；
//! 错版本、错哈希、错许可都是**样本**，绝不拒收。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;

/// 诊断段（0x1f4e0 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f4e0;

/// 条目 id 越界。
pub const MP_EID: u32 = SEG_BASE + 0x01;
/// 登记序倒退。
pub const MP_EORDER: u32 = SEG_BASE + 0x02;
/// 同 id 重复登记。
pub const MP_EDUP: u32 = SEG_BASE + 0x03;
/// 条目表已满。
pub const MP_EFULL: u32 = SEG_BASE + 0x04;
/// 封册后写账。
pub const MP_EFROZEN: u32 = SEG_BASE + 0x05;
/// 观测了未登记的 id。
pub const MP_EUNSEEN: u32 = SEG_BASE + 0x06;
/// 复算判据与登记时不符。
pub const MP_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 账未观测满。
pub const MP_EUNOBS: u32 = SEG_BASE + 0x08;
/// 同一条目重复观测。
pub const MP_EDOBS: u32 = SEG_BASE + 0x09;
/// 哈希段计数/直方不平。
pub const MP_EBALANCE: u32 = SEG_BASE + 0x0a;

/// 判据码：版本漂移。
pub const PV_VER: u32 = 1;
/// 判据码：哈希不符。
pub const PV_HASH: u32 = 2;
/// 判据码：许可不符。
pub const PV_LIC: u32 = 3;
/// 判据码：SLA 失约。
pub const PV_SLA: u32 = 4;
/// 判据码：上游 tag 缺失。
pub const PV_TAG: u32 = 5;
/// 判据码：F11219 联动缺项。
pub const PV_LINK: u32 = 6;

/// 钉定版本 1.2.5 的压缩值。
pub const PIN_VER: u32 = 0x010205;
/// 钉定版本的可读串。
pub const PIN_VER_TEXT: &str = "1.2.5";
/// 钉定上游 tag。
pub const PIN_TAG: &str = "v1.2.5";
/// tag 长度（缺 tag 记 0）。
pub const TAG_LEN: u32 = 6;
/// SHA-256 的期望四段（钉定值，逐段比对）。
pub const EXPECTED_HASH: [u64; 4] = [
    0x8f2a_1c4d_66b0_9315,
    0x2be7_50a9_4d18_c6f3,
    0x71d4_0e88_3fa2_5b67,
    0xc093_6b21_8e5f_47ad,
];
/// 许可码：MIT。
pub const LIC_MIT: u32 = 1;
/// 许可码：GPL-2.0。
pub const LIC_GPL2: u32 = 2;
/// 许可码：BSD-2-Clause。
pub const LIC_BSD2: u32 = 3;
/// 许可码：未声明。
pub const LIC_NONE: u32 = 0;
/// 跟随 SLA 基准（天）。
pub const SLA_BASE: u32 = 30;
/// 跟随 SLA 硬上限（天）。
pub const SLA_MAX: u32 = 90;
/// 条目表定容。
pub const CAP: usize = 12;
/// id 合法域。
pub const ID_MIN: u32 = 1;
pub const ID_MAX: u32 = 4096;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_78;

/// 一条钉定审计样本：版本 + 四段哈希 + 许可 + SLA + tag 长 + 联动号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pin {
    pub id: u32,
    pub ver: u32,
    pub hash: [u64; 4],
    pub lic: u32,
    pub sla_days: u32,
    pub tag_len: u32,
    pub link: u32,
}

impl Pin {
    pub const fn blank() -> Pin {
        Pin { id: 0, ver: 0, hash: [0u64; 4], lic: 0, sla_days: 0, tag_len: 0, link: 0 }
    }

    /// 版本三段解包。
    pub fn ver_parts(&self) -> (u32, u32, u32) {
        ((self.ver >> 16) & 0xff, (self.ver >> 8) & 0xff, self.ver & 0xff)
    }

    pub fn render(&self) -> String {
        let (a, b, c) = self.ver_parts();
        format!("P{} {}.{}.{} lic{} sla{} tag{} link{}",
            self.id, a, b, c, self.lic, self.sla_days, self.tag_len, self.link)
    }
}

/// 纯判据：版本 → 哈希四段 → 许可 → SLA → tag → 联动。
pub fn pin_verdict(p: &Pin) -> u32 {
    if p.ver != PIN_VER {
        return PV_VER;
    }
    let mut k = 0usize;
    while k < 4 {
        let got = match p.hash.get(k) {
            Some(v) => *v,
            None => 0,
        };
        let want = match EXPECTED_HASH.get(k) {
            Some(v) => *v,
            None => 0,
        };
        if got != want {
            return PV_HASH;
        }
        k = k.wrapping_add(1);
    }
    if p.lic != LIC_MIT {
        return PV_LIC;
    }
    if p.sla_days == 0 || p.sla_days > SLA_MAX {
        return PV_SLA;
    }
    if p.tag_len != TAG_LEN {
        return PV_TAG;
    }
    if p.link == 0 {
        return PV_LINK;
    }
    0
}

/// 钉定账本：样本 + 判据 + 七槽直方 + 哈希段计数。
#[derive(Clone, Copy, Debug)]
pub struct PinLedger {
    cases: [Pin; CAP],
    verdicts: [u32; CAP],
    seen: [bool; CAP],
    hist: [u32; 7],
    bytes_checked: u32,
    len: usize,
    attempts: u32,
    accepts: u32,
    rejects: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
    root: Option<u64>,
}

impl PinLedger {
    pub const fn new() -> PinLedger {
        PinLedger {
            cases: [Pin::blank(); CAP],
            verdicts: [0u32; CAP],
            seen: [false; CAP],
            hist: [0u32; 7],
            bytes_checked: 0,
            len: 0,
            attempts: 0,
            accepts: 0,
            rejects: 0,
            observed: 0,
            matched: 0,
            frozen: false,
            root: None,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记：封册 → id 域 → 重复 → 顺序 → 定容。
    pub fn register(&mut self, p: Pin) -> Result<usize, u32> {
        self.attempts = self.attempts.wrapping_add(1);
        if self.frozen {
            return Err(self.fail(MP_EFROZEN));
        }
        if p.id < ID_MIN || p.id > ID_MAX {
            return Err(self.fail(MP_EID));
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == p.id {
                    return Err(self.fail(MP_EDUP));
                }
            }
            k = k.wrapping_add(1);
        }
        if self.len > 0 {
            if let Some(last) = self.cases.get(self.len - 1) {
                if p.id <= last.id {
                    return Err(self.fail(MP_EORDER));
                }
            }
        }
        if self.len >= CAP {
            return Err(self.fail(MP_EFULL));
        }
        if let Some(slot) = self.cases.get_mut(self.len) {
            *slot = p;
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.len - 1)
    }

    /// 登记 + 判据：只有真正进入哈希比对（版本已对上）才计四段。
    pub fn register_ruled(&mut self, p: Pin) -> Result<(usize, u32), u32> {
        let idx = self.register(p)?;
        let v = pin_verdict(&p);
        if let Some(slot) = self.verdicts.get_mut(idx) {
            *slot = v;
        }
        if let Some(h) = self.hist.get_mut(v as usize) {
            *h = h.wrapping_add(1);
        }
        if v != PV_VER {
            self.bytes_checked = self.bytes_checked.wrapping_add(4);
        }
        Ok((idx, v))
    }

    /// 观测实测判据码。
    pub fn observe(&mut self, id: u32, seen_code: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(MP_EFROZEN);
        }
        let mut k = 0usize;
        let mut hit: Option<usize> = None;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == id {
                    hit = Some(k);
                }
            }
            k = k.wrapping_add(1);
        }
        let idx = match hit {
            Some(v) => v,
            None => return Err(MP_EUNSEEN),
        };
        match self.seen.get(idx) {
            Some(true) => return Err(MP_EDOBS),
            _ => {}
        }
        if let Some(slot) = self.seen.get_mut(idx) {
            *slot = true;
        }
        self.observed = self.observed.wrapping_add(1);
        let expect = match self.verdicts.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        if seen_code == expect {
            self.matched = self.matched.wrapping_add(1);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
    pub fn accepts(&self) -> u32 {
        self.accepts
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn observed(&self) -> u32 {
        self.observed
    }
    pub fn matched(&self) -> u32 {
        self.matched
    }
    pub fn bytes_checked(&self) -> u32 {
        self.bytes_checked
    }
    pub fn hist_at(&self, i: usize) -> u32 {
        match self.hist.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
    pub fn hist_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut k = 0usize;
        while k < 7 {
            if let Some(v) = self.hist.get(k) {
                s = s.wrapping_add(*v);
            }
            k = k.wrapping_add(1);
        }
        s
    }
    pub fn zero_verdicts(&self) -> u32 {
        self.hist_at(0)
    }
    pub fn case_at(&self, i: usize) -> Option<Pin> {
        self.cases.get(i).copied()
    }
    pub fn verdict_at(&self, i: usize) -> Option<u32> {
        self.verdicts.get(i).copied()
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    pub fn root(&self) -> Option<u64> {
        self.root
    }

    /// FNV-1a 覆盖版本、四段哈希、许可、SLA、tag、联动与全部计数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.attempts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.observed as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.matched as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.bytes_checked as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.frozen as u64).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.len {
            if let Some(c) = self.cases.get(k) {
                h = (h ^ c.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.ver as u64).wrapping_mul(FNV_PRIME);
                let mut w = 0usize;
                while w < 4 {
                    if let Some(v) = c.hash.get(w) {
                        h = (h ^ *v).wrapping_mul(FNV_PRIME);
                    }
                    w = w.wrapping_add(1);
                }
                h = (h ^ c.lic as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.sla_days as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.tag_len as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.link as u64).wrapping_mul(FNV_PRIME);
            }
            if let Some(v) = self.verdicts.get(k) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            if let Some(b) = self.seen.get(k) {
                h = (h ^ *b as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < 7 {
            if let Some(v) = self.hist.get(j) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        h
    }

    /// 复账：全登记、全观测、全复算、直方与哈希段数双平衡。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len == 0 {
            return Err(MP_EUNOBS);
        }
        if self.observed as usize != self.len || self.matched as usize != self.len {
            return Err(MP_EUNOBS);
        }
        if self.accepts.wrapping_add(self.rejects) != self.attempts {
            return Err(MP_EBALANCE);
        }
        if self.hist_sum() != self.len as u32 {
            return Err(MP_EBALANCE);
        }
        let ran = self.len as u32 - self.hist_at(PV_VER as usize);
        if self.bytes_checked != ran.wrapping_mul(4) {
            return Err(MP_EBALANCE);
        }
        let mut k = 0usize;
        while k < self.len {
            let c = match self.cases.get(k) {
                Some(v) => *v,
                None => return Err(MP_EUNOBS),
            };
            let v = match self.verdicts.get(k) {
                Some(x) => *x,
                None => return Err(MP_EMISMATCH),
            };
            if pin_verdict(&c) != v {
                return Err(MP_EMISMATCH);
            }
            match self.seen.get(k) {
                Some(true) => {}
                _ => return Err(MP_EUNOBS),
            }
            k = k.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 封册：复账后上锁，二次封册即 EFROZEN。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(MP_EFROZEN);
        }
        let h = self.replay()?;
        self.frozen = true;
        self.root = Some(h);
        Ok(h)
    }
}

/// 构造一条「哈希正确」的样本，id 由调用方补。
pub fn pin_full(ver: u32, lic: u32, sla: u32, tag_len: u32, link: u32) -> Pin {
    Pin { id: 1, ver, hash: EXPECTED_HASH, lic, sla_days: sla, tag_len, link }
}

/// 构造一条指定哈希段被翻一位的样本。
pub fn pin_bad_hash(word: usize) -> Pin {
    let mut p = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 0x119);
    if let Some(w) = p.hash.get_mut(word) {
        *w ^= 1;
    }
    p
}

/// 十二条规格样本：两条放行 + 十条逐类失配。
pub fn spec_of(id: usize) -> (Pin, u32) {
    match id {
        1 => (pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 0x119), 0),
        2 => (pin_full(0x010204, LIC_MIT, SLA_BASE, TAG_LEN, 0x119), PV_VER),
        3 => (pin_bad_hash(3), PV_HASH),
        4 => (pin_full(PIN_VER, LIC_GPL2, SLA_BASE, TAG_LEN, 0x119), PV_LIC),
        5 => (pin_full(PIN_VER, LIC_MIT, 0, TAG_LEN, 0x119), PV_SLA),
        6 => (pin_full(PIN_VER, LIC_MIT, SLA_MAX + 1, TAG_LEN, 0x119), PV_SLA),
        7 => (pin_full(PIN_VER, LIC_MIT, SLA_BASE, 0, 0x119), PV_TAG),
        8 => (pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 0), PV_LINK),
        9 => (pin_full(0x020000, LIC_GPL2, SLA_BASE, TAG_LEN, 0x119), PV_VER),
        10 => {
            let mut p = pin_bad_hash(0);
            p.sla_days = 0;
            (p, PV_HASH)
        }
        11 => (pin_full(PIN_VER, LIC_MIT, SLA_MAX, TAG_LEN, 0x119), 0),
        12 => (pin_full(PIN_VER, LIC_BSD2, SLA_BASE, TAG_LEN, 0x119), PV_LIC),
        _ => (Pin::blank(), MP_EID),
    }
}

/// 逐条登记 + 观测。
pub fn build_ledger() -> PinLedger {
    let mut l = PinLedger::new();
    let mut i = 1usize;
    while i <= CAP {
        let (mut p, expect) = spec_of(i);
        p.id = i as u32;
        if l.register_ruled(p).is_ok() {
            let _ = l.observe(p.id, expect);
        }
        i = i.wrapping_add(1);
    }
    l
}

/// 四条独立拒绝：顺序、重复、越界、封册。
pub fn inject_four() -> [bool; 4] {
    let mut a = PinLedger::new();
    let mut hi = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1);
    hi.id = 2;
    let _ = a.register(hi);
    let mut lo = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1);
    lo.id = 1;
    let bad_a = a.register(lo).err() == Some(MP_EORDER);

    let mut b = PinLedger::new();
    let p = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1);
    let _ = b.register(p);
    let bad_b = b.register(p).err() == Some(MP_EDUP);

    let mut c = PinLedger::new();
    let mut wide = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1);
    wide.id = ID_MAX + 1;
    let bad_c = c.register(wide).err() == Some(MP_EID);

    let mut d = build_ledger();
    let _ = d.freeze();
    let mut late = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1);
    late.id = 13;
    let bad_d = d.register(late).err() == Some(MP_EFROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// 十三条自检：前九项压钉定三要素 + SLA/tag/联动 + 账本，后四项压拒绝面。
pub fn run_unx11278_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11278");

    let layout = SEG_BASE == 0x1f4e0
        && MP_EID == SEG_BASE + 0x01
        && MP_EORDER == SEG_BASE + 0x02
        && MP_EDUP == SEG_BASE + 0x03
        && MP_EFULL == SEG_BASE + 0x04
        && MP_EFROZEN == SEG_BASE + 0x05
        && MP_EUNSEEN == SEG_BASE + 0x06
        && MP_EMISMATCH == SEG_BASE + 0x07
        && MP_EUNOBS == SEG_BASE + 0x08
        && MP_EDOBS == SEG_BASE + 0x09
        && MP_EBALANCE == SEG_BASE + 0x0a
        && PV_VER == 1 && PV_HASH == 2 && PV_LIC == 3
        && PV_SLA == 4 && PV_TAG == 5 && PV_LINK == 6;
    s.add("S1-段布局与判据码序", layout, "0x1f4e0 段十码 + 六个判据码连续");

    let mut hash_ok = EXPECTED_HASH.len() == 4;
    let mut i = 0usize;
    while i < EXPECTED_HASH.len() {
        let mut j = i.wrapping_add(1);
        while j < EXPECTED_HASH.len() {
            if let (Some(a), Some(b)) = (EXPECTED_HASH.get(i), EXPECTED_HASH.get(j)) {
                if a == b || *a == 0 {
                    hash_ok = false;
                }
            }
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    s.add("S2-钉定版本哈希许可常量", hash_ok
        && PIN_VER == 0x010205
        && PIN_VER_TEXT == "1.2.5"
        && PIN_TAG == "v1.2.5"
        && TAG_LEN == 6
        && LIC_MIT == 1,
        "1.2.5 钉死、SHA-256 四段非零互异、tag 长 6、许可 MIT");

    let ver_gate = pin_verdict(&pin_full(0x010204, LIC_MIT, SLA_BASE, TAG_LEN, 1)) == PV_VER
        && pin_verdict(&pin_full(0x020000, LIC_MIT, SLA_BASE, TAG_LEN, 1)) == PV_VER
        && pin_verdict(&pin_full(0x010204, LIC_GPL2, 0, 0, 0)) == PV_VER
        && pin_verdict(&pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1)) == 0;
    s.add("S3-版本漂移即判 PV_VER", ver_gate,
        "1.2.4/2.0.0 均拒，且版本先于许可/SLA/tag 判定");

    let mut each_hash = true;
    let mut w = 0usize;
    while w < 4 {
        if pin_verdict(&pin_bad_hash(w)) != PV_HASH {
            each_hash = false;
        }
        w = w.wrapping_add(1);
    }
    s.add("S4-SHA-256 四段逐段比对", each_hash
        && pin_verdict(&pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1)) == 0,
        "任一段翻一位即 PV_HASH，四段全同才放行");

    let lic_gate = pin_verdict(&pin_full(PIN_VER, LIC_GPL2, SLA_BASE, TAG_LEN, 1)) == PV_LIC
        && pin_verdict(&pin_full(PIN_VER, LIC_BSD2, SLA_BASE, TAG_LEN, 1)) == PV_LIC
        && pin_verdict(&pin_full(PIN_VER, LIC_NONE, SLA_BASE, TAG_LEN, 1)) == PV_LIC
        && pin_verdict(&pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1)) == 0;
    s.add("S5-SPDX 许可门", lic_gate, "GPL-2.0/BSD-2/未声明均拒，仅 MIT 放行");

    let sla_gate = pin_verdict(&pin_full(PIN_VER, LIC_MIT, 0, TAG_LEN, 1)) == PV_SLA
        && pin_verdict(&pin_full(PIN_VER, LIC_MIT, SLA_MAX + 1, TAG_LEN, 1)) == PV_SLA
        && pin_verdict(&pin_full(PIN_VER, LIC_MIT, 1, TAG_LEN, 1)) == 0
        && pin_verdict(&pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1)) == 0
        && pin_verdict(&pin_full(PIN_VER, LIC_MIT, SLA_MAX, TAG_LEN, 1)) == 0;
    s.add("S6-跟随 SLA 基准与上限", sla_gate,
        "0 天与 91 天均失约，1/30/90 天在约");

    let tag_gate = pin_verdict(&pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN - 1, 1)) == PV_TAG
        && pin_verdict(&pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN + 1, 1)) == PV_TAG
        && pin_verdict(&pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 0)) == PV_LINK
        && pin_verdict(&pin_full(PIN_VER, LIC_MIT, SLA_BASE, 0, 0)) == PV_TAG;
    s.add("S7-上游 tag 与 F11219 联动门", tag_gate,
        "tag 长非 6 判 PV_TAG，联动号 0 判 PV_LINK，tag 先于联动");

    let led = build_ledger();
    let bal = led.len() == 12
        && led.zero_verdicts() == 2
        && led.hist_at(PV_VER as usize) == 2
        && led.hist_at(PV_HASH as usize) == 2
        && led.hist_at(PV_LIC as usize) == 2
        && led.hist_at(PV_SLA as usize) == 2
        && led.hist_at(PV_TAG as usize) == 1
        && led.hist_at(PV_LINK as usize) == 1
        && led.hist_sum() == 12
        && led.bytes_checked() == 40;
    s.add("S8-直方七槽与哈希段计数", bal,
        "12 条 = 放行 2 + 拒 10；版本对上的 10 条各走四段，计 40");

    s.add("S9-全观测且判据复算一致", led.replay().is_ok()
        && led.observed() == 12 && led.matched() == 12,
        "十二条逐条复算并与观测对齐，段计数与直方互证");

    let mut spec_ok = true;
    let mut k = 1usize;
    while k <= CAP {
        let (mut p, want) = spec_of(k);
        p.id = k as u32;
        if pin_verdict(&p) != want {
            spec_ok = false;
        }
        k = k.wrapping_add(1);
    }
    let parts = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1).ver_parts();
    s.add("S10-规格样本与三段解包", spec_ok
        && parts == (1, 2, 5)
        && pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1).render().contains("1.2.5"),
        "十二条规格逐条自洽，0x010205 解包为 1.2.5");

    let mut g = PinLedger::new();
    let mut c2 = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1);
    c2.id = 2;
    let _ = g.register(c2);
    let mut c1 = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1);
    c1.id = 1;
    let bad_a = g.register(c1).err() == Some(MP_EORDER);
    let bad_b = g.register(c2).err() == Some(MP_EDUP);
    let mut wide = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1);
    wide.id = 0;
    let bad_c = g.register(wide).err() == Some(MP_EID);
    let mut full = build_ledger();
    let mut extra = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1);
    extra.id = 13;
    let bad_d = full.register(extra).err() == Some(MP_EFULL);
    s.add("S11-顺序重复越界定容四闸", bad_a && bad_b && bad_c && bad_d
        && g.accepts() == 1 && g.rejects() == 3 && g.attempts() == 4
        && inject_four() == [true, true, true, true],
        "四闸独立触发，尝试数等于接受数加拒绝数");

    let mut ob = PinLedger::new();
    let mut one = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1);
    one.id = 1;
    let _ = ob.register_ruled(one);
    let miss = ob.replay().err() == Some(MP_EUNOBS);
    let unseen = ob.observe(9, 0).err() == Some(MP_EUNSEEN);
    let wrong = ob.observe(1, PV_SLA) == Ok(false);
    let dob = ob.observe(1, 0).err() == Some(MP_EDOBS);
    s.add("S12-观测面三门", miss && unseen && wrong && dob,
        "未观测 EUNOBS、未登记 EUNSEEN、错观测 false、重观测 EDOBS");

    let mut fz = build_ledger();
    let root = match fz.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = fz.freeze();
    let mut late = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1);
    late.id = 13;
    let post = fz.register(late).err();
    s.add_dyn("S13-封册先复账后上锁", root != 0
        && again == Err(MP_EFROZEN)
        && post == Some(MP_EFROZEN)
        && fz.root() == Some(root)
        && fz.replay().is_ok(),
        &format!("根={:x} 二次={:?} 封后写={:?}", root, again, post));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11278_pin_constants() {
        assert_eq!(PIN_VER, 0x010205);
        assert_eq!(PIN_VER_TEXT, "1.2.5");
        assert_eq!(TAG_LEN, 6);
        assert_eq!(EXPECTED_HASH.len(), 4);
        assert_eq!(pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1).ver_parts(), (1, 2, 5));
    }

    #[test]
    fn unx11278_priority() {
        assert_eq!(pin_verdict(&pin_full(0x010204, LIC_GPL2, 0, 0, 0)), PV_VER);
        let mut p = pin_bad_hash(1);
        p.sla_days = 0;
        assert_eq!(pin_verdict(&p), PV_HASH);
        assert_eq!(pin_verdict(&pin_full(PIN_VER, LIC_NONE, SLA_BASE, TAG_LEN, 0)), PV_LIC);
        assert_eq!(pin_verdict(&pin_full(PIN_VER, LIC_MIT, SLA_MAX + 1, 0, 0)), PV_SLA);
        assert_eq!(pin_verdict(&pin_full(PIN_VER, LIC_MIT, SLA_BASE, 0, 0)), PV_TAG);
    }

    #[test]
    fn unx11278_ledger() {
        let l = build_ledger();
        assert_eq!(l.len(), 12);
        assert_eq!(l.zero_verdicts(), 2);
        assert_eq!(l.hist_sum(), 12);
        assert_eq!(l.bytes_checked(), 40);
        assert_eq!(l.accepts(), 12);
        assert_eq!(l.rejects(), 0);
        assert!(l.replay().is_ok());
        assert_eq!(l.verdict_at(0), Some(0));
        assert_eq!(l.verdict_at(11), Some(PV_LIC));
    }

    #[test]
    fn unx11278_refusals() {
        assert_eq!(inject_four(), [true, true, true, true]);
        let mut l = PinLedger::new();
        let p = pin_full(PIN_VER, LIC_MIT, SLA_BASE, TAG_LEN, 1);
        assert!(l.register(p).is_ok());
        assert_eq!(l.register(p).err(), Some(MP_EDUP));
        assert_eq!(l.attempts(), 2);
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn unx11278_freeze_and_checks() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert!(root != 0);
        assert_eq!(l.freeze(), Err(MP_EFROZEN));
        assert!(l.replay().is_ok());
        let s = run_unx11278_checks();
        assert!(s.all_passed());
        assert_eq!(s.red_items_text(), "");
    }
}
