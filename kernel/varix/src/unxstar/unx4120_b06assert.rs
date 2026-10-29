//! unx4120_b06assert — UNX-F4120 B06 批判据聚合断言集（诊断段 0x9100）。
//!
//! 判据原文（提炼）：
//!   J1 本批 19 条判据全部聚合入 ktest vfs B06 面，聚合报告 19/19 无缺项。
//!   J2 通过率 100% 才算绿：任一判据缺失或未通过，整批保持非绿。
//!   J3 缺项注入可检出：屏蔽任一判据行后聚合账检出缺项并定位缺号。
//!   J4 聚合账与内嵌判据表一一对应：聚合数==表行数，重复聚合被拒。
//!   J5 批账核对 API 独立可跑，不 import 同批其他模块文件（内嵌表自足）。
//! 设计纪律：
//!   1. 判据表内嵌：19 条 B06 判据（UNX-F4102..F4120 各卡 J1）以
//!      const 表写死于本文件，每行含判据号/诊断段/判据摘要。
//!   2. 聚合状态机：Empty→Aggregated→Green/Red，Green 只能从
//!      Aggregated 且 19/19 全通过抵达；Red 为可恢复态。
//!   3. 缺项检测：以判据号为主键逐行核销，核销不全即 Red 并报缺号。
//!   4. 重复聚合注入拒绝：同一判据号二次核销计入错误账不入通过账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9100 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9100;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MISSING: u16 = SEG_BASE + 1;   // 聚合核销存在缺项
const E_DUP_CLAIM: u16 = SEG_BASE + 2; // 同一判据重复核销（注入）
const E_ID_BAD: u16 = SEG_BASE + 3;    // 判据号不在内嵌表内
const E_STATE_ILLEGAL: u16 = SEG_BASE + 4; // 聚合状态机非法迁移
const E_NOT_GREEN: u16 = SEG_BASE + 5; // 非绿被当绿申报（判据 J2 失守）
const E_ROW_BAD: u16 = SEG_BASE + 6;   // 内嵌表行损坏（段号/摘要为空）

/// 本批 19 条判据的判据号（B06 批 UNX-F4102..F4120 各卡 J1）。
pub const CRIT_IDS: [&str; 19] = [
    "UNX-F4102-J1", "UNX-F4103-J1", "UNX-F4104-J1", "UNX-F4105-J1",
    "UNX-F4106-J1", "UNX-F4107-J1", "UNX-F4108-J1", "UNX-F4109-J1",
    "UNX-F4110-J1", "UNX-F4111-J1", "UNX-F4112-J1", "UNX-F4113-J1",
    "UNX-F4114-J1", "UNX-F4115-J1", "UNX-F4116-J1", "UNX-F4117-J1",
    "UNX-F4118-J1", "UNX-F4119-J1", "UNX-F4120-J1",
];

/// 内嵌判据表行：判据号 + 诊断段 + 摘要（来源施工单判据原文）。
pub struct AggRow {
    pub id: &'static str,   // 判据号，主键
    pub seg: u16,           // 所属诊断段
    pub crit: &'static str, // 判据摘要（非空）
}

/// 内嵌 19 行判据表（B06 批聚合源，独立自足不引外模块）。
pub fn crit_table() -> [AggRow; 19] {
    [
        AggRow { id: CRIT_IDS[0],  seg: 0x8EC0, crit: "查找失败入负缓存，负命中 ENOENT 零穿透，标记位可区分" },
        AggRow { id: CRIT_IDS[1],  seg: 0x8EE0, crit: "10 万次不存在查找：开负缓存穿透=1、关闭=10万，抑制比可算" },
        AggRow { id: CRIT_IDS[2],  seg: 0x8F00, crit: "驱逐只从 cold 链尾取，重建等价账零差" },
        AggRow { id: CRIT_IDS[3],  seg: 0x8F20, crit: "配额耗尽驱逐量=超额量，配额可配置，零配额不崩" },
        AggRow { id: CRIT_IDS[4],  seg: 0x8F40, crit: "unlink/rename 关联条目全失效，陈旧读取零命中" },
        AggRow { id: CRIT_IDS[5],  seg: 0x8F60, crit: "kill_sb 整树出清残留=0（双结构核对），清除先于释放" },
        AggRow { id: CRIT_IDS[6],  seg: 0x8F80, crit: "负缓存创建条件矩阵 6 格全过，错码入账注入被检出" },
        AggRow { id: CRIT_IDS[7],  seg: 0x8FA0, crit: "命中缓存 fs 钩子零调用，未命中恰好一次，逐请求可核对" },
        AggRow { id: CRIT_IDS[8],  seg: 0x8FC0, crit: "双链遍历计数与驻留一致，断链注入被检出并定位" },
        AggRow { id: CRIT_IDS[9],  seg: 0x8FE0, crit: "开/关负缓存对照账齐备，命中率增益可量化" },
        AggRow { id: CRIT_IDS[10], seg: 0x9000, crit: "被引用条目零驱逐（千次注入），10 万次交错零崩零错账" },
        AggRow { id: CRIT_IDS[11], seg: 0x9020, crit: "ramfs 负载抽样 1000 次零差，双模式终态一致" },
        AggRow { id: CRIT_IDS[12], seg: 0x9040, crit: "每 dentry 定长占用有账，池占用=驻留数×定长" },
        AggRow { id: CRIT_IDS[13], seg: 0x9060, crit: "毒化注入被检测器检出，整表重建语义等价" },
        AggRow { id: CRIT_IDS[14], seg: 0x9080, crit: "跌破阈值出一条告警，阈值可配置，正常水位零误报" },
        AggRow { id: CRIT_IDS[15], seg: 0x90A0, crit: "双 sb 同名零串扰（千次断言），单 sb 失效不影响他 sb" },
        AggRow { id: CRIT_IDS[16], seg: 0x90C0, crit: "分级命中率表导出格式固定可 diff，导出与账复算一致" },
        AggRow { id: CRIT_IDS[17], seg: 0x90E0, crit: "B06 断言面注册一次成功，重复注册注入被拒" },
        AggRow { id: CRIT_IDS[18], seg: 0x9100, crit: "本批 19 条判据聚合 19/19 无缺项，通过率 100% 绿" },
    ]
}

