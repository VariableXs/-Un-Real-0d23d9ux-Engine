//! cgd15_d01close — CGPU-F0496 D01 组收口与 D02 移交（组不收口不算完，移交
//! 要有单据）。
//!
//! 判据原文（施工书锚点 CGPU-F0496）：D01 组收口：16 项自检入树/计时精度
//! 实测报告/组双签/台账更新/向 D02 抖动抑制与帧间隔均衡组移交（帧计时与
//! 合同校验就绪——抑制与均衡有了精确的测量基础）。判据：入树、报告、双
//! 签、移交、判据。
//!
//! 职责定位：16 项自检清单核验器（D01 帧计时组全部域单的判据集汇总——逐
//! 项点名，缺一不收口）+ 计时精度实测报告单（三源残余/开销比/吞吐/查询四
//! 口径实测值固化成报告——报告即快照指纹，事后不可改）+ 组双签纪律（两组
//! 签名：施工方自签+验收方核签——单签无效，签名绑定组指纹）+ 台账更新单
//! （收口条目落组台账——编号/时间/指纹）+ D02 移交单（帧计时与合同校验
//! 就绪清单——测量基础三件套：口径表/账本查询 API/精度声明，D02 抖动抑制
//! 与帧间隔均衡组凭此开工）。红线：缺项收口=虚假收口（16 项必须逐项真实
//! 绿——绿是跑出来的不是抄出来的）；移交单不带测量基础=空手移交（拒收）。
//! 诊断码段 0x6201~0x6205。

use alloc::vec::Vec;

/// 模块版本。
pub const D01_CLOSE_VERSION: &str = "D15-d01close-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x62 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_SELFCHECK_SHORT: u32 = 0x6201; // 自检缺项（16 项不满——虚假收口）
pub const XC_SINGLE_SIGN: u32 = 0x6202; // 单签收口（双签纪律违约）
pub const XC_REPORT_MUTATED: u32 = 0x6203; // 报告快照被改（指纹失配）
pub const XC_EMPTY_HANDOVER: u32 = 0x6204; // 空手移交（测量基础缺失）
pub const XC_LEDGER_NOT_UPDATED: u32 = 0x6205; // 台账未更新（收口条目缺账）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// D01 组自检项数（判据"16 项自检"）。
pub const D01_SELFCHECK_ITEMS: usize = 16;
/// 报告口径数（精度实测四口径）。
pub const REPORT_METRICS: usize = 4;
/// 移交测量基础件数（三件套）。
pub const HANDOVER_ITEMS: usize = 3;

/// 自检项条目（点名制——每项带归属域单）。
#[derive(Clone, Debug, PartialEq)]
pub struct SelfCheckItem {
    /// 项号（1..=16）。
    pub no: usize,
    /// 归属域单（如 cgd02_frametimer）。
    pub owner: &'static str,
    /// 判据摘要。
    pub brief: &'static str,
    /// 实测绿/红。
    pub passed: bool,
}

/// 精度实测报告（快照指纹锁定——判据"计时精度实测报告"）。
#[derive(Clone, Debug, PartialEq)]
pub struct PrecisionReport {
    /// 三源残余误差（微秒，绝对值）。
    pub residual_us: [i64; 3],
    /// 计时开销比（ppm）。
    pub overhead_ppm: u64,
    /// 账本写入摊销（步/条）。
    pub write_amort: u64,
    /// 查询延迟（步/次）。
    pub query_amort: u64,
    /// 报告指纹（生成时锁定）。
    pub sig: u64,
}

impl PrecisionReport {
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < 3 {
            h = fnv_step(h, &self.residual_us[i].to_le_bytes());
            i += 1;
        }
        h = fnv_step(h, &self.overhead_ppm.to_le_bytes());
        h = fnv_step(h, &self.write_amort.to_le_bytes());
        h = fnv_step(h, &self.query_amort.to_le_bytes());
        h
    }
}

pub const fn fnv_step(h: u64, data: &[u8]) -> u64 {
    let mut x = h;
    let mut i = 0;
    while i < data.len() {
        x = (x ^ (data[i] as u64)).wrapping_mul(0x1000_0000_01b3);
        i += 1;
    }
    x
}

