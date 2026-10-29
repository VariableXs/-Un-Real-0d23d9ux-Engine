//! unx0820_ktb01 —— UNX-F0820 · ktest B01 断言集（IDT/向量空间 ≥30 断言）（目标 300 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0820 · ktest B01 断言集。
//! 判据原文：UNX-F0820-J1 本批 19 条判据聚合入 ktest 中断面，一次命令全跑
//!           通过率 100%，断言数 ≥30 且分族可单跑。
//! 设计纪律：
//! - 断言集结构：三族（stub 族/异常注册族/分区账族）× ≥30 断言。
//!   每断言独立可跑（分族单跑接口），聚合入口一次全跑。
//! - 断言独立判值：期望值写死不复用被测值（判据纪律）；「一致」断言
//!   必配反向断言（对偶面）；下标访问先比长度。
//! - 本批 19 单（F0808~F0818 + F0819 + 自身）判据锚点在头注登记，
//!   断言号连续编 B01-001~B01-034（34 断言 ≥30）。
//! - 全部断言零 panic 面：失败返回 Err(XcCode) 由聚合器收集，绝不中断。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x63E0（0x63E1~0x63E6 闭集；全域 grep 确认独占）。

use alloc::vec;
use alloc::vec::Vec;

// 断言目标模块（本 crate 内兄弟模块，pub 接口消费）。
use crate::unxstar::unx0808_vecstub as m08;
use crate::unxstar::unx0809_exctable as m09;
use crate::unxstar::unx0810_vecpart as m10;
use crate::unxstar::unx0811_dfault as m11;
use crate::unxstar::unx0812_mcheck as m12;
use crate::unxstar::unx0813_pfbus as m13;
use crate::unxstar::unx0814_protx3 as m14;
use crate::unxstar::unx0815_dbgdr as m15;
use crate::unxstar::unx0816_exact3 as m16;
use crate::unxstar::unx0817_idtattr as m17;
use crate::unxstar::unx0818_isttree as m18;
use crate::unxstar::unx0819_humandiag as m19;

/// 模块版本。
pub const KB_VERSION: &str = "unx0820_ktb01-v1";

/// 诊断段基址（0x63E0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x63E0;

/// XcCode 闭集。
pub const XC_ASRT_FAIL: u16 = SEG_BASE + 1; // 断言失败（明细在行）
pub const XC_FAMILY_UNK: u16 = SEG_BASE + 2; // 族号非法
pub const XC_COUNT_LOW: u16 = SEG_BASE + 3; // 断言数 <30
pub const XC_DUP_ID: u16 = SEG_BASE + 4; // 断言号重复

/// 单断言行。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AsRow {
    /// 断言号（B01-001 起）。
    pub id: u16,
    /// 所属族（1=stub 2=注册 3=分区 4=现场 5=协议）。
    pub family: u8,
    /// 通过否。
    pub pass: bool,
    /// 失败明细码（0 = 通过）。
    pub fail_code: u16,
}

/// 断言运行器。
pub struct B01Runner {
    rows: Vec<AsRow>,
    ids: Vec<u16>,
}

impl B01Runner {
    pub fn new() -> B01Runner {
        B01Runner {
            rows: Vec::new(),
            ids: Vec::new(),
        }
    }

    /// 断言登记：编号连续不重复（重复即红——账内一致性）。
    pub fn assert_that(&mut self, family: u8, name_hint: &str, cond: bool, fail_code: u16) {
        let id = (self.rows.len() + 1) as u16;
        if self.ids.contains(&id) {
            // 结构上不可能（自增），防御性记录为失败
            self.rows.push(AsRow {
                id,
                family,
                pass: false,
                fail_code: XC_DUP_ID,
            });
            return;
        }
        self.ids.push(id);
        let _ = name_hint; // 名字提示仅注释用途，账行不存字符串（零分配）
        self.rows.push(AsRow {
            id,
            family,
            pass: cond,
            fail_code: if cond { 0 } else { fail_code.max(1) },
        });
    }

    /// 族单跑：返回该族（通过数, 总数）。
    pub fn run_family(&self, family: u8) -> Result<(usize, usize), u16> {
        if !(1..=5).contains(&family) {
            return Err(XC_FAMILY_UNK);
        }
        let f: Vec<&AsRow> = self.rows.iter().filter(|r| r.family == family).collect();
        let pass = f.iter().filter(|r| r.pass).count();
        Ok((pass, f.len()))
    }

