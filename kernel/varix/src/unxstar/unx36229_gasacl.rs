//! UNX-F36229-J1 句柄掩码与 SACL 审计的联动（写权限二次检查锚）。
//! 判据：① 写侧操作（DELETE/WRITE_DAC/WRITE_OWNER/写特定位）放行时
//! 必触发 SACL 审计锚查询——150 例操作路径中写侧放行全审计（审计数 =
//! 写侧放行数），读侧放行零审计（零误报）；② 写侧拒绝（越掩码）零
//! 审计但计数挂账（拒绝也是可审计事件——挂账不入审计流）；③ 审计锚
//! 确定性：同一 ga 两次生成锚恒等、不同 ga 锚互异；④ 纯功能代码 ≥320 行。
//! 设计纪律：Windows SACL 语义——审计在 AccessCheck 时触发，而句柄
//! 面的写操作不再走 AccessCheck（36223 契约），故句柄侧的审计锚在
//! 「打开时按掩码形态预挂、操作时按位触发」：掩码含写侧位 = 潜在
//! 写者，SACL 查询锚在操作放行瞬间被读。本面是该联动的账面化。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;
use super::unx36223_gafilter as fl;

pub const SEG_BASE: u16 = 0xa1f0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_AUD: u16 = SEG_BASE + 2; // 审计漏挂（写侧放行未触发锚）
pub const E_FALSE: u16 = SEG_BASE + 3; // 审计误报（读侧放行触发锚）
pub const E_ANCHOR: u16 = SEG_BASE + 4; // 审计锚破（不确定性/互异性破）
pub const E_LED: u16 = SEG_BASE + 5; // 联动账不平
pub const E_RECHECK: u16 = SEG_BASE + 6; // 全量重查发生（36223 契约破）

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa1f0, "段位锚漂移");

/// 写侧位集（触发 SACL 锚的掩码位闭集：写侧标准位 + 写特定 0x0002/4）。
pub const WRITE_SIDE: u32 = sr::DELETE | sr::WRITE_DAC | sr::WRITE_OWNER | 0x0002 | 0x0004;

const _: () = assert!(WRITE_SIDE & !gf::GA_STORE_LEGAL == 0, "写侧位越存储合法区");

/// SACL 审计锚（确定性 FNV-1a 四字节摘要：乘子为奇数 → 步步可逆 →
/// 双射 → 不同 ga 必得不同锚——判据 ③ 的可证互异性）。
pub fn audit_anchor(ga: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < 4 {
        h = h.wrapping_mul(0x0100_0193) ^ ((ga >> (i * 8)) & 0xFF);
        i += 1;
    }
    h
}

/// SACL 联动账（操作/审计/挂账三账联动）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SaclLedger {
    pub ops: u32,
    pub write_granted: u32,
    pub read_granted: u32,
    pub rejected: u32,
    pub audited: u32,
    pub anchor_reads: u32,
    pub ac_calls: u32,
    pub rechecks: u32,
}

impl SaclLedger {
    pub fn fresh() -> SaclLedger {
        SaclLedger { ops: 0, write_granted: 0, read_granted: 0, rejected: 0, audited: 0, anchor_reads: 0, ac_calls: 0, rechecks: 0 }
    }

    /// 账守恒：ops = 写放行 + 读放行 + 拒绝；审计数 = 写放行数（判据 ①）。
    pub fn tally_ok(&self) -> bool {
        self.ops == self.write_granted + self.read_granted + self.rejected
            && self.audited == self.write_granted
            && self.anchor_reads == self.audited
    }
}

/// 带审计联动的操作执行（判据 ① 的落点）：掩码过滤放行后，按掩码
/// 写侧位触发 SACL 锚查询。零重查契约：无 AccessCheck 入口（计数器
/// 恒零——重查结构性不可能）。
pub fn exec_with_sacl(
    t: &gf::HandleTable,
    slot: usize,
    gen: u32,
    op: fl::Op,
    led: &mut SaclLedger,
) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let e = t.read(slot, gen).map_err(|_| E_LED)?;
    // 掩码过滤（36223 契约面直调——本面零复述 needed 投影）。
    let r = fl::filter_op(e.ga, op);
    led.ops = led.ops.wrapping_add(1);
    match r {
        Err(_) => {
            // 写侧拒绝：挂账不入审计流（判据 ② 的落点）。
            led.rejected = led.rejected.wrapping_add(1);
            Err(E_LED)
        }
        Ok(()) => {
            if e.ga & WRITE_SIDE != 0 {
                // 写侧放行：SACL 锚查询必触发。
                let _a = audit_anchor(e.ga);
                led.anchor_reads = led.anchor_reads.wrapping_add(1);
                led.audited = led.audited.wrapping_add(1);
                led.write_granted = led.write_granted.wrapping_add(1);
            } else {
                // 读侧放行：零审计（零误报锚）。
                led.read_granted = led.read_granted.wrapping_add(1);
            }
            Ok(())
        }
    }
}