/// 移交单（判据"向 D02 移交"——测量基础三件套）。
#[derive(Clone, Debug, PartialEq)]
pub struct HandoverDoc {
    /// 件一：口径表指纹（cgd14 口径注册表语义指纹集）。
    pub metric_reg_sig: u64,
    /// 件二：账本查询 API 指纹（cgd12 API 表指纹）。
    pub api_table_sig: u64,
    /// 件三：精度声明指纹（本批报告指纹）。
    pub precision_sig: u64,
    /// 移交目标组。
    pub to_group: &'static str,
    /// 移交指纹（三件套合成——D02 核验锚）。
    pub handover_sig: u64,
}

impl HandoverDoc {
    pub fn composite_sig(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_step(h, &self.metric_reg_sig.to_le_bytes());
        h = fnv_step(h, &self.api_table_sig.to_le_bytes());
        h = fnv_step(h, &self.precision_sig.to_le_bytes());
        h = fnv_step(h, self.to_group.as_bytes());
        h
    }
}

/// D01 组收口管理器。
pub struct D01Closer {
    items: Vec<SelfCheckItem>,
    report: Option<PrecisionReport>,
    /// 双签：施工方/验收方（None=未签）。
    pub signer_build: Option<u64>,
    pub signer_accept: Option<u64>,
    /// 台账条目数。
    ledger_entries: u64,
    handover: Option<HandoverDoc>,
    /// 遥测。
    pub closed: bool,
    pub rejects: u64,
}

impl D01Closer {
    pub fn new() -> Self {
        D01Closer {
            items: Vec::new(),
            report: None,
            signer_build: None,
            signer_accept: None,
            ledger_entries: 0,
            handover: None,
            closed: false,
            rejects: 0,
        }
    }