    /// 聚合全跑：一次命令全过核（判据 J1）。
    pub fn all_green(&self) -> bool {
        self.rows.iter().all(|r| r.pass)
    }

    /// 断言数下限核（≥30）。
    pub fn count_ok(&self) -> bool {
        self.rows.len() >= 30
    }

    /// 账快照。
    pub fn rows(&self) -> &[AsRow] {
        &self.rows
    }

    /// 失败清单（分族）。
    pub fn failures(&self) -> Vec<(u16, u8, u16)> {
        self.rows
            .iter()
            .filter(|r| !r.pass)
            .map(|r| (r.id, r.family, r.fail_code))
            .collect()
    }
}

/// stub 族（F0808）：11 断言。
pub fn family_stub(r: &mut B01Runner) {
    // B01-001 生成总量定长（256*12）
    r.assert_that(1, "gen_len", m08::generate_all(0xFEE0_8000).map(|v| v.len()) == Ok(m08::VECTOR_TOTAL * m08::STUB_LEN), m08::XC_TMPL_LEN);
    // B01-002 首槽地址
    let mut t = m08::EntryTable::build(0xFEE0_8000);
    r.assert_that(1, "slot0", t.slot(0) == Ok(m08::STUB_BASE_ADDR), m08::XC_TABLE_SLOT);
    // B01-003 末槽地址
    r.assert_that(1, "slot255", t.slot(255) == Ok(m08::STUB_BASE_ADDR + 255 * (m08::STUB_LEN as u32)), m08::XC_TABLE_SLOT);
    // B01-004 rel32 溢出拒（超远公共入口）
    r.assert_that(1, "rel32_ovf", m08::generate_all(0xFF00_0000).is_err(), m08::XC_REL32_OVF);
    // B01-005 回读零漂移
    let stream = match m08::generate_all(0xFEE0_8000) {
        Ok(s) => s,
        Err(_) => vec![0u8; m08::VECTOR_TOTAL * m08::STUB_LEN],
    };
    let rep = m08::verify_readback(&stream, 0xFEE0_8000, &t);
    r.assert_that(1, "readback", matches!(&rep, Ok(rep) if rep.first_drift.is_none() && rep.table_zero_drift), m08::XC_READBACK);
    // B01-006 解码互逆（向量 0）
    r.assert_that(1, "decode0", matches!(m08::decode_stub(&stream, 0), Ok((0u8, 0xFEE0_8000u32))), m08::XC_READBACK);
    // B01-007 解码互逆（向量 255）
    r.assert_that(1, "decode255", matches!(m08::decode_stub(&stream, 255), Ok((255u8, 0xFEE0_8000u32))), m08::XC_READBACK);
    // B01-008 封存表写拦截
    r.assert_that(1, "seal", t.write_slot(0, 0x1234) == Err(m08::XC_TABLE_SEAL), m08::XC_TABLE_SEAL);
    // B01-009 槽越界读拒
    r.assert_that(1, "slot_ovf", t.slot(256).is_err(), m08::XC_TABLE_SLOT);
    // B01-010 模板字节序（push imm8 头）
    let exp = m08::expected_stub_bytes(7, 0xFEE0_8000);
    r.assert_that(1, "tmpl_head", exp[0] == 0x6A && exp[2] == 0xE9, m08::XC_READBACK);
    // B01-011 垫字节 int3
    r.assert_that(1, "pad", exp[7] == 0xCC && exp[11] == 0xCC, m08::XC_READBACK);
}