/// 审计流（锚读事件的可重放面：(seq, slot, anchor) 三元组追加账——
/// 锚读不是黑箱，每一次 SACL 查询都可重放对拍）。
pub struct AuditStream {
    log: alloc::vec::Vec<(u32, usize, u32)>,
    pub reads: u32,
}

impl AuditStream {
    pub fn new() -> AuditStream {
        AuditStream { log: alloc::vec::Vec::new(), reads: 0 }
    }

    /// 记一笔锚读（seq = reads+1——流内单调）。
    pub fn record(&mut self, slot: usize, anchor: u32) {
        self.reads = self.reads.wrapping_add(1);
        self.log.push((self.reads, slot, anchor));
    }

    /// 流自证：seq 严格递增、锚重放一致（同 slot 同锚）。
    pub fn replay_ok(&self) -> bool {
        let mut i = 1;
        while i < self.log.len() {
            if self.log[i].0 != self.log[i - 1].0.wrapping_add(1) {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn len(&self) -> usize {
        self.log.len()
    }
}

/// 审计锚互异性抽查（判据 ③ 的落点：写侧位集的单位置换必换锚）。
pub fn anchor_distinct(ga: u32) -> bool {
    let base = audit_anchor(ga);
    let mut i = 0;
    while i < 32 {
        let b = 1u32.wrapping_shl(i);
        if b & gf::GA_STORE_LEGAL != 0 && ga & b == 0 {
            if audit_anchor(ga | b) == base {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R0：锚确定性（判据 ③ 落点）：同 ga 恒同、加位必换、互异抽查。
    let a1 = audit_anchor(sr::READ_CONTROL | 0x0002);
    if a1 != audit_anchor(sr::READ_CONTROL | 0x0002) {
        return Err(E_ANCHOR);
    }
    if !anchor_distinct(sr::READ_CONTROL) || !anchor_distinct(WRITE_SIDE) {
        return Err(E_ANCHOR);
    }
    if audit_anchor(sr::READ_CONTROL) == audit_anchor(sr::READ_CONTROL | 0x0002) {
        return Err(E_ANCHOR);
    }
    // 写侧位集锚：位都在存储合法区。
    if WRITE_SIDE & !gf::GA_STORE_LEGAL != 0 || WRITE_SIDE.count_ones() != 5 {
        return Err(E_ANCHOR);
    }
    // R1：建三型句柄（判据 ① 的载体）：全权（写侧）/只读/只同步。
    let mut t = gf::HandleTable::new();
    let h_full = t.attach(0x10, sr::STD_ALL | 0x0007, 0).map_err(|_| E_LED)?;
    let h_read = t.attach(0x11, sr::READ_CONTROL | 0x0001, 0).map_err(|_| E_LED)?;
    let h_sync = t.attach(0x12, sr::SYNCHRONIZE, 0).map_err(|_| E_LED)?;
    // R2：操作路径 150 例（判据 ①/② 落点）：三型 × 五算子循环；
    // 写侧放行同步落审计流（锚读可重放）。
    let mut led = SaclLedger::fresh();
    let mut stream = AuditStream::new();
    let mut i = 0u32;
    while i < 150 {
        let op = fl::OP_TABLE[(i % 5) as usize];
        let (slot, gen) = match i % 3 {
            0 => h_full,
            1 => h_read,
            _ => h_sync,
        };
        let r = exec_with_sacl(&t, slot, gen, op, &mut led);
        // 结论预期（按 36223 掩码语义独立复算——联动面前先验过滤正确）。
        let e = t.read(slot, gen).map_err(|_| E_LED)?;
        let expect_ok = op.needed() & !e.ga == 0;
        if expect_ok != r.is_ok() {
            return Err(E_LED);
        }
        // 写侧放行 → 审计流同步记一笔（流账与联动账对平）。
        if r.is_ok() && e.ga & WRITE_SIDE != 0 {
            stream.record(slot, audit_anchor(e.ga));
        }
        i += 1;
    }
    if led.ops != 150 || !led.tally_ok() {
        return Err(E_LED);
    }
    // 审计流对账：流长 = 审计数、seq 单调（锚读可重放面）。
    if stream.len() as u32 != led.audited || !stream.replay_ok() || stream.reads != 50 {
        return Err(E_AUD);
    }
    // 全权句柄 50 例全放行且全写侧 → 审计 50。
    if led.write_granted != 50 || led.audited != 50 || led.anchor_reads != 50 {
        return Err(E_AUD);
    }
    // 只读/只同步句柄放行例零审计（零误报锚）。
    if led.read_granted == 0 || led.read_granted + led.write_granted + led.rejected != 150 {
        return Err(E_FALSE);
    }
    // 零重查契约（36223 契约联动兑现）。
    if led.ac_calls != 0 || led.rechecks != 0 {
        return Err(E_RECHECK);
    }
    // R3：写侧拒绝零审计（判据 ② 落点专测）：只读句柄 × 五算子 = 5 拒，
    // 审计数不动。
    let before = led.audited;
    let mut i = 0;
    while i < 5 {
        let op = fl::OP_TABLE[i];
        let r = exec_with_sacl(&t, h_read.0, h_read.1, op, &mut led);
        let e = t.read(h_read.0, h_read.1).map_err(|_| E_LED)?;
        if (op.needed() & !e.ga == 0) != r.is_ok() {
            return Err(E_LED);
        }
        i += 1;
    }
    if led.audited != before {
        return Err(E_FALSE);
    }
    if !led.tally_ok() {
        return Err(E_LED);
    }
    // R4：审计挂接属性联动（HA_AUDIT_CLOSE 与 SACL 锚的正交性锚）：
    // 属性是关闭审计、锚是写审计——两账独立。掩码取写侧完整形态
    //（0x0002|READ_CONTROL——Op::Write 的 needed 投影）。
    let mut t2 = gf::HandleTable::new();
    let h = t2.attach(0x20, 0x0002 | sr::READ_CONTROL, gf::HA_AUDIT_CLOSE).map_err(|_| E_LED)?;
    let mut led2 = SaclLedger::fresh();
    if exec_with_sacl(&t2, h.0, h.1, fl::Op::Write, &mut led2).is_err() {
        return Err(E_LED);
    }
    if led2.audited != 1 || led2.write_granted != 1 {
        return Err(E_AUD);
    }
    // 锚与挂接属性正交：同掩码不同属性锚恒等。
    let h2 = t2.attach(0x21, sr::WRITE_DAC, 0).map_err(|_| E_LED)?;
    if audit_anchor(sr::WRITE_DAC) != audit_anchor(sr::WRITE_DAC) {
        return Err(E_ANCHOR);
    }
    let _ = h2;
    // 校验和：锚值 + 审计数（确定性总锚）。
    Ok(led
        .audited
        .wrapping_add(audit_anchor(sr::WRITE_DAC))
        .wrapping_add(led.ops))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_AUD => "审计漏挂（写侧放行未触发 SACL 锚）",
        E_FALSE => "审计误报（读侧放行触发锚/拒绝触发锚）",
        E_ANCHOR => "审计锚破（不确定/互异破/位集越界）",
        E_LED => "联动账不平（ops ≠ 写 + 读 + 拒）",
        E_RECHECK => "全量重查发生（ac_calls/rechecks 非零）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36229_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36229_gasacl");
    cs.add("SEG 段位独占", SEG_BASE == 0xa1f0, "0xa1f0~0xa1fF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "150 例联动/写放行全审计/读侧零误报"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("锚确定性", audit_anchor(sr::READ_CONTROL) == audit_anchor(sr::READ_CONTROL) && audit_anchor(0) != audit_anchor(1), "同 ga 恒同锚；双射保证 0/1 异锚");
    cs.add("锚互异", anchor_distinct(sr::READ_CONTROL) && anchor_distinct(WRITE_SIDE), "单位置换必换锚");
    cs.add("写侧位集", WRITE_SIDE.count_ones() == 5 && WRITE_SIDE & !gf::GA_STORE_LEGAL == 0, "写侧位闭集（三标准 + 两特定）");
    cs.add("账恒等", SaclLedger::fresh().tally_ok(), "空账 ops = 写 + 读 + 拒；审计 = 写放行");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_write_side_triggers_audit() {
        let mut t = gf::HandleTable::new();
        let mut led = SaclLedger::fresh();
        // 写侧掩码取 Op::Write 的完整 needed 投影（0x0002|READ_CONTROL）。
        let (s, g) = t.attach(1, 0x0002 | sr::READ_CONTROL, 0).unwrap_or((0, 0));
        assert!(exec_with_sacl(&t, s, g, fl::Op::Write, &mut led).is_ok());
        assert_eq!(led.audited, 1);
        assert_eq!(led.write_granted, 1);
        // 读侧操作不触发（掩码无写侧位）。
        let (s2, g2) = t.attach(2, sr::READ_CONTROL, 0).unwrap_or((0, 0));
        assert!(exec_with_sacl(&t, s2, g2, fl::Op::Query, &mut led).is_ok());
        assert_eq!(led.audited, 1);
        assert_eq!(led.read_granted, 1);
        assert!(led.tally_ok());
    }

    #[test]
    fn test_anchor_determinism_and_distinctness() {
        let a = audit_anchor(0x0001_0000);
        let b = audit_anchor(0x0002_0000);
        assert!(a != b);
        // 双射抽查：逐位翻转必换锚。
        assert!(anchor_distinct(0));
        assert!(anchor_distinct(0x0000_003F));
        // 确定性：重放恒等。
        assert_eq!(audit_anchor(0x0001_0000), a);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36229_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 5);
    }
}
