//! UNX-F10247 认证陈述书（模板+证据链引用）（0x12E20 段）
//!
//! 锚点：UNX-F10247-J1 陈述书生成达成（模板槽位全填 + 证据链引用全
//! 可达 10/10 次）；J2 槽位缺漏检出（任一槽位空/引用悬空拒绝出书
//! 10/10 次检出）；J3 陈述书指纹（同证据链同书同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·认证面）。
//!
//! 结构：模板槽位（八槽闭集）+ 证据链账（锚点 → 判据账本）+ 编织核
//! （槽位填充 + 引用解析）+ 注入核（空槽/悬空引用/冻结后改必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x12E20 独占 0x20）。
pub const SEG_BASE: u32 = 0x12E20;
pub const CT_EMPTY_SLOT: u32 = SEG_BASE + 0x01; // 空槽
pub const CT_DANGLING: u32 = SEG_BASE + 0x02; // 悬空引用
pub const CT_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const CT_BAD_REF: u32 = SEG_BASE + 0x04; // 坏引用格式

/// 八槽闭集。
pub const S_BATCH: u8 = 0; // 批次号
pub const S_SCOPE: u8 = 1; // 认证范围
pub const S_GATES: u8 = 2; // 五闸摘要
pub const S_CKS: u8 = 3; // 判据面摘要
pub const S_DISC: u8 = 4; // 披露面摘要
pub const S_ENV: u8 = 5; // 环境声明
pub const S_SIGN: u8 = 6; // 签署人
pub const S_DATE: u8 = 7; // 日期
pub const SLOT_N: u8 = 8;

pub fn slot_name(s: u8) -> &'static str {
    match s {
        S_BATCH => "batch",
        S_SCOPE => "scope",
        S_GATES => "gates",
        S_CKS => "cks",
        S_DISC => "disc",
        S_ENV => "env",
        S_SIGN => "sign",
        _ => "date",
    }
}

/// 证据链账：锚点 id → 证据文本。
pub struct EvidChain {
    rows: Vec<(u32, String)>,
    frozen: bool,
}

impl EvidChain {
    pub fn new() -> EvidChain {
        EvidChain { rows: Vec::new(), frozen: false }
    }

    pub fn put(&mut self, id: u32, text: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if id == 0 {
            return Err("zero_id");
        }
        if text.is_empty() {
            return Err("empty_text");
        }
        if self.rows.iter().any(|(i, _)| *i == id) {
            return Err("dup_id");
        }
        self.rows.push((id, text.to_string()));
        Ok(())
    }

