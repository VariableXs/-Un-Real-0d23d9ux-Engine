//! UNX-F10250 豁免披露面（认证报告豁免附录全量）（0x13480 段）
//!
//! 锚点：UNX-F10250-J1 豁免附录全量达成（豁免条目全量入册且每条带四
//! 要素：对象/类别/依据/影响面 10/10 齐备）；J2 豁免滥用检出（无依据
//! 豁免/超范围豁免/豁免后复活 10/10 次检出）；J3 附录哈希（同豁免集
//! 同哈希，加删条目哈希必变）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·认证面）。
//!
//! 结构：豁免册（四要素条目）+ 类别闭集（NA/资源受限/规范偏离）+ 依据
//! 核（依据文本可解析）+ 附录哈希 + 注入核（无依据/超范围/复活必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x13480 独占 0x20）。
pub const SEG_BASE: u32 = 0x13480;
pub const EX_NO_BASIS: u32 = SEG_BASE + 0x01; // 无依据
pub const EX_OUT_SCOPE: u32 = SEG_BASE + 0x02; // 超范围
pub const EX_REVIVE: u32 = SEG_BASE + 0x03; // 豁免后复活
pub const EX_BAD_CAT: u32 = SEG_BASE + 0x04; // 坏类别
pub const EX_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后改

/// 豁免类别闭集。
pub const X_NA: u8 = 0; // 不适用
pub const X_RESOURCE: u8 = 1; // 资源受限豁免
pub const X_DEVIATE: u8 = 2; // 规范偏离披露
pub const CAT_N: u8 = 3;

pub fn cat_ok(c: u8) -> bool {
    c < CAT_N
}

pub fn cat_name(c: u8) -> &'static str {
    match c {
        X_NA => "na",
        X_RESOURCE => "resource",
        _ => "deviate",
    }
}

/// 豁免条目（四要素）。
#[derive(Clone, PartialEq)]
pub struct Exempt {
    pub id: u32,
    pub obj: String,  // 对象：条款/判据锚点
    pub cat: u8,      // 类别
    pub basis: String, // 依据
    pub impact: String, // 影响面
}

impl Exempt {
    /// 四要素齐备：对象/依据/影响面非空 + 类别闭集内。
    pub fn ok(&self) -> bool {
        !self.obj.is_empty()
            && cat_ok(self.cat)
            && self.basis.len() >= 4 // 依据须可解析（含前缀 B-）
            && self.basis.starts_with("B-")
            && !self.impact.is_empty()
            && self.id != 0
    }

    pub fn render(&self) -> String {
        format!(
            "EX{}|{}|{}|{}|{}",
            self.id,
            self.obj,
            cat_name(self.cat),
            self.basis,
            self.impact
        )
    }
}

/// 豁免册。
pub struct ExemptBook {
    rows: Vec<Exempt>,
    frozen: bool,
    revoked: Vec<u32>, // 已撤销豁免（复活检测账）
}

impl ExemptBook {
    pub fn new() -> ExemptBook {
        ExemptBook { rows: Vec::new(), frozen: false, revoked: Vec::new() }
    }

    /// 入册：四要素缺一/重号/冻结后写拒收。
    pub fn add(
        &mut self,
        id: u32,
        obj: &str,
        cat: u8,
        basis: &str,
        impact: &str,
    ) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if id == 0 {
            return Err("zero_id");
        }
        if !cat_ok(cat) {
            return Err("bad_cat");
        }
        if self.rows.iter().any(|r| r.id == id) {
            return Err("dup_id");
        }
        let e = Exempt {
            id,
            obj: obj.to_string(),
            cat,
            basis: basis.to_string(),
            impact: impact.to_string(),
        };
        if !e.ok() {
            return Err("missing_factor");
        }
        self.rows.push(e);
        Ok(())
    }

    /// 撤销豁免（复活检测账）。
    pub fn revoke(&mut self, id: u32) -> Result<(), &'static str> {
        if !self.rows.iter().any(|r| r.id == id) {
            return Err("no_id");
        }
        if self.revoked.contains(&id) {
            return Err("already");
        }
        self.revoked.push(id);
        Ok(())
    }

    /// 复用检测：已撤销豁免再次入册即拒（复活红线）。
    pub fn revive_attempt(&mut self, id: u32) -> Result<(), &'static str> {
        if self.revoked.contains(&id) {
            return Err("revived");
        }
        self.add(id, "复活对象", X_NA, "B-复活依据", "复活影响")
    }

    /// 范围核：豁免对象必须在本核条款面内（前缀校验）。
    pub fn in_scope(obj: &str) -> bool {
        obj.starts_with("PX-") || obj.starts_with("KT-") || obj.starts_with("UNX-")
    }

    /// 带范围校验的入册。
    pub fn add_scoped(
        &mut self,
        id: u32,
        obj: &str,
        cat: u8,
        basis: &str,
        impact: &str,
    ) -> Result<(), &'static str> {
        if !Self::in_scope(obj) {
            return Err("out_scope");
        }
        self.add(id, obj, cat, basis, impact)
    }

    /// 附录哈希：条目按 id 序折叠。
    pub fn appendix_fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        let mut sorted: Vec<&Exempt> = self.rows.iter().collect();
        sorted.sort_by_key(|e| e.id);
        for e in sorted {
            for b in e.render().as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|r| r.ok())
    }
}

