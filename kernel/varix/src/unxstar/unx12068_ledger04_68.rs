//! UNX-F12068 CurrentLocale 与区域语义账（0x19aa0 段）
//!
//! 卡面：TEB→CurrentLocale——线程侧区域环境句柄（LCID）语义账册。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 LCID 拆解：主语言段（低 16 位）与排序段（高 16 位）可独立回读。
//! - J2 零段拒收：语言段为 0 或排序段为 0 的 LCID 绑定必拒。
//! - J3 绑定状态机：Unbound→Bound→Rebound 单向推进，回跳必拒。
//! - J4 换绑留痕：每次换绑记账（旧/新/序号），序号严格递增且唯一。
//! - J5 印账冻结：seal 后任何换绑必拒。
//! - J6 注入五况全抓。
//!
//! 结构：Lcid 双段拆解类型 + Locale 绑定状态机 + LocaleLedger 换绑账 +
//! 折叠指纹（211 进制卷积）+ 注入核（零语言段/零排序段/回跳/空留痕/印后写）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19aa0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19aa0;
pub const LC_ZERO_LANG: u32 = SEG_BASE + 0x01; // 语言段为 0
pub const LC_ZERO_SORT: u32 = SEG_BASE + 0x02; // 排序段为 0
pub const LC_BACK_JUMP: u32 = SEG_BASE + 0x03; // 状态机回跳
pub const LC_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 换绑留痕为空
pub const LC_FROZEN: u32 = SEG_BASE + 0x05; // 印账后换绑

/// LCID：低 16 位主语言段 + 高 16 位排序段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcid(pub u32);

impl Lcid {
    /// 主语言段（低 16 位）。
    pub fn lang(&self) -> u16 {
        (self.0 & 0xFFFF) as u16
    }
    /// 排序段（高 16 位）。
    pub fn sort(&self) -> u16 {
        (self.0 >> 16) as u16
    }
    /// 两段皆非零才合法。
    pub fn well_formed(&self) -> bool {
        self.lang() != 0 && self.sort() != 0
    }
    /// 渲染账面行：`L<hex>:S<hex>`。
    pub fn render(&self) -> String {
        format!("L{:04x}:S{:04x}", self.lang(), self.sort())
    }
}

/// 绑定状态三值：未绑/已绑/已换绑。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindState {
    Unbound = 0,
    Bound = 1,
    Rebound = 2,
}

/// 一次换绑的账本留痕。
#[derive(Clone)]
pub struct RebindRec {
    pub seq: u32,
    pub old: Lcid,
    pub new: Lcid,
    pub note: String,
}

impl RebindRec {
    /// 账目合法性：序号非零、留痕非空、new 两段非零；
    /// old 允许零哨兵（首绑无旧绑——J6 契约：首绑记录 old 为零 LCID 但
    /// new 合法仍过 ok()），非零则须两段非零。
    pub fn ok(&self) -> bool {
        self.seq != 0
            && !self.note.is_empty()
            && self.new.well_formed()
            && (self.old.0 == 0 || self.old.well_formed())
    }
    pub fn render_line(&self) -> String {
        format!(
            "R{}|{}->{}|{}",
            self.seq, self.old.render(), self.new.render(), self.note
        )
    }
}

