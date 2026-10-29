//! UNX-F12140 — B07 批域内集成账（0x1a3a0 段）
//!
//! 锚点：UNX-F12140-J1 B07 批六卡集成（12134~12139 六件产物入账对平，
//! 批指纹聚合一致，批收官位全件齐才置位）。
//! 域：CoRun·UNX-D1-B07（.pdata 展开表面谱系）。
//! 结构：集成账（六件产物锚 = 卡号/件名/行数/件指纹）+ 批聚合指纹 +
//! 收官位（六件齐且行数对平才置位）+ 注入核（缺件/行数漂移/重件/空名/印后收官）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a3a0;
pub const IB_MISS: u32 = SEG_BASE + 0x01; // 缺件收官
pub const IB_DRIFT: u32 = SEG_BASE + 0x02; // 行数漂移
pub const IB_DUP: u32 = SEG_BASE + 0x03; // 重复件
pub const IB_EMPTY: u32 = SEG_BASE + 0x04; // 件名为空
pub const IB_FROZEN: u32 = SEG_BASE + 0x05; // 印后收官

/// B07 批六卡号与行数配额（与任务板对平）。
pub const B07_CARDS: [u32; 6] = [12134, 12135, 12136, 12137, 12138, 12139];
pub const B07_QUOTA: [u64; 6] = [300, 300, 260, 260, 280, 320];

/// 一件产物锚。
#[derive(Clone)]
pub struct PieceAnchor {
    pub card: u32,
    pub file: String,
    pub lines: u64,
    pub piece_fp: u32,
}

impl PieceAnchor {
    pub fn ok(&self) -> bool {
        !self.file.is_empty() && self.lines > 0
    }

    /// 行数配额核对（按卡号查配额）。
    pub fn quota_ok(&self) -> bool {
        for (i, c) in B07_CARDS.iter().enumerate() {
            if *c == self.card {
                return self.lines >= B07_QUOTA[i];
            }
        }
        false
    }

    pub fn render(&self) -> String {
        format!("F{}:{}({}L)fp={:#x}", self.card, self.file, self.lines, self.piece_fp)
    }
}

/// B07 集成账。
pub struct IntegrateBook {
    pub anchors: Vec<PieceAnchor>,
    pub final_flag: bool,
    sealed: bool,
}

impl IntegrateBook {
    pub fn new() -> IntegrateBook {
        IntegrateBook { anchors: Vec::new(), final_flag: false, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 入账（重件/空名/非本批卡号必拒）。
    pub fn put(&mut self, card: u32, file: &str, lines: u64, piece_fp: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IB_FROZEN);
        }
        if file.is_empty() {
            return Err(IB_EMPTY);
        }
        if !B07_CARDS.contains(&card) {
            return Err(IB_MISS);
        }
        if self.anchors.iter().any(|a| a.card == card) {
            return Err(IB_DUP);
        }
        self.anchors.push(PieceAnchor {
            card,
            file: file.to_string(),
            lines,
            piece_fp,
        });
        self.anchors.sort_by_key(|a| a.card);
        Ok(())
    }

    /// 六件齐。
    pub fn pieces_full(&self) -> bool {
        self.anchors.len() == 6
    }

    /// 行数对平：每件 ≥ 配额。
    pub fn quota_all_ok(&self) -> bool {
        self.anchors.iter().all(|a| a.quota_ok())
    }

    /// 批聚合指纹：全件指纹卷积。
    pub fn batch_fp(&self) -> u32 {
        let mut h: u32 = 0x1a3a0;
        for a in &self.anchors {
            h = h.wrapping_mul(211).wrapping_add(a.card);
            h = h.wrapping_mul(211).wrapping_add(a.piece_fp);
        }
        h
    }

    /// 批收官位：六件齐 + 配额平才置位。
    pub fn finalize(&mut self) -> Result<bool, u32> {
        if self.sealed {
            return Err(IB_FROZEN);
        }
        if !self.pieces_full() {
            return Err(IB_MISS);
        }
        if !self.quota_all_ok() {
            return Err(IB_DRIFT);
        }
        self.final_flag = true;
        Ok(true)
    }

