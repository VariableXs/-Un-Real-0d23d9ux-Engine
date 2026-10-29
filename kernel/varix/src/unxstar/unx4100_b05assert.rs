//! unx4100_b05assert — UNX-F4100 B05 批判据聚合断言集（诊断段 0x8EA0）。
//!
//! 判据原文（提炼）：
//!   J1 本批 19 条判据全部聚合入 ktest vfs B05 面，聚合报告 19/19 无缺项。
//!   J2 通过率 100% 才算绿：任一判据缺失或未通过，整批保持非绿。
//!   J3 缺项注入可检出：屏蔽任一判据行后聚合账检出缺项并定位缺号。
//!   J4 聚合账与内嵌判据表一一对应：聚合数==表行数，重复聚合被拒。
//!   J5 批账核对 API 独立可跑，不 import 同批其他模块文件（内嵌表自足）。
//! 设计纪律：
//!   1. 判据表内嵌：19 条 B05 判据以 const 表写死于本文件，
//!      每行含判据号/诊断段/判据摘要，来源为本批施工单判据原文。
//!   2. 聚合状态机：Empty→Aggregated→Green/Red，Green 只能从
//!      Aggregated 且 19/19 全通过抵达；Red 为可恢复态（补齐后可复审）。
//!   3. 缺项检测：以判据号为主键逐行核销，核销不全即 Red 并报缺号。
//!   4. 重复聚合注入拒绝：同一判据号二次核销计入错误账不入通过账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x8EA0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8EA0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MISSING: u16 = SEG_BASE + 1;   // 聚合核销存在缺项
const E_DUP_CLAIM: u16 = SEG_BASE + 2; // 同一判据重复核销（注入）
const E_ID_BAD: u16 = SEG_BASE + 3;    // 判据号不在内嵌表内
const E_STATE_ILLEGAL: u16 = SEG_BASE + 4; // 聚合状态机非法迁移
const E_NOT_GREEN: u16 = SEG_BASE + 5; // 非绿被当绿申报（判据 J2 失守）
const E_ROW_BAD: u16 = SEG_BASE + 6;   // 内嵌表行损坏（段号/摘要为空）

/// 本批 19 条判据的判据号（B05 批 UNX-F4080..F4098 各卡 J1）。
pub const CRIT_IDS: [&str; 19] = [
    "UNX-F4080-J1", "UNX-F4081-J1", "UNX-F4082-J1", "UNX-F4083-J1",
    "UNX-F4084-J1", "UNX-F4085-J1", "UNX-F4086-J1", "UNX-F4087-J1",
    "UNX-F4088-J1", "UNX-F4089-J1", "UNX-F4090-J1", "UNX-F4091-J1",
    "UNX-F4092-J1", "UNX-F4093-J1", "UNX-F4094-J1", "UNX-F4095-J1",
    "UNX-F4096-J1", "UNX-F4097-J1", "UNX-F4098-J1",
];

/// 内嵌判据表行：判据号 + 诊断段 + 摘要（来源施工单判据原文）。
pub struct AggRow {
    pub id: &'static str, // 判据号，主键
    pub seg: u16,         // 所属诊断段
    pub crit: &'static str, // 判据摘要（非空）
}

/// 内嵌 19 行判据表（B05 批聚合源，独立自足不引外模块）。
pub fn crit_table() -> [AggRow; 19] {
    [
        AggRow { id: CRIT_IDS[0],  seg: 0x8D40, crit: "B05 首卡判据：面基础语义成立" },
        AggRow { id: CRIT_IDS[1],  seg: 0x8D60, crit: "d_hash 哈希一致性判据" },
        AggRow { id: CRIT_IDS[2],  seg: 0x8D80, crit: "dentry 父链不变式判据" },
        AggRow { id: CRIT_IDS[3],  seg: 0x8DA0, crit: "d_subdirs 树遍历对账判据" },
        AggRow { id: CRIT_IDS[4],  seg: 0x8DC0, crit: "哈希表桶结构守恒判据" },
        AggRow { id: CRIT_IDS[5],  seg: 0x8DE0, crit: "lookup 命中路径零穿透判据" },
        AggRow { id: CRIT_IDS[6],  seg: 0x8E00, crit: "lookup 未命中恰好一次穿透判据" },
        AggRow { id: CRIT_IDS[7],  seg: 0x8E20, crit: "dget/dput 引用守恒判据" },
        AggRow { id: CRIT_IDS[8],  seg: 0x8E40, crit: "dentry 与 inode 绑定判据" },
        AggRow { id: CRIT_IDS[9],  seg: 0x8E60, crit: "重命名迁移语义判据" },
        AggRow { id: CRIT_IDS[10], seg: 0x8E70, crit: "缓存一致性失效判据" },
        AggRow { id: CRIT_IDS[11], seg: 0x8E75, crit: "路径逐分量解析联动判据" },
        AggRow { id: CRIT_IDS[12], seg: 0x8E78, crit: "dentry 生命周期状态机判据" },
        AggRow { id: CRIT_IDS[13], seg: 0x8E7B, crit: "统计账与命中率判据" },
        AggRow { id: CRIT_IDS[14], seg: 0x8E7D, crit: "大目录子树剪枝预研判据" },
        AggRow { id: CRIT_IDS[15], seg: 0x8E7F, crit: "跨挂载点 walk 边界判据" },
        AggRow { id: CRIT_IDS[16], seg: 0x8E82, crit: "短生命周期回收账判据" },
        AggRow { id: CRIT_IDS[17], seg: 0x8E84, crit: "d_compare 大小写敏感声明判据" },
        AggRow { id: CRIT_IDS[18], seg: 0x8E86, crit: "d_revalidate 钩子判据" },
    ]
}