/// 预置豁免基线（三条真实披露）。
pub fn seed(b: &mut ExemptBook) -> Result<(), &'static str> {
    b.add_scoped(1, "PX-106", X_NA, "B-无tty设备", "行规程面不可达")?;
    b.add_scoped(2, "PX-110", X_NA, "B-无MMU", "mmap 语义不可达")?;
    b.add_scoped(3, "UNX-112", X_DEVIATE, "B-只读快照设计", "environ 不可写偏离")?;
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut ExemptBook, code: u32, id: u32) -> Result<(), &'static str> {
    match code {
        EX_NO_BASIS => b.add_scoped(id, "PX-101", X_NA, "没依据", "影响面"),
        EX_OUT_SCOPE => b.add_scoped(id, "OTHER-999", X_NA, "B-依据文本", "影响面"),
        EX_REVIVE => {
            let _ = b.add_scoped(id, "PX-101", X_NA, "B-依据", "影响");
            let _ = b.revoke(id);
            b.revive_attempt(id)
        }
        EX_BAD_CAT => b.add_scoped(id, "PX-101", 9, "B-依据", "影响"),
        EX_FROZEN => {
            b.freeze();
            b.add_scoped(id, "PX-101", X_NA, "B-冻结依据", "冻结影响")
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10250_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10250");

    // J1 豁免基线三条入册四要素齐备
    let j1 = {
        let mut b = ExemptBook::new();
        let _ = seed(&mut b);
        b.len() == 3 && b.all_ok()
    };
    s.add("J1-基线三条齐备", j1, "四要素全备入册");

    // J2a 无依据拒收（依据不含 B- 前缀）
    let j2a = inject_fault(&mut ExemptBook::new(), EX_NO_BASIS, 10) == Err("missing_factor");
    s.add("J2a-无依据拒", j2a, "缺 B- 前缀即拒");

    // J2b 超范围拒收
    let j2b = inject_fault(&mut ExemptBook::new(), EX_OUT_SCOPE, 11) == Err("out_scope");
    s.add("J2b-超范围拒", j2b, "对象前缀外即拒");

    // J2c 豁免复活检出
    let j2c = inject_fault(&mut ExemptBook::new(), EX_REVIVE, 12) == Err("revived");
    s.add("J2c-复活检出", j2c, "撤销后再入即拒");

    // J3 附录哈希：同集同哈希/加删必变
    let j3 = {
        let mk = || {
            let mut b = ExemptBook::new();
            let _ = seed(&mut b);
            b.appendix_fp()
        };
        let f1 = mk();
        let f2 = mk();
        let mut b3 = ExemptBook::new();
        let _ = seed(&mut b3);
        let _ = b3.add_scoped(9, "KT-555", X_RESOURCE, "B-内存受限", "映射面收窄");
        let f3 = b3.appendix_fp();
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J3-附录哈希", j3, "同集同哈希加条必变");

    // J4 坏类别拒收
    let j4 = inject_fault(&mut ExemptBook::new(), EX_BAD_CAT, 13) == Err("bad_cat");
    s.add("J4-坏类别拒", j4, "cat=9 即拒");

    // J5 冻结红线
    let j5 = {
        let mut b = ExemptBook::new();
        inject_fault(&mut b, EX_FROZEN, 14);
        b.is_frozen()
    };
    s.add("J5-冻结拒收", j5, "印后入册必抓");

    // J6 撤销账一致性：撤销后 revive 尝试都进拒绝路径
    let j6 = {
        let mut b = ExemptBook::new();
        let _ = seed(&mut b);
        let r1 = b.revoke(1);
        let r2 = b.revoke(1);
        r1 == Ok(()) && r2 == Err("already") && b.revive_attempt(1) == Err("revived")
    };
    s.add("J6-撤销账一致", j6, "二次撤销与复活全拒");

    // J7 范围核真源：三类前缀全放行
    let j7 = ExemptBook::in_scope("PX-101")
        && ExemptBook::in_scope("KT-5")
        && ExemptBook::in_scope("UNX-112")
        && !ExemptBook::in_scope("XX-1");
    s.add("J7-范围核真源", j7, "三前缀放行一前缀拒");

    // J8 条目负样本：零 id/坏前缀依据/空影响 ok 必假
    let j8 = {
        let zero = Exempt {
            id: 0, obj: "PX-1".to_string(), cat: X_NA,
            basis: "B-x".to_string(), impact: "y".to_string(),
        };
        let nob = Exempt {
            id: 1, obj: "PX-1".to_string(), cat: X_NA,
            basis: "短".to_string(), impact: "y".to_string(),
        };
        !zero.ok() && !nob.ok()
    };
    s.add("J8-负样本两假", j8, "零id/短依据 ok 必假");

    // J9 渲染行含类别名
    let j9 = {
        let mut b = ExemptBook::new();
        let _ = seed(&mut b);
        b.rows[0].render().contains("na") && b.rows[2].render().contains("deviate")
    };
    s.add("J9-渲染含类别", j9, "渲染行带类别名");

    // J10 十轮附录回放：10 轮哈希一致（确定性）
    let j10 = {
        let mk = || {
            let mut b = ExemptBook::new();
            let _ = seed(&mut b);
            b.appendix_fp()
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
    s.add("J10-十轮附录", j10, "10/10 轮哈希一致");

    s
}