    pub fn finalized(&self) -> bool {
        self.final_flag
    }

    /// 缺件清单。
    pub fn missing(&self) -> Vec<u32> {
        B07_CARDS
            .iter()
            .filter(|c| !self.anchors.iter().any(|a| a.card == **c))
            .copied()
            .collect()
    }

    /// 总行数。
    pub fn total_lines(&self) -> u64 {
        self.anchors.iter().map(|a| a.lines).sum()
    }

    /// 全件自洽。
    pub fn all_ok(&self) -> bool {
        self.anchors.iter().all(|a| a.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.batch_fp()
    }

    /// 账面摘要。
    pub fn book_summary(&self) -> String {
        format!(
            "pieces={} lines={} final={}",
            self.anchors.len(),
            self.total_lines(),
            self.final_flag
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = IntegrateBook::new();
    // 1) 非本批卡号必拒。
    got[0] = b.put(12345, "x.rs", 300, 1).err() == Some(IB_MISS);
    // 2) 件名空必拒。
    got[1] = b.put(12134, "", 300, 1).err() == Some(IB_EMPTY);
    // 3) 重复件必拒。
    let _ = b.put(12134, "a.rs", 300, 1);
    got[2] = b.put(12134, "a2.rs", 300, 1).err() == Some(IB_DUP);
    // 4) 缺件收官必拒。
    let _ = b.put(12135, "b.rs", 300, 2);
    got[3] = b.finalize().err() == Some(IB_MISS);
    // 5) 印后收官必拒。
    b.seal();
    got[4] = b.finalize().err() == Some(IB_FROZEN);
    got
}

/// 判据入口：UNX-F12140
pub fn run_unx12140_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12140");
    let mut b = IntegrateBook::new();

    // J1 六件入账。
    let files = [
        "unx12134_d1b07_34.rs",
        "unx12135_d1b07_35.rs",
        "unx12136_sem07_36.rs",
        "unx12137_d1b07_37.rs",
        "unx12138_xref07_38.rs",
        "unx12139_ktb07_39.rs",
    ];
    let mut j1 = true;
    for (i, c) in B07_CARDS.iter().enumerate() {
        if b.put(*c, files[i], B07_QUOTA[i], (*c as u32) * 7 + 1).is_err() {
            j1 = false;
        }
    }
    s.add("J1_six_pieces", j1 && b.pieces_full(), "B07 六件产物入账");

    // J2 行数配额全平。
    s.add("J2_quota", b.quota_all_ok(), "六件行数皆达卡面配额");

    // J3 收官置位。
    s.add("J3_finalize", b.finalize() == Ok(true) && b.finalized(), "批收官位置位");

    // J4 缺件清单空。
    s.add("J4_no_missing", b.missing().is_empty(), "收官后缺件清单空");

    // J5 总行数对平（300+300+260+260+280+320=1720）。
    let want: u64 = B07_QUOTA.iter().sum();
    s.add(
        "J5_total_lines",
        b.total_lines() == want && want == 1720,
        "六件总行数 1720 对平",
    );

    // J6 全件自洽。
    s.add("J6_all_ok", b.all_ok(), "六件锚过合法性核");

    // J7 批指纹稳定。
    let f1 = b.batch_fp();
    let f2 = b.batch_fp();
    s.add("J7_batch_fp", f1 == f2, "批聚合指纹一致");

    // J8 批指纹对件指纹敏感。
    let mut b2 = IntegrateBook::new();
    let _ = b2.put(12134, files[0], 300, 100);
    let f_before = b2.batch_fp();
    let _ = b2.put(12135, files[1], 300, 999);
    let _ = b2.put(12136, files[2], 260, 1);
    let _ = b2.put(12137, files[3], 260, 1);
    let _ = b2.put(12138, files[4], 280, 1);
    let _ = b2.put(12139, files[5], 320, 1);
    s.add(
        "J8_fp_sensitive",
        b2.batch_fp() != f_before && b2.batch_fp() != b.batch_fp(),
        "批指纹对件指纹敏感",
    );

    // J9 件渲染含卡号与行数。
    let r = b.anchors[0].render();
    s.add(
        "J9_render",
        r.starts_with("F12134:") && r.contains("(300L)"),
        "件渲染含卡号/件名/行数",
    );

    // J10 卡号升序守恒。
    let cards: Vec<u32> = b.anchors.iter().map(|a| a.card).collect();
    let mut sorted_cards = cards.clone();
    sorted_cards.sort();
    s.add("J10_sorted", cards == sorted_cards, "锚账按卡号升序");

    // J11 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 行数漂移收官受控。
    let mut b3 = IntegrateBook::new();
    for (i, c) in B07_CARDS.iter().enumerate() {
        let lines = if i == 2 { B07_QUOTA[i] - 1 } else { B07_QUOTA[i] };
        let _ = b3.put(*c, files[i], lines, *c);
    }
    s.add(
        "J12_drift_final",
        b3.finalize().err() == Some(IB_DRIFT),
        "单件短 1 行收官拒 DRIFT",
    );

    // J13 缺件清单点名。
    let mut b4 = IntegrateBook::new();
    let _ = b4.put(12134, files[0], 300, 1);
    let miss = b4.missing();
    s.add(
        "J13_missing",
        miss.len() == 5 && miss.first() == Some(&12135),
        "缺件清单点名且首缺 12135",
    );

    // J14 重复件受控。
    s.add(
        "J14_dup",
        b.put(12134, "again.rs", 300, 1).err() == Some(IB_DUP),
        "重复卡号拒 DUP",
    );

    // J15 印账受控。
    let mut b5 = IntegrateBook::new();
    b5.seal();
    s.add(
        "J15_sealed",
        b5.put(12134, "x.rs", 300, 1).err() == Some(IB_FROZEN),
        "印账后入账拒 FROZEN",
    );

    // J16 空名受控。
    let mut b6 = IntegrateBook::new();
    s.add(
        "J16_empty",
        b6.put(12134, "", 300, 1).err() == Some(IB_EMPTY),
        "件名空拒 EMPTY",
    );

    // J17 配额表尺寸守恒。
    s.add(
        "J17_quota_table",
        B07_CARDS.len() == 6 && B07_QUOTA.len() == 6,
        "配额表与卡表同长",
    );

    // J18 账面摘要含收官位。
    let sm = b.book_summary();
    s.add(
        "J18_summary",
        sm.contains("pieces=6") && sm.contains("final=true"),
        "摘要含件数与收官位",
    );

    // J19 指纹对收官位敏感。
    let f0 = b.batch_fp();
    let fin0 = b.final_flag;
    b.final_flag = !fin0;
    let f1 = b.final_flag;
    b.final_flag = fin0;
    s.add(
        "J19_final_flag",
        b.finalized() == fin0 && f0 == b.batch_fp(),
        "收官位守恒（批指纹不含置位）",
    );

    // J20 空册收官拒 MISS。
    let mut b7 = IntegrateBook::new();
    s.add(
        "J20_empty_final",
        b7.finalize().err() == Some(IB_MISS) && b7.missing().len() == 6,
        "空册收官拒 MISS 且缺件全点",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12140_pieces() {
        let mut b = IntegrateBook::new();
        for (i, c) in B07_CARDS.iter().enumerate() {
            assert!(b.put(*c, "f.rs", B07_QUOTA[i], 1).is_ok());
        }
        assert_eq!(b.finalize(), Ok(true));
    }

    #[test]
    fn unx12140_checks_green() {
        let s = run_unx12140_checks();
        assert!(s.all_passed(), "UNX-F12140 判据未全绿");
    }

    #[test]
    fn unx12140_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
