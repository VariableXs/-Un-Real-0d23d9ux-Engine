//! UNX-F12138 — .pdata 对照表制度与异常目录交付协议（D2 联签）（0x1a360 段）
//!
//! 锚点：UNX-F12138-J1 对照表三表齐（结构/接口/交付路径）+ D2 联签交付
//! （双签齐备才置交付位，缺签/错签/越序签 10/10 拒）。
//! 域：CoRun·UNX-D1-B07（.pdata 展开表面谱系）。
//! 结构：对照表（结构表/接口表/交付表三账）+ 联签核（D2 侧签名比对）+
//! 交付位（三表齐 + 双签齐才置位）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a360;
pub const XT_MISS: u32 = SEG_BASE + 0x01; // 表未齐
pub const XT_BAD_SIGN: u32 = SEG_BASE + 0x02; // 签名不符
pub const XT_ORDER: u32 = SEG_BASE + 0x03; // 越序签
pub const XT_EMPTY: u32 = SEG_BASE + 0x04; // 项为空
pub const XT_FROZEN: u32 = SEG_BASE + 0x05; // 交付后再签

/// 表种闭集（三表制度）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TableKind {
    Struct,  // 结构表
    Iface,   // 接口表
    Deliver, // 交付表
}

impl TableKind {
    pub fn name(&self) -> &'static str {
        match self {
            TableKind::Struct => "struct",
            TableKind::Iface => "iface",
            TableKind::Deliver => "deliver",
        }
    }
}

/// 一条对照项。
#[derive(Clone)]
pub struct XrefItem {
    pub seq: u32,
    pub kind: TableKind,
    pub key: String,
    pub expect: String,
}

impl XrefItem {
    pub fn ok(&self) -> bool {
        !self.key.is_empty() && !self.expect.is_empty()
    }

    /// 命中判定：键相等且期望一致。
    pub fn matches(&self, got: &str) -> bool {
        got == self.expect
    }
}

/// 对照表制度册（含 D2 联签）。
pub struct XrefBook {
    pub items: Vec<XrefItem>,
    /// D2 侧签名（交付协议对端）。
    pub d2_sign: Option<u32>,
    /// 本侧签名。
    pub sign: Option<u32>,
    /// 交付位。
    pub delivered: bool,
    pub seq_next: u32,
    sealed: bool,
}