/// CurrentLocale 区域语义账：绑定状态机 + 换绑账本。
pub struct LocaleBook {
    pub state: BindState,
    pub current: Option<Lcid>,
    pub ledger: Vec<RebindRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl LocaleBook {
    pub fn new() -> LocaleBook {
        LocaleBook {
            state: BindState::Unbound,
            current: None,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 绑定：仅 Unbound 态接受，两段任一为零必拒。
    pub fn bind(&mut self, lcid: Lcid, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(LC_FROZEN);
        }
        if lcid.lang() == 0 {
            return Err(LC_ZERO_LANG);
        }
        if lcid.sort() == 0 {
            return Err(LC_ZERO_SORT);
        }
        if self.state != BindState::Unbound {
            return Err(LC_BACK_JUMP);
        }
        self.state = BindState::Bound;
        self.current = Some(lcid);
        self.ledger.push(RebindRec {
            seq: self.seq_next,
            old: Lcid(0),
            new: lcid,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 换绑：Bound/Rebound 态接受，留痕为空必拒；单向推进。
    pub fn rebind(&mut self, lcid: Lcid, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(LC_FROZEN);
        }
        if lcid.lang() == 0 {
            return Err(LC_ZERO_LANG);
        }
        if lcid.sort() == 0 {
            return Err(LC_ZERO_SORT);
        }
        if note.is_empty() {
            return Err(LC_EMPTY_NOTE);
        }
        let old = match self.current {
            Some(o) => o,
            None => return Err(LC_BACK_JUMP),
        };
        if old == lcid {
            return Err(LC_BACK_JUMP); // 换绑到同值视作回跳
        }
        if self.state == BindState::Rebound {
            // 已换绑态允许继续换绑，但不得回到首绑值。
            if let Some(first) = self.ledger.first() {
                if first.new == lcid {
                    return Err(LC_BACK_JUMP);
                }
            }
        }
        self.state = BindState::Rebound;
        self.current = Some(lcid);
        self.ledger.push(RebindRec {
            seq: self.seq_next,
            old,
            new: lcid,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 账本自洽：逐条合法且序号严格递增。
    pub fn ledger_ok(&self) -> bool {
        let mut prev: u32 = 0;
        for r in &self.ledger {
            if !r.ok() || r.seq <= prev {
                return false;
            }
            prev = r.seq;
        }
        true
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.state as u32);
        if let Some(c) = self.current {
            h = h.wrapping_mul(211).wrapping_add(c.0);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
            h = h.wrapping_mul(211).wrapping_add(r.old.0);
            h = h.wrapping_mul(211).wrapping_add(r.new.0);
            for by in r.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：换绑次数。
    pub fn rebind_count(&self) -> usize {
        self.ledger.len()
    }

    /// 诊断面：绑定密度（基点）。
    pub fn bind_density_bps(&self) -> u32 {
        if self.state == BindState::Unbound {
            return 0;
        }
        10000
    }

    /// 诊断面：账面摘要。
    pub fn summary(&self) -> String {
        format!(
            "state={} cur={} recs={}",
            self.state as u16,
            match self.current {
                Some(c) => c.render(),
                None => "none".to_string(),
            },
            self.ledger.len()
        )
    }

    /// 诊断面：最近一次换绑序号。
    pub fn last_seq(&self) -> u32 {
        self.ledger.last().map(|r| r.seq).unwrap_or(0)
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = LocaleBook::new();
    let _ = b.bind(Lcid(0x0409_0409), "base");
    // 1) 零语言段拒收。
    got[0] = b.rebind(Lcid(0x0409_0000), "zero-lang").err() == Some(LC_ZERO_LANG);
    // 2) 零排序段拒收。
    got[1] = b.rebind(Lcid(0x0000_0409), "zero-sort").err() == Some(LC_ZERO_SORT);
    // 3) 换绑到当前值视作回跳必拒。
    let cur = b.current.unwrap_or(Lcid(1));
    got[2] = b.rebind(cur, "same").err() == Some(LC_BACK_JUMP);
    // 4) 空留痕换绑必拒。
    got[3] = b.rebind(Lcid(0x0804_0804), "").err() == Some(LC_EMPTY_NOTE);
    // 5) 印账后换绑必抓。
    b.seal();
    got[4] = b.rebind(Lcid(0x0407_0407), "post").err() == Some(LC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12068_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12068");

    // J1 LCID 拆解：双段独立回读。
    let l = Lcid(0x0804_0409);
    let j1 = l.lang() == 0x0409 && l.sort() == 0x0804 && l.well_formed();
    s.add("J1-LCID拆解", j1, "双段独立回读");

    // J2 零段拒收（语言段）。
    let mut b2 = LocaleBook::new();
    let j2 = b2.bind(Lcid(0x0409_0000), "z").err() == Some(LC_ZERO_LANG);
    s.add("J2-零段拒收", j2, "语言段为零必拒");

    // J3 绑定状态机：Unbound→Bound→Rebound，首绑后不得再 bind。
    let j3 = {
        let mut b = LocaleBook::new();
        b.bind(Lcid(0x0409_0409), "init").is_ok()
            && b.state == BindState::Bound
            && b.rebind(Lcid(0x0804_0804), "r").is_ok()
            && b.state == BindState::Rebound
            && b.bind(Lcid(0x0407_0407), "again").err() == Some(LC_BACK_JUMP)
    };
    s.add("J3-状态机单向", j3, "Unbound→Bound→Rebound 单向");

    // J4 换绑留痕：逐条合法且序号严格递增。
    let j4 = {
        let mut b = LocaleBook::new();
        let _ = b.bind(Lcid(0x0409_0409), "i");
        let _ = b.rebind(Lcid(0x0804_0804), "r1");
        let _ = b.rebind(Lcid(0x0407_0407), "r2");
        b.ledger.len() == 3 && b.ledger_ok() && b.last_seq() == 3
    };
    s.add("J4-换绑留痕", j4, "序号递增且逐条合法");

    // J5 印账冻结。
    let j5 = {
        let mut b = LocaleBook::new();
        let _ = b.bind(Lcid(0x0409_0409), "i");
        b.seal();
        b.rebind(Lcid(0x0407_0407), "post").err() == Some(LC_FROZEN)
    };
    s.add("J5-印账冻结", j5, "seal 后换绑必拒");

    // J6 首绑账目：首绑记录 old 为零 LCID 但 new 合法，仍过 ok()。
    let j6 = {
        let mut b = LocaleBook::new();
        let _ = b.bind(Lcid(0x0409_0409), "first");
        b.ledger[0].ok() && b.ledger[0].render_line().contains("R1")
    };
    s.add("J6-首绑账目", j6, "首绑行可渲染且合法");

    // J7 指纹幂等且 seal 后封禁。
    let j7 = {
        let mut b = LocaleBook::new();
        let _ = b.bind(Lcid(0x0409_0409), "i");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.bind(Lcid(0x0407_0407), "post").err() == Some(LC_FROZEN)
    };
    s.add("J7-指纹封账", j7, "指纹幂等且印后必拒");

    // J8 注入核 5/5。
    let got = inject_five();
    let j8 = got.iter().all(|x| *x);
    s.add_dyn("J8-注入全抓", j8, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = LocaleBook::new();
    let _ = b.bind(Lcid(0x0409_0409), "sum-i");
    let _ = b.rebind(Lcid(0x0804_0804), "sum-r");

    s.add("账本自洽", b.ledger_ok(), "逐条过自洽核");
    s.add("绑定密度", b.bind_density_bps() == 10000, "已绑态密度满格");
    s.add("账面摘要", b.summary().contains("recs=2"), "摘要含记录数");
    s.add("渲染行", b.ledger[1].render_line().contains("0804"), "换绑行含新值");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12068_bind_flow() {
        let mut b = LocaleBook::new();
        assert!(b.bind(Lcid(0x0409_0409), "i").is_ok());
        assert_eq!(b.rebind(Lcid(0x0000_0409), "z"), Err(LC_ZERO_SORT));
        assert!(b.rebind(Lcid(0x0804_0804), "r").is_ok());
        assert_eq!(b.ledger.len(), 2);
    }

    #[test]
    fn unx12068_checks_green() {
        let s = run_unx12068_checks();
        assert!(s.all_passed(), "UNX-F12068 判据未全绿");
    }

    #[test]
    fn lcid_parts_and_render() {
        let l = Lcid(0x0407_0c0a);
        assert_eq!(l.lang(), 0x0c0a);
        assert_eq!(l.sort(), 0x0407);
        assert!(l.render().contains("0c0a"));
        assert!(!Lcid(0).well_formed());
    }
}