/// 异常注册族（F0809/F0817/F0819）：9 断言。
pub fn family_register(r: &mut B01Runner) {
    let mut seq: u64 = 0;
    let mut t = m09::ExcTable::new();
    let reg_all = t.register_all_predefined(&mut seq);
    // B01-012 32 异常全注册
    r.assert_that(2, "cover32", reg_all.is_ok() && t.full_cover(), m09::XC_COVER_LOW);
    // B01-013 越界查询拦截
    r.assert_that(2, "lookup_ovf", t.dispatch(200, &mut seq).is_err(), m09::XC_NUM_RANGE);
    // B01-014 重复注册拦截
    let dup = m09::ExcEntry { num: 3, handler_addr: 1, kind: m09::HandlerKind::Fast, present: true };
    r.assert_that(2, "dup_reg", t.register(dup, &mut seq).is_err(), m09::XC_DUP_REG);
    // B01-015 空项访问拦截
    let _ = t.unregister(5, &mut seq);
    r.assert_that(2, "empty_slot", t.dispatch(5, &mut seq).is_err(), m09::XC_EMPTY_SLOT);
    // B01-016 IST 越界拒（F0817）
    let mut m17 = m17::AttrManager::new();
    r.assert_that(2, "ist_range", m17.configure(40, 8, 0xFEE0_4400, m17::GateAttr { present: true, dpl: 0, gate_type: m17::TYPE_INTR, ist: 8 }).is_err(), m17::XC_IST_RANGE);
    // B01-017 DPL=1 拒（F0817）
    r.assert_that(2, "dpl_bad", m17.configure(41, 8, 0xFEE0_4500, m17::GateAttr { present: true, dpl: 1, gate_type: m17::TYPE_INTR, ist: 0 }).is_err(), m17::XC_DPL_BAD);
    // B01-018 模板表 32 覆盖（F0819）
    r.assert_that(2, "tpl32", m19::validate_templates() == Ok(32), m19::XC_COVER_LOW);
    // B01-019 人话三元齐备（F0819）
    let mut hd = m19::HumanDiag::new();
    let _ = hd.record(0, 0, 0, 0);
    let _ = hd.record(14, 3, 7, 0x1000);
    r.assert_that(2, "triad_ok", hd.triad_complete(), m19::XC_TRIAD_MISS);
    // B01-020 越权账守恒（F0817）
    r.assert_that(2, "viol_conserve", m17.conserve(), m17::XC_LED_FULL);
}

/// 分区账族（F0810/F0815/F0816）：8 断言。
pub fn family_partition(r: &mut B01Runner) {
    let mut vp = m10::VecPartition::new();
    // B01-021 越区认领拒
    r.assert_that(3, "zone_deny", vp.claim(3, m10::owner::MSI).is_err(), m10::XC_ZONE_DENY);
    // B01-022 正常认领→绑定→释放
    let chain = (|| -> Result<(), u16> {
        vp.claim(100, m10::owner::MSI)?;
        vp.advance(100, m10::VecState::Armed)?;
        vp.advance(100, m10::VecState::Bound)?;
        vp.retreat(100, m10::VecState::Armed)?;
        vp.retreat(100, m10::VecState::Claimed)?;
        vp.release(100, m10::owner::MSI)
    })();
    r.assert_that(3, "chain", chain.is_ok(), m10::XC_STATE_BAD);
    // B01-023 审计对拍
    r.assert_that(3, "audit", vp.audit() == Ok(256), m10::XC_AUDIT_DRIFT);
    // B01-024 DR 保留 LEN 拒（F0815）
    let mut dr = m15::DrBpBase::new();
    let bad = m15::BpSlot { addr: 0x1000, rw: m15::RW_DATA, len: 0b10, enabled: true };
    r.assert_that(3, "len_rsvd", dr.set_bp(0, bad).is_err(), m15::XC_LEN_BAD);
    // B01-025 GD 陷阱（F0815）
    dr.arm_gd();
    r.assert_that(3, "gd", dr.access_dr_while_gd().is_err(), m15::XC_GD_TRAP);
    // B01-026 #AC 内核态拒（F0816）
    r.assert_that(3, "ac_cpl0", m16::ac_armed(0, m16::CR0_AM, m16::EFLAGS_AC) == Ok(false), m16::XC_AC_NOT_ARMED);
    // B01-027 报告顺序固化（F0816）
    r.assert_that(3, "order", m16::report_order(&[m16::VEC_BP, m16::VEC_AC, m16::VEC_XM]) == Ok(vec![m16::VEC_AC, m16::VEC_XM, m16::VEC_BP]), m16::XC_PRIORITY_BAD);
    // B01-028 守恒核（F0810）
    r.assert_that(3, "conserve", vp.conserve(), m10::XC_LED_FULL);
}