impl XrefBook {
    pub fn new() -> XrefBook {
        XrefBook {
            items: Vec::new(),
            d2_sign: None,
            sign: None,
            delivered: false,
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂对照项（键/期望为空必拒）。
    pub fn put(&mut self, kind: TableKind, key: &str, expect: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(XT_FROZEN);
        }
        if key.is_empty() || expect.is_empty() {
            return Err(XT_EMPTY);
        }
        let seq = self.seq_next;
        self.items.push(XrefItem {
            seq,
            kind,
            key: key.to_string(),
            expect: expect.to_string(),
        });
        self.seq_next += 1;
        Ok(seq)
    }

    /// 三表齐判定：每种表至少一项。
    pub fn tables_full(&self) -> bool {
        for k in [TableKind::Struct, TableKind::Iface, TableKind::Deliver] {
            if !self.items.iter().any(|i| i.kind == k) {
                return false;
            }
        }
        true
    }

    /// 本侧签名（全项折叠）。
    pub fn compute_sign(&self) -> u32 {
        let mut h: u32 = 0x1a360;
        for i in &self.items {
            h = h.wrapping_mul(211).wrapping_add(i.seq);
            for by in i.key.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
            for by in i.expect.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    /// 本侧落签。
    pub fn sign_self(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(XT_FROZEN);
        }
        if !self.tables_full() {
            return Err(XT_MISS);
        }
        self.sign = Some(self.compute_sign());
        Ok(self.sign.unwrap())
    }

    /// D2 落签（错签必拒：签名与本侧折叠不一致即拒）。
    pub fn sign_d2(&mut self, sig: u32) -> Result<(), u32> {
        if self.delivered {
            return Err(XT_FROZEN);
        }
        if self.sign.is_none() {
            return Err(XT_ORDER);
        }
        if Some(sig) != self.sign {
            return Err(XT_BAD_SIGN);
        }
        self.d2_sign = Some(sig);
        Ok(())
    }

    /// 交付判定：三表齐 + 双签齐且相等。
    pub fn deliver(&mut self) -> Result<bool, u32> {
        if !self.tables_full() {
            return Err(XT_MISS);
        }
        if self.sign.is_none() || self.d2_sign.is_none() {
            return Err(XT_ORDER);
        }
        if self.sign != self.d2_sign {
            return Err(XT_BAD_SIGN);
        }
        self.delivered = true;
        Ok(true)
    }

    /// 对照抽验：按键查项并比期望。
    pub fn verify(&self, kind: TableKind, key: &str, got: &str) -> Result<bool, u32> {
        let it = self
            .items
            .iter()
            .find(|i| i.kind == kind && i.key == key)
            .ok_or(XT_MISS)?;
        Ok(it.matches(got))
    }

    /// 按表清点。
    pub fn count_of(&self, kind: TableKind) -> usize {
        self.items.iter().filter(|i| i.kind == kind).count()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a360;
        h = h.wrapping_mul(211).wrapping_add(self.sign.unwrap_or(0));
        h = h.wrapping_mul(211).wrapping_add(self.d2_sign.unwrap_or(0));
        h = h.wrapping_mul(211).wrapping_add(self.delivered as u32);
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 账面摘要。
    pub fn book_summary(&self) -> String {
        format!(
            "items={} full={} delivered={}",
            self.items.len(),
            self.tables_full(),
            self.delivered
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = XrefBook::new();
    // 1) 表未齐先签必拒（越序）。
    let _ = b.put(TableKind::Struct, "s1", "v1");
    got[0] = b.sign_self().err() == Some(XT_MISS);
    // 2) 键空必拒。
    got[1] = b.put(TableKind::Iface, "", "v").err() == Some(XT_EMPTY);
    // 3) 表齐后 D2 签错必拒。
    let _ = b.put(TableKind::Iface, "i1", "v2");
    let _ = b.put(TableKind::Deliver, "d1", "v3");
    let _ = b.sign_self();
    got[2] = b.sign_d2(0xDEAD).err() == Some(XT_BAD_SIGN);
    // 4) D2 先于本侧签必拒（新册）。
    let mut b2 = XrefBook::new();
    got[3] = b2.sign_d2(1).err() == Some(XT_ORDER);
    // 5) 交付后再签必拒。
    let sig = b.sign.unwrap();
    let _ = b.sign_d2(sig);
    let _ = b.deliver();
    got[4] = b.sign_d2(sig).err() == Some(XT_FROZEN);
    got
}

/// 判据入口：UNX-F12138
pub fn run_unx12138_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12138");
    let mut b = XrefBook::new();

    // J1 三表挂项 15/15（每表 5 项）。
    let mut j1 = true;
    for kind in [TableKind::Struct, TableKind::Iface, TableKind::Deliver] {
        for i in 0..5usize {
            let key = format!("{}_k{}", kind.name(), i);
            let expect = format!("{}_v{}", kind.name(), i);
            if b.put(kind, &key, &expect).is_err() {
                j1 = false;
            }
        }
    }
    s.add("J1_put_15x", j1 && b.items.len() == 15, "三表 15 项全挂");

    // J2 三表齐。
    s.add("J2_tables_full", b.tables_full(), "结构/接口/交付三表齐");

    // J3 表清点对平。
    s.add(
        "J3_count",
        b.count_of(TableKind::Struct) == 5
            && b.count_of(TableKind::Iface) == 5
            && b.count_of(TableKind::Deliver) == 5,
        "三表各 5 项对平",
    );

    // J4 对照抽验命中。
    s.add(
        "J4_verify_hit",
        b.verify(TableKind::Struct, "struct_k0", "struct_v0") == Ok(true),
        "结构表抽验命中",
    );

    // J5 对照抽验错值必拒（false 而非错）。
    s.add(
        "J5_verify_miss",
        b.verify(TableKind::Struct, "struct_k0", "wrong") == Ok(false),
        "错值抽验返回 false",
    );

    // J6 未挂键查无此项。
    s.add(
        "J6_verify_miss_key",
        b.verify(TableKind::Struct, "nope", "x").err() == Some(XT_MISS),
        "未挂键查无此项拒 MISS",
    );

    // J7 本侧落签。
    let sig = b.sign_self();
    s.add("J7_sign_self", sig.is_ok() && b.sign.is_some(), "表齐本侧落签成");

    // J8 D2 同签放行。
    let sig = sig.unwrap();
    s.add("J8_sign_d2_ok", b.sign_d2(sig) == Ok(()), "D2 同签放行");

    // J9 交付位置位。
    s.add("J9_deliver", b.deliver() == Ok(true) && b.delivered, "双签齐交付位置位");

    // J10 错签受控（新册重演）。
    let mut b2 = XrefBook::new();
    for kind in [TableKind::Struct, TableKind::Iface, TableKind::Deliver] {
        let _ = b2.put(kind, "k", "v");
    }
    let _ = b2.sign_self();
    s.add(
        "J10_bad_sign",
        b2.sign_d2(42).err() == Some(XT_BAD_SIGN),
        "错签拒 BAD_SIGN",
    );

    // J11 表未齐签自受控。
    let mut b3 = XrefBook::new();
    let _ = b3.put(TableKind::Struct, "k", "v");
    s.add(
        "J11_tables_miss",
        b3.sign_self().err() == Some(XT_MISS) && b3.deliver().err() == Some(XT_MISS),
        "表未齐签/交付皆拒 MISS",
    );

    // J12 键/期望空受控。
    let mut b4 = XrefBook::new();
    s.add(
        "J12_empty",
        b4.put(TableKind::Struct, "", "v").err() == Some(XT_EMPTY)
            && b4.put(TableKind::Struct, "k", "").err() == Some(XT_EMPTY),
        "键/期望空拒 EMPTY",
    );

    // J13 指纹对交付位敏感。
    let f0 = b.fold();
    b.delivered = false;
    let f1 = b.fold();
    b.delivered = true;
    s.add(
        "J13_fp_deliver",
        f0 != f1 && b.fold() == f0,
        "指纹对交付位敏感",
    );

    // J14 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J14_inject_all", caught == 5, "5 注入点全检出");

    // J15 印账受控。
    let mut b5 = XrefBook::new();
    b5.seal();
    s.add(
        "J15_sealed",
        b5.put(TableKind::Struct, "k", "v").err() == Some(XT_FROZEN)
            && b5.sign_self().err() == Some(XT_FROZEN),
        "印账后挂项/落签拒 FROZEN",
    );

    // J16 序号单调。
    let seqs: Vec<u32> = b.items.iter().map(|i| i.seq).collect();
    let mono = seqs.windows(2).all(|w| w[1] > w[0]);
    s.add("J16_seq_mono", mono && seqs.first() == Some(&1), "对照项序号单调");

    // J17 全项自洽。
    s.add("J17_items_ok", b.items.iter().all(|i| i.ok()), "15 项过合法性核");

    // J18 表名闭集。
    let names = [TableKind::Struct.name(), TableKind::Iface.name(), TableKind::Deliver.name()];
    s.add(
        "J18_kind_names",
        names == ["struct", "iface", "deliver"],
        "三表名闭集",
    );

    // J19 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J19_summary",
        sm.contains("items=15") && sm.contains("delivered=true"),
        "摘要含项数与交付位",
    );

    // J20 双签相等守恒。
    s.add(
        "J20_sign_eq",
        b.sign == b.d2_sign && b.sign.is_some(),
        "交付后双签相等守恒",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12138_sign_deliver() {
        let mut b = XrefBook::new();
        for kind in [TableKind::Struct, TableKind::Iface, TableKind::Deliver] {
            let _ = b.put(kind, "k", "v");
        }
        let sig = b.sign_self().unwrap();
        assert!(b.sign_d2(sig).is_ok());
        assert_eq!(b.deliver(), Ok(true));
    }

    #[test]
    fn unx12138_checks_green() {
        let s = run_unx12138_checks();
        assert!(s.all_passed(), "UNX-F12138 判据未全绿");
    }

    #[test]
    fn unx12138_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