/// 聚合状态机：Empty→Aggregated→Green；异常态 Red 可恢复。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggState {
    Empty,      // 尚未聚合
    Aggregated, // 已核销但未全过或未复审
    Green,      // 19/19 全过（终审绿）
    Red,        // 检出缺项/注入，待补齐
}

/// 批级聚合账：核销账 + 错误账 + 缺项定位。
pub struct AggLedger {
    pub claimed: [bool; 19],  // 判据号核销位图
    pub dup_claims: u32,      // 重复核销注入计数
    pub bad_claims: u32,      // 非法判据号注入计数
    pub state: AggState,      // 聚合状态机
    pub audits: u32,          // 批账核对执行轮数
}

impl AggLedger {
    pub fn new() -> AggLedger {
        AggLedger {
            claimed: [false; 19],
            dup_claims: 0,
            bad_claims: 0,
            state: AggState::Empty,
            audits: 0,
        }
    }

    /// 按判据号在表内定位（不在表返回 E_ID_BAD）。
    fn table_idx(&self, id: &str) -> Result<usize, u16> {
        for i in 0..19 {
            if CRIT_IDS[i] == id {
                return Ok(i);
            }
        }
        Err(E_ID_BAD)
    }

    /// 核销一条判据（判据 J3/J4）：重复核销拒绝并计错误账。
    pub fn claim(&mut self, id: &str) -> Result<(), u16> {
        let i = match self.table_idx(id) {
            Ok(i) => i,
            Err(e) => {
                self.bad_claims += 1;
                return Err(e);
            }
        };
        if self.claimed[i] {
            self.dup_claims += 1;
            self.state = AggState::Red;
            return Err(E_DUP_CLAIM);
        }
        self.claimed[i] = true;
        if self.state == AggState::Empty {
            self.state = AggState::Aggregated; // 首核销即入聚合态
        }
        Ok(())
    }

    /// 已核销计数。
    pub fn claimed_count(&self) -> u32 {
        let mut n = 0u32;
        for i in 0..19 {
            if self.claimed[i] {
                n += 1;
            }
        }
        n
    }

    /// 缺项清单：未核销判据的下标集（判据 J3 定位面）。
    pub fn missing(&self) -> Vec<usize> {
        let mut v = Vec::new();
        for i in 0..19 {
            if !self.claimed[i] {
                v.push(i);
            }
        }
        v
    }

    /// 内嵌表完整性自检：行摘要非空、段号非零（判据 J5 前置）。
    fn table_sane(&self) -> bool {
        let t = crit_table();
        for r in &t {
            if r.crit.is_empty() || r.seg == 0 || r.id.is_empty() {
                return false;
            }
        }
        true
    }

    /// 批账核对（判据 J1/J2）：19/19 全核销且表完好才转绿，
    /// 否则转 Red 并返回缺项数语义错误。
    pub fn audit_batch(&mut self) -> Result<u32, u16> {
        self.audits += 1;
        if !self.table_sane() {
            self.state = AggState::Red;
            return Err(E_ROW_BAD);
        }
        let have = self.claimed_count();
        if have == 19 {
            self.state = AggState::Green;
            Ok(19)
        } else {
            self.state = AggState::Red;
            Err(E_MISSING) // 缺 19-have 条，missing() 可定位
        }
    }

    /// 绿灯申报终审（判据 J2）：非绿态申报即拒（防假绿注入）。
    pub fn declare_green(&self) -> Result<(), u16> {
        if self.state != AggState::Green {
            return Err(E_NOT_GREEN);
        }
        Ok(())
    }

    /// 状态机合法迁移复核。
    pub fn state_ok(&self) -> bool {
        match self.state {
            AggState::Green => self.claimed_count() == 19,
            AggState::Red => self.audits > 0,
            AggState::Empty => self.audits == 0,
            AggState::Aggregated => self.audits == 0 && self.claimed_count() > 0,
        }
    }