    /// 自检项登记（判据"16 项自检入树"——满 16 项且全绿才可能收口）。
    pub fn add_item(&mut self, no: usize, owner: &'static str, brief: &'static str, passed: bool) -> Result<(), u32> {
        if no == 0 || no > D01_SELFCHECK_ITEMS {
            self.rejects += 1;
            return Err(XC_SELFCHECK_SHORT);
        }
        // 重复项号拒绝。
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].no == no {
                self.rejects += 1;
                return Err(XC_SELFCHECK_SHORT);
            }
            i += 1;
        }
        self.items.push(SelfCheckItem {
            no,
            owner,
            brief,
            passed,
        });
        Ok(())
    }

    /// 报告固化（生成即锁指纹——判据"实测报告"）。
    pub fn seal_report(&mut self, residual_us: [i64; 3], overhead_ppm: u64, write_amort: u64, query_amort: u64) -> u64 {
        let mut r = PrecisionReport {
            residual_us,
            overhead_ppm,
            write_amort,
            query_amort,
            sig: 0,
        };
        r.sig = r.fingerprint();
        self.report = Some(r.clone());
        r.sig
    }

    /// 报告完整性核验（快照防改——判据红线"事后不可改"）。
    pub fn verify_report(&mut self) -> Result<(), u32> {
        match &self.report {
            Some(r) => {
                if r.fingerprint() != r.sig {
                    self.rejects += 1;
                    Err(XC_REPORT_MUTATED)
                } else {
                    Ok(())
                }
            }
            None => {
                self.rejects += 1;
                Err(XC_EMPTY_HANDOVER)
            }
        }
    }

    /// 双签（判据"组双签"——两方签名齐才有效）。
    pub fn sign(&mut self, role: u8, key: u64) -> Result<(), u32> {
        match role {
            0 => self.signer_build = Some(key),
            1 => self.signer_accept = Some(key),
            _ => {
                self.rejects += 1;
                return Err(XC_SINGLE_SIGN);
            }
        }
        Ok(())
    }

    /// 台账更新（判据"台账更新"——收口前必须落账）。
    pub fn update_ledger(&mut self, entries: u64) -> Result<(), u32> {
        if entries == 0 {
            self.rejects += 1;
            return Err(XC_LEDGER_NOT_UPDATED);
        }
        self.ledger_entries = entries;
        Ok(())
    }

    /// 收口（五判据全过才真收口：16 项全绿+报告锁定+双签+台账+报告核验）。
    pub fn close(&mut self) -> Result<u64, u32> {
        // 1) 16 项齐。
        if self.items.len() != D01_SELFCHECK_ITEMS {
            self.rejects += 1;
            return Err(XC_SELFCHECK_SHORT);
        }
        // 2) 全绿（点名制：每项真实绿）。
        let mut i = 0;
        while i < self.items.len() {
            if !self.items[i].passed {
                self.rejects += 1;
                return Err(XC_SELFCHECK_SHORT);
            }
            i += 1;
        }
        // 3) 报告锁定。
        self.verify_report()?;
        // 4) 双签。
        if self.signer_build.is_none() || self.signer_accept.is_none() {
            self.rejects += 1;
            return Err(XC_SINGLE_SIGN);
        }
        // 5) 台账。
        if self.ledger_entries == 0 {
            self.rejects += 1;
            return Err(XC_LEDGER_NOT_UPDATED);
        }
        self.closed = true;
        // 组收口指纹（16 项+报告+双签合成）。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        i = 0;
        while i < self.items.len() {
            h = fnv_step(h, &(self.items[i].no as u64).to_le_bytes());
            let pass_byte: u64 = if self.items[i].passed { 1 } else { 0 };
            h = fnv_step(h, &pass_byte.to_le_bytes());
            i += 1;
        }
        if let Some(r) = &self.report {
            h = fnv_step(h, &r.sig.to_le_bytes());
        }
        h = fnv_step(h, &self.signer_build.unwrap_or(0).to_le_bytes());
        h = fnv_step(h, &self.signer_accept.unwrap_or(0).to_le_bytes());
        Ok(h)
    }

    /// D02 移交（判据"移交"——收口后才可移交；三件套齐才有效）。
    pub fn handover_to_d02(&mut self, metric_reg_sig: u64, api_table_sig: u64) -> Result<HandoverDoc, u32> {
        if !self.closed {
            self.rejects += 1;
            return Err(XC_EMPTY_HANDOVER);
        }
        if metric_reg_sig == 0 || api_table_sig == 0 {
            self.rejects += 1;
            return Err(XC_EMPTY_HANDOVER);
        }
        let precision_sig = match &self.report {
            Some(r) => r.sig,
            None => return Err(XC_EMPTY_HANDOVER),
        };
        let mut doc = HandoverDoc {
            metric_reg_sig,
            api_table_sig,
            precision_sig,
            to_group: "D02",
            handover_sig: 0,
        };
        doc.handover_sig = doc.composite_sig();
        self.handover = Some(doc.clone());
        Ok(doc)
    }

    /// 移交单核验（D02 侧收货核验——判据"测量基础"完整性）。
    pub fn verify_handover(&self, doc: &HandoverDoc) -> Result<(), u32> {
        if doc.composite_sig() != doc.handover_sig {
            return Err(XC_REPORT_MUTATED);
        }
        if doc.metric_reg_sig == 0 || doc.api_table_sig == 0 || doc.precision_sig == 0 {
            return Err(XC_EMPTY_HANDOVER);
        }
        Ok(())
    }

    /// 自检项计数。
    pub const fn item_count(&self) -> usize {
        self.items.len()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据五组：入树 / 报告 / 双签 / 台账 / 移交）
// ---------------------------------------------------------------------------

impl D01Closer {
    /// 组一：入树——16 项齐全绿才过、缺项拒、红项拒。
    pub fn selfcheck_items(&mut self) -> Result<(), u32> {
        let mut c = D01Closer::new();
        // 只登记 15 项 → 收口拒。
        let mut i = 1;
        while i < D01_SELFCHECK_ITEMS {
            c.add_item(i, "cgd02", "四段计时", true)?;
            i += 1;
        }
        if c.close() != Err(XC_SELFCHECK_SHORT) {
            return Err(XC_SELFCHECK_SHORT);
        }
        // 补第 16 项但红 → 拒。
        c.add_item(16, "cgd14", "口径统一", false)?;
        if c.close() != Err(XC_SELFCHECK_SHORT) {
            return Err(XC_SELFCHECK_SHORT);
        }
        // 重复项号拒。
        if c.add_item(16, "cgd14", "口径统一", true) != Err(XC_SELFCHECK_SHORT) {
            return Err(XC_SELFCHECK_SHORT);
        }
        Ok(())
    }

    /// 组二：报告——固化锁定、篡改即破、未固化拒。
    pub fn selfcheck_report(&mut self) -> Result<(), u32> {
        let mut c = D01Closer::new();
        // 未固化核验拒。
        if c.verify_report() != Err(XC_EMPTY_HANDOVER) {
            return Err(XC_EMPTY_HANDOVER);
        }
        let sig = c.seal_report([12, 30, 8], 30, 10, 9);
        c.verify_report()?;
        // 篡改。
        if let Some(r) = &mut c.report {
            r.overhead_ppm = 9999;
        }
        if c.verify_report() != Err(XC_REPORT_MUTATED) {
            return Err(XC_REPORT_MUTATED);
        }
        // 重新固化同参数 → 同指纹（确定性）。
        let sig2 = c.seal_report([12, 30, 8], 30, 10, 9);
        if sig != sig2 {
            return Err(XC_REPORT_MUTATED);
        }
        Ok(())
    }

    /// 组三：双签——单签拒、双签过、非法角色拒。
    pub fn selfcheck_signs(&mut self) -> Result<(), u32> {
        let mut c = fx_full_green_closer();
        let _ = c.seal_report([10, 20, 30], 30, 10, 9);
        c.sign(0, 0xBEEF)?;
        c.update_ledger(16)?;
        // 单签收口拒。
        if c.close() != Err(XC_SINGLE_SIGN) {
            return Err(XC_SINGLE_SIGN);
        }
        c.sign(1, 0xCAFE)?;
        let g = c.close()?;
        if g == 0 {
            return Err(XC_SINGLE_SIGN);
        }
        // 非法角色。
        if c.sign(7, 1) != Err(XC_SINGLE_SIGN) {
            return Err(XC_SINGLE_SIGN);
        }
        Ok(())
    }

    /// 组四：台账——零条目拒、更新后过。
    pub fn selfcheck_ledger(&mut self) -> Result<(), u32> {
        let mut c = fx_full_green_closer();
        let _ = c.seal_report([10, 20, 30], 30, 10, 9);
        c.sign(0, 1)?;
        c.sign(1, 2)?;
        if c.close() != Err(XC_LEDGER_NOT_UPDATED) {
            return Err(XC_LEDGER_NOT_UPDATED);
        }
        c.update_ledger(16)?;
        c.close()?;
        Ok(())
    }

    /// 组五：移交——未收口拒、缺件拒、收口后移交、D02 核验、篡改即破。
    pub fn selfcheck_handover(&mut self) -> Result<(), u32> {
        let mut c = D01Closer::new();
        // 未收口移交拒。
        if c.handover_to_d02(11, 22) != Err(XC_EMPTY_HANDOVER) {
            return Err(XC_EMPTY_HANDOVER);
        }
        // 走完全套收口。
        let mut i = 1;
        while i <= D01_SELFCHECK_ITEMS {
            c.add_item(i, "cgd02", "项", true)?;
            i += 1;
        }
        let _ = c.seal_report([12, 30, 8], 30, 10, 9);
        c.sign(0, 0xBEEF)?;
        c.sign(1, 0xCAFE)?;
        c.update_ledger(16)?;
        c.close()?;
        // 缺件拒（口径表指纹零）。
        if c.handover_to_d02(0, 22) != Err(XC_EMPTY_HANDOVER) {
            return Err(XC_EMPTY_HANDOVER);
        }
        let doc = c.handover_to_d02(0xA1B2, 0xC3D4)?;
        c.verify_handover(&doc)?;
        // 篡改移交单。
        let mut bad = doc.clone();
        bad.api_table_sig += 1;
        if c.verify_handover(&bad) != Err(XC_REPORT_MUTATED) {
            return Err(XC_REPORT_MUTATED);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_items()?;
        self.selfcheck_report()?;
        self.selfcheck_signs()?;
        self.selfcheck_ledger()?;
        self.selfcheck_handover()?;
        Ok(())
    }
}

/// 全绿 16 项样例工厂。
pub fn fx_full_green_closer() -> D01Closer {
    let mut c = D01Closer::new();
    let owners: [&str; 8] = [
        "cgd02", "cgd03", "cgd04", "cgd05", "cgd06", "cgd09", "cgd10", "cgd14",
    ];
    let mut i = 1;
    while i <= D01_SELFCHECK_ITEMS {
        let owner = owners[(i - 1) % 8];
        let _ = c.add_item(i, owner, "判据项", true);
        i += 1;
    }
    c
}