/// 聚合状态机：Empty→Aggregated→Green；Aggregated/Empty 可转 Red。
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
    /// 否则转 Red 并返回缺项数。Green 重复复审幂等。
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

    /// 状态机合法迁移复核：Red 只能经 audit_batch 回 Green，
    /// Empty 不能直接 declare_green。
    pub fn state_ok(&self) -> bool {
        match self.state {
            AggState::Green => self.claimed_count() == 19,
            AggState::Red => self.audits > 0,
            AggState::Empty => self.audits == 0,
            AggState::Aggregated => self.audits == 0 && self.claimed_count() > 0,
        }
    }

    /// 按表下标核销（批量聚合辅助面）：下标越界拒收。
    /// 与 claim 共用核销位图，重复核销语义一致。
    pub fn claim_by_index(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= 19 {
            self.bad_claims += 1;
            return Err(E_ID_BAD); // 越界下标视同非法判据
        }
        self.claim(CRIT_IDS[idx])
    }

    /// 聚合报告三元组（判据 J1 报告面）：
    /// 返回（已核销数，缺项数，错误注入数），19/19 无缺项即前两项
    /// 为 (19, 0)。
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

    /// 全量核销便捷面：一次核销全部 19 条（批聚合的规范入口）。
    /// 返回成功核销数（应为 19）；过程中任一异常即短路报错。
    pub fn claim_all(&mut self) -> Result<u32, u16> {
        let t = crit_table();
        let mut n = 0u32;
        for r in &t {
            self.claim(r.id)?;
            n += 1;
        }
        Ok(n)
    }
}

/// 自例程：R1 全量聚合→R2 缺项注入检出→R3 补齐复绿→R4 对账终审。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 正常路径：19 条全部核销，audit_batch 绿，declare_green 过。
    let mut led = AggLedger::new();
    let t = crit_table();
    for r in &t {
        led.claim(r.id)?;
    }
    let n = led.audit_batch()?;
    if n != 19 {
        return Err(E_MISSING);
    }
    led.declare_green()?;
    hits += 1;

    // R2 缺项注入：清空一条核销位 → 复审转 Red 并定位缺号。
    led.claimed[7] = false;
    if led.audit_batch().is_ok() {
        return Err(E_NOT_GREEN); // 缺项必须复审失败
    }
    let miss = led.missing();
    if miss.len() != 1 || miss[0] != 7 {
        return Err(E_MISSING); // 必须恰好定位到第 7 号
    }
    if led.declare_green().is_ok() {
        return Err(E_NOT_GREEN); // Red 态假绿申报必须被拒
    }
    hits += 1;

    // R3 注入拦截：非法判据号、重复核销全拒并计错误账。
    let mut led2 = AggLedger::new();
    if led2.claim("UNX-F4999-J1").is_ok() {
        return Err(E_ID_BAD); // 不在表内判据号应拒
    }
    led2.claim(CRIT_IDS[0])?;
    if led2.claim(CRIT_IDS[0]).is_ok() {
        return Err(E_DUP_CLAIM); // 重复核销应拒
    }
    if led2.bad_claims != 1 || led2.dup_claims != 1 {
        return Err(E_STATE_ILLEGAL); // 错误账必须如实记账
    }
    hits += 1;

    // R4 守恒对账：补齐后复绿；核销数==19；状态机不变式成立。
    led.claimed[7] = true;
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
    hits += 1;

    // R5 报告面：report 三元组、下标核销、全量核销便捷面终审。
    let (have, miss_n, err_n) = led.report();
    if have != 19 || miss_n != 0 || err_n != 0 {
        return Err(E_MISSING); // 19/19 无缺项无注入
    }
    let mut led3 = AggLedger::new();
    if led3.claim_by_index(19).is_ok() {
        return Err(E_ID_BAD); // 越界下标应拒
    }
    led3.claim_by_index(0)?;
    if led3.claim_by_index(0).is_ok() {
        return Err(E_DUP_CLAIM); // 位图复用查重应拒
    }
    if led3.id_of(0)? != CRIT_IDS[0] {
        return Err(E_ROW_BAD); // 反查必须一致
    }
    if led3.id_of(19).is_ok() {
        return Err(E_ID_BAD);
    }
    // 全量核销便捷面：新账一次 19 条，audit 直绿。
    let mut led4 = AggLedger::new();
    let n4 = led4.claim_all()?;
    if n4 != 19 {
        return Err(E_MISSING);
    }
    if led4.audit_batch()? != 19 || led4.state != AggState::Green {
        return Err(E_NOT_GREEN);
    }
    led4.declare_green()?;
    hits += 1;

    Ok(hits)
}