    /// 批量核销（规范入口）：一次核销一批判据号，
    /// 任一被拒即短路（前缀已入账可审计）。返回成功核销数。
    pub fn claim_batch(&mut self, ids: &[&str]) -> Result<u32, u16> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < ids.len() {
            self.claim(ids[i])?;
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 聚合报告三元组（判据 J1 报告面）：
    /// 返回（已核销数，缺项数，错误注入数）。
    pub fn report(&self) -> (u32, u32, u32) {
        let have = self.claimed_count();
        (have, 19 - have, self.dup_claims + self.bad_claims)
    }

    /// 表下标 → 判据号反查（审计面）：越界拒收。
    pub fn id_of(&self, idx: usize) -> Result<&'static str, u16> {
        if idx >= 19 {
            return Err(E_ID_BAD);
        }
        Ok(CRIT_IDS[idx])
    }
}

/// 自例程：R1 全量聚合→R2 缺项注入检出→R3 注入拦截→R4 复绿对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 正常路径：19 条全部核销（含 Aggregated 中间态），audit 绿。
    let mut led = AggLedger::new();
    let t = crit_table();
    for r in &t {
        led.claim(r.id)?;
    }
    if led.state != AggState::Aggregated {
        return Err(E_STATE_ILLEGAL); // 核销完未复审应为聚合态
    }
    let n = led.audit_batch()?;
    if n != 19 {
        return Err(E_MISSING);
    }
    led.declare_green()?;
    hits += 1;

    // R2 缺项注入：清空一条核销位 → 复审转 Red 并定位缺号（判据 J3）。
    led.claimed[11] = false;
    if led.audit_batch().is_ok() {
        return Err(E_NOT_GREEN);
    }
    let miss = led.missing();
    if miss.len() != 1 || miss[0] != 11 {
        return Err(E_MISSING); // 必须恰好定位到第 11 号
    }
    if led.declare_green().is_ok() {
        return Err(E_NOT_GREEN); // Red 态假绿申报必须被拒
    }
    hits += 1;

    // R3 注入拦截：非法判据号、重复核销全拒并计错误账（判据 J4）。
    let mut led2 = AggLedger::new();
    if led2.claim("UNX-F4999-J1").is_ok() {
        return Err(E_ID_BAD);
    }
    led2.claim(CRIT_IDS[0])?;
    if led2.claim(CRIT_IDS[0]).is_ok() {
        return Err(E_DUP_CLAIM);
    }
    if led2.bad_claims != 1 || led2.dup_claims != 1 {
        return Err(E_STATE_ILLEGAL);
    }
    if led2.state != AggState::Red {
        return Err(E_STATE_ILLEGAL); // 重复核销置 Red
    }
    hits += 1;

    // R4 守恒对账：补齐后复绿；核销数==19；状态机不变式成立（判据 J1/J2）。
    led.claimed[11] = true;
    let n2 = led.audit_batch()?;
    if n2 != 19 || led.state != AggState::Green {
        return Err(E_NOT_GREEN);
    }
    led.declare_green()?;
    if led.claimed_count() != 19 || !led.state_ok() {
        return Err(E_STATE_ILLEGAL);
    }
    // 幂等复审：Green 再审仍绿，核销位不动。
    let n3 = led.audit_batch()?;
    if n3 != 19 || led.state != AggState::Green {
        return Err(E_NOT_GREEN);
    }
    // 内嵌表终审：19 行、id 唯一、与核销位图一一对应。
    for i in 0..19 {
        if !led.claimed[i] {
            return Err(E_MISSING);
        }
    }
    let mut j = 0usize;
    while j < 19 {
        let mut k = j + 1;
        while k < 19 {
            if CRIT_IDS[j] == CRIT_IDS[k] {
                return Err(E_ROW_BAD); // id 唯一性
            }
            k += 1;
        }
        j += 1;
    }
    hits += 1;

    // R5 报告面：report 三元组、批量核销、反查审计。
    let (have, miss_n, err_n) = led.report();
    if have != 19 || miss_n != 0 || err_n != 0 {
        return Err(E_MISSING); // 19/19 无缺项无注入
    }
    let mut led3 = AggLedger::new();
    let part = [
        CRIT_IDS[0], CRIT_IDS[1], CRIT_IDS[2],
    ];
    let n5 = led3.claim_batch(&part)?;
    if n5 != 3 {
        return Err(E_MISSING); // 前缀批量核销三进三出
    }
    if led3.claim_batch(&part).is_ok() {
        return Err(E_DUP_CLAIM); // 整批重放首条即撞重复
    }
    if led3.id_of(18)? != CRIT_IDS[18] {
        return Err(E_ROW_BAD); // 反查一致
    }
    if led3.id_of(19).is_ok() {
        return Err(E_ID_BAD); // 越界反查应拒
    }
    if led3.report().0 != 3 {
        return Err(E_MISSING);
    }
    hits += 1;

    Ok(hits)
}