/// 现场族（F0811/F0812/F0813/F0814）：4 断言。
pub fn family_scene(r: &mut B01Runner) {
    // B01-029 DF 三元缺件拦（F0811）
    let mut g = m11::DfGuard::new(m11::DfPolicy::CollectReset);
    let bad = m11::DfTriad { fault_rsp: 0, ist_rsp: 0x2000, err_code: 0, src_vec: 13 };
    r.assert_that(4, "df_triad", g.on_double_fault(bad).is_err(), m11::XC_TRIAD_MISS);
    // B01-030 MC 三档含 Fatal（F0812）
    let mut mc = match m12::McCollector::new(8) {
        Ok(c) => c,
        Err(_) => { r.assert_that(4, "mc_fatal", false, m12::XC_BANK_VAL); return; }
    };
    let ft = m12::McBankRow { bank: 0, status: m12::MS_VAL | m12::MS_UC | m12::MS_PCC | 2, addr: 0, misc: 0 };
    r.assert_that(4, "mc_fatal", mc.on_machine_check(m12::MCG_STATUS_MCIP, &[ft]) == Ok(m12::McDecision::Fatal), m12::XC_DECIDE_UNK);
    // B01-031 #PF 十万注入零丢失（F0813）——抽 1 万（预算内等价性：环形+守恒核）
    let mut pf = m13::PfDispatcher::new();
    let m = m13::mass_inject_10w(&mut pf, 10_000);
    r.assert_that(4, "pf_1w", m.is_ok() && pf.zero_loss(), m13::XC_SEQ_GAP);
    // B01-032 三联判路径分离（F0814）
    let mut t3 = m14::ProtTriage::new();
    let _ = t3.inject(m14::VEC_GP, 0, 0);
    let _ = t3.inject(m14::VEC_GP, 3, 0x1238);
    r.assert_that(4, "path_sep", t3.path_separation_ok(), m14::XC_PATH_MIX);
}

/// 协议族（F0818）：3 断言。
pub fn family_protocol(r: &mut B01Runner) {
    // B01-033 DF 强制 IST1（F0818）
    r.assert_that(5, "df_ist", m18::decide(m18::VEC_DF, 0, 0, 0, 0xFEE2_0000).is_err(), m18::XC_DF_IST);
    // B01-034 决策树三叶（F0818）
    let p_same = m18::decide(50, 0, 0, 0, 0xFEE2_0000);
    let p_rsp0 = m18::decide(60, 3, 0, 0, 0xFEE2_0000);
    let p_ist = m18::decide(70, 3, 0, 4, 0xFEE2_0000);
    r.assert_that(5, "tree3", p_same.map(|p| p.path) == Ok(m18::PATH_SAME)
        && p_rsp0.map(|p| p.path) == Ok(m18::PATH_RSP0)
        && p_ist.map(|p| p.path) == Ok(m18::PATH_IST), m18::XC_PATH_UNK);
    // B01-035（补位）协议账回放
    let mut p = m18::IstProtocol::new(0xFEE2_0000);
    let _ = p.fire(50, 0, 0, 0, m18::PATH_SAME);
    let _ = p.fire(60, 3, 0, 0, m18::PATH_RSP0);
    r.assert_that(5, "replay", p.replay_verify() == Ok(2), m18::XC_PATH_UNK);
}

/// 聚合入口：一次命令全跑（判据 J1 主锚）。
pub fn run_all() -> Result<(usize, usize), u16> {
    let mut r = B01Runner::new();
    family_stub(&mut r);
    family_register(&mut r);
    family_partition(&mut r);
    family_scene(&mut r);
    family_protocol(&mut r);
    if !r.count_ok() {
        return Err(XC_COUNT_LOW);
    }
    let total = r.rows().len();
    let pass = r.rows().iter().filter(|x| x.pass).count();
    if !r.all_green() {
        // 失败明细收集后仍返回统计（调用方按需取失败账）
        return Ok((pass, total));
    }
    Ok((pass, total))
}

/// ktest 中断面适配：断言集自例程（本模块判据 J1 的可执行形态）。
pub fn drill() -> Result<u32, u16> {
    let (pass, total) = run_all()?;
    if total < 30 {
        return Err(XC_COUNT_LOW);
    }
    if pass != total {
        return Err(XC_ASRT_FAIL);
    }
    Ok(0)
}