    /// 引用解析：锚点 id → 证据文本（悬空返回 None）。
    pub fn resolve(&self, id: u32) -> Option<&str> {
        self.rows.iter().find(|(i, _)| *i == id).map(|(_, t)| t.as_str())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 陈述书草稿：八槽内容 + 每槽引用锚点集。
pub struct Statement {
    slots: [(bool, String); SLOT_N as usize], // (已填, 内容)
    refs: Vec<(u8, u32)>,            // 槽位 → 锚点引用
    frozen: bool,
}

const SLOT_NONE: (bool, String) = (false, String::new());

impl Statement {
    pub fn new() -> Statement {
        Statement {
            slots: [SLOT_NONE; SLOT_N as usize],
            refs: Vec::new(),
            frozen: false,
        }
    }

    /// 填槽：空内容拒收。
    pub fn fill(&mut self, slot: u8, text: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if slot >= SLOT_N {
            return Err("bad_slot");
        }
        if text.is_empty() {
            return Err("empty_slot");
        }
        self.slots[slot as usize] = (true, text.to_string());
        Ok(())
    }

    /// 挂引用：槽位 → 证据锚点。
    pub fn refer(&mut self, slot: u8, evid_id: u32) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if slot >= SLOT_N {
            return Err("bad_slot");
        }
        if evid_id == 0 {
            return Err("bad_ref");
        }
        if self.refs.iter().any(|(s, e)| *s == slot && *e == evid_id) {
            return Err("dup_ref");
        }
        self.refs.push((slot, evid_id));
        Ok(())
    }

    /// 八槽齐备。
    pub fn full(&self) -> bool {
        self.slots.iter().all(|(f, _)| *f)
    }

    pub fn missing(&self) -> Vec<u8> {
        (0..SLOT_N)
            .filter(|s| !self.slots[*s as usize].0)
            .collect()
    }

    /// 编织出书：槽位全填 + 引用全可达才出文本。
    pub fn render(&self, chain: &EvidChain) -> Result<String, String> {
        let miss = self.missing();
        if !miss.is_empty() {
            let names: Vec<&str> = miss.iter().map(|s| slot_name(*s)).collect();
            return Err(format!("slot_missing:{}", names.join(",")));
        }
        // 悬空引用检测
        for (slot, eid) in &self.refs {
            if chain.resolve(*eid).is_none() {
                return Err(format!("dangling:{}@{}", slot_name(*slot), eid));
            }
        }
        let mut out = String::from("STATEMENT|");
        for s in 0..SLOT_N {
            out.push_str(&format!("{}={},", slot_name(s), self.slots[s as usize].1));
        }
        out.push_str("refs=");
        for (slot, eid) in &self.refs {
            out.push_str(&format!("{}#{},", slot_name(*slot), eid));
        }
        Ok(out)
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 陈述书指纹。
pub fn stmt_fp(text: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in text.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 铺八槽 + 证据链基线。
pub fn seed(st: &mut Statement, ch: &mut EvidChain) -> Result<(), &'static str> {
    let fills: [(u8, &str); 8] = [
        (S_BATCH, "UNX-F10100~F10299 批"),
        (S_SCOPE, "ktest syscall 面 C3-B40"),
        (S_GATES, "五闸全过 5/5"),
        (S_CKS, "判据 200 卡全绿"),
        (S_DISC, "豁免 2 项偏离 1 项披露"),
        (S_ENV, "no_std 内核测试床"),
        (S_SIGN, "AI-WBX1027-J2"),
        (S_DATE, "2026-10-28"),
    ];
    for (s, t) in fills {
        st.fill(s, t)?;
    }
    st.refer(S_GATES, 1)?;
    st.refer(S_CKS, 2)?;
    st.refer(S_DISC, 3)?;
    ch.put(1, "五闸证据：探针/零红/编译/写回/快照")?;
    ch.put(2, "判据证据：200 卡 CheckSet 全绿")?;
    ch.put(3, "披露证据：豁免与偏离清单")?;
    Ok(())
}

/// 注入核。
pub fn inject_fault(
    st: &mut Statement,
    ch: &mut EvidChain,
    code: u32,
    slot: u8,
) -> Result<(), &'static str> {
    match code {
        CT_EMPTY_SLOT => st.fill(slot, ""),
        CT_DANGLING => st.refer(slot, 999),
        CT_FROZEN => {
            st.freeze();
            st.fill(slot, "冻结后填")
        }
        CT_BAD_REF => st.refer(slot, 0),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10247_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10247");

    // J1 陈述书生成 + 证据链全可达
    let j1 = {
        let mut st = Statement::new();
        let mut ch = EvidChain::new();
        let _ = seed(&mut st, &mut ch);
        match st.render(&ch) {
            Ok(t) => t.starts_with("STATEMENT|") && t.contains("sign=AI-WBX1027-J2"),
            Err(_) => false,
        }
    };
    s.add("J1-陈述书生成", j1, "八槽全填引用全可达");

    // J2a 空槽检出（八槽轮转缺席 10/10）
    let j2a = {
        let mut ok = 0;
        for miss in 0..SLOT_N {
            let mut st = Statement::new();
            let mut ch = EvidChain::new();
            let _ = seed(&mut st, &mut ch);
            // 清掉 miss 槽（重建为缺该槽）
            let mut st2 = Statement::new();
            let fills: [(u8, &str); 8] = [
                (S_BATCH, "b"), (S_SCOPE, "s"), (S_GATES, "g"), (S_CKS, "c"),
                (S_DISC, "d"), (S_ENV, "e"), (S_SIGN, "x"), (S_DATE, "t"),
            ];
            for (sl, t) in fills {
                if sl != miss {
                    let _ = st2.fill(sl, t);
                }
            }
            let _ = seed_refs(&mut st2);
            let _ = &mut ch;
            match st2.render(&ch) {
                Err(e) => {
                    if e.contains("slot_missing") && e.contains(slot_name(miss)) {
                        ok += 1;
                    }
                }
                Ok(_) => {}
            }
        }
        ok == 8
    };
    s.add("J2a-空槽八轮", j2a, "8/8 轮缺槽精确点名");

    // J2b 悬空引用检出
    let j2b = {
        let mut st = Statement::new();
        let mut ch = EvidChain::new();
        let _ = seed(&mut st, &mut ch);
        let _ = inject_fault(&mut st, &mut ch, CT_DANGLING, S_ENV);
        matches!(st.render(&ch), Err(e) if e.contains("dangling"))
    };
    s.add("J2b-悬空引用", j2b, "锚点 999 精确报");

    // J3 陈述书指纹：同槽同链同指纹
    let j3 = {
        let mk = || {
            let mut st = Statement::new();
            let mut ch = EvidChain::new();
            let _ = seed(&mut st, &mut ch);
            stmt_fp(&st.render(&ch).unwrap())
        };
        let f1 = mk();
        let f2 = mk();
        f1 == f2 && f1 != 0
    };
    s.add("J3-指纹一致", j3, "同证据链同指纹");

    // J4 坏引用格式拒收
    let j4 = {
        let mut st = Statement::new();
        let mut ch = EvidChain::new();
        let _ = seed(&mut st, &mut ch);
        inject_fault(&mut st, &mut ch, CT_BAD_REF, S_DATE) == Err("bad_ref")
    };
    s.add("J4-坏引用拒", j4, "零锚点即拒");

    // J5 空槽填拒收
    let j5 = {
        let mut st = Statement::new();
        let mut ch = EvidChain::new();
        let _ = seed(&mut st, &mut ch);
        inject_fault(&mut st, &mut ch, CT_EMPTY_SLOT, S_DATE) == Err("empty_slot")
    };
    s.add("J5-空内容拒", j5, "空文本即拒");

    // J6 冻结红线（书与链双面）
    let j6 = {
        let mut st = Statement::new();
        let mut ch = EvidChain::new();
        let _ = seed(&mut st, &mut ch);
        st.freeze();
        let _ = ch.freeze();
        st.is_frozen()
            && ch.is_frozen()
            && st.fill(S_DATE, "后") == Err("frozen")
            && ch.put(9, "后") == Err("frozen")
    };
    s.add("J6-双面冻结", j6, "书与链冻结后全拒");

    // J7 重号证据拒收
    let j7 = {
        let mut ch = EvidChain::new();
        let a = ch.put(5, "一");
        let bb = ch.put(5, "二");
        a == Ok(()) && bb == Err("dup_id")
    };
    s.add("J7-重号证据拒", j7, "同锚点二次即拒");

    // J8 槽位闭集外拒收
    let j8 = {
        let mut st = Statement::new();
        st.fill(9, "x") == Err("bad_slot") && st.refer(9, 1) == Err("bad_slot")
    };
    s.add("J8-槽位外拒", j8, "slot=9 即拒");

    // J9 零锚点证据拒收
    let j9 = EvidChain::new().put(0, "零锚点") == Err("zero_id");
    s.add("J9-零锚点拒", j9, "id=0 即拒");

    // J10 十轮出书回放：10 轮指纹一致（确定性）
    let j10 = {
        let mk = || {
            let mut st = Statement::new();
            let mut ch = EvidChain::new();
            let _ = seed(&mut st, &mut ch);
            stmt_fp(&st.render(&ch).unwrap())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮出书", j10, "10/10 轮指纹一致");

    s
}

/// 引用基线（判据 J2a 辅助：只挂三引用不铺证据文本）。
fn seed_refs(st: &mut Statement) {
    let _ = st.refer(S_GATES, 1);
    let _ = st.refer(S_CKS, 2);
}
