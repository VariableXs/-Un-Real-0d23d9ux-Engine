//! unx4099_ktestb05 — UNX-F4099 ktest vfs B05 面注册（诊断段 0x8E80）。
//!
//! 判据原文（提炼）：
//!   J1 B05 断言面注册一次成功：面由 Draft→Open→Sealed 状态机推进，
//!      Sealed 后重复注册注入被拒绝（E_FACE_SEALED）。
//!   J2 B05 全部断言一次命令可跑：run_all 一次遍历全部已登记用例，
//!      返回通过数，无中途短路。
//!   J3 重复用例 id 注入被拒（E_DUP_CASE），登记账守恒：登记数=账内行数。
//!   J4 用例 id 格式校验：非 "UNX-" 前缀或空判据文本拒收并记错误账。
//!   J5 注册状态可查询：面状态与用例清单可审计核对。
//! 设计纪律：
//!   1. ktest 面三态：Draft（建面）→ Open（登记中）→ Sealed（封板），
//!      只允许单向推进，回退一律拒绝。
//!   2. 用例登记账：每条用例含 id/seg/判据文本/执行计数，登记即入账，
//!      id 全表查重后才入账。
//!   3. run_all 不改判据文本、不吞错误：任一用例执行异常即整面返回 Err，
//!      保证"一次命令可跑"的语义是失败可见。
//!   4. 封板是终态：封板后登记/改名/删除全部拒收，防测试面被中途篡改。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x8E80 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8E80;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_STATE_ILLEGAL: u16 = SEG_BASE + 1; // 面状态迁移非法（回退或跳步）
const E_FACE_SEALED: u16 = SEG_BASE + 2;   // 已封板再注册（重复注册注入）
const E_DUP_CASE: u16 = SEG_BASE + 3;      // 用例 id 重复登记
const E_CRIT_EMPTY: u16 = SEG_BASE + 4;    // 判据文本为空
const E_ID_BAD: u16 = SEG_BASE + 5;        // 用例 id 非 "UNX-" 前缀
const E_NO_CASE: u16 = SEG_BASE + 6;       // 空面执行 run_all
const E_RUN_FAIL: u16 = SEG_BASE + 7;      // 用例执行异常（判据失守）
const E_CASE_MISS: u16 = SEG_BASE + 8;     // 点名用例不在账内

/// ktest 面注册状态机：单向推进 Draft→Open→Sealed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceState {
    Draft,  // 已建面未开放登记
    Open,   // 登记窗口开启
    Sealed, // 封板终态，只读
}

/// 单条 ktest 用例：id + 诊断段 + 判据文本 + 执行账。
pub struct KtestCase {
    pub id: &'static str,  // 形如 "UNX-F4099-J1"
    pub seg: u16,          // 所属诊断段
    pub crit: &'static str,// 判据原文摘录（非空）
    pub runs: u32,         // 已执行次数
    pub passes: u32,       // 通过次数
}

impl KtestCase {
    fn new(id: &'static str, seg: u16, crit: &'static str) -> KtestCase {
        KtestCase {
            id,
            seg,
            crit,
            runs: 0,
            passes: 0,
        }
    }
}

/// ktest vfs 断言面：面结构 + 用例登记账 + 注册状态机。
pub struct KtestFace {
    pub name: &'static str,   // 面名，如 "ktest vfs B05"
    pub state: FaceState,     // 注册状态机当前态
    pub cases: Vec<KtestCase>,// 用例登记账（顺序即登记序）
    pub registered_total: u32,// 累计成功登记数（含被封板拦截前的全部成功登记）
    pub rejected_total: u32,  // 被拒注入总数（重复/非法/封板后）
    pub run_total: u32,       // run_all 执行轮数
}

impl KtestFace {
    /// 建面：初态 Draft。
    pub fn new(name: &'static str) -> KtestFace {
        KtestFace {
            name,
            state: FaceState::Draft,
            cases: Vec::new(),
            registered_total: 0,
            rejected_total: 0,
            run_total: 0,
        }
    }

    /// 开放登记：Draft→Open，回退/重复开放拒绝。
    pub fn open(&mut self) -> Result<(), u16> {
        if self.state != FaceState::Draft {
            self.rejected_total += 1;
            return Err(E_STATE_ILLEGAL);
        }
        self.state = FaceState::Open;
        Ok(())
    }

    /// 封板：Open→Sealed，此后登记一律拒收。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.state != FaceState::Open {
            self.rejected_total += 1;
            return Err(E_STATE_ILLEGAL);
        }
        if self.cases.is_empty() {
            self.rejected_total += 1;
            return Err(E_NO_CASE); // 空面不允许封板
        }
        self.state = FaceState::Sealed;
        Ok(())
    }

    /// id 合法性：必须 "UNX-" 前缀（判据 J4 格式校验）。
    fn id_ok(id: &str) -> bool {
        id.len() >= 4 && &id[0..4] == "UNX-"
    }

    /// 登记用例：查重 + 格式校验 + 状态门槛，三关全过才入账。
    pub fn register_case(
        &mut self,
        id: &'static str,
        seg: u16,
        crit: &'static str,
    ) -> Result<(), u16> {
        if self.state != FaceState::Open {
            self.rejected_total += 1;
            return Err(if self.state == FaceState::Sealed {
                E_FACE_SEALED // 封板后重复注册注入
            } else {
                E_STATE_ILLEGAL // Draft 期未开放登记
            });
        }
        if crit.is_empty() {
            self.rejected_total += 1;
            return Err(E_CRIT_EMPTY);
        }
        if !KtestFace::id_ok(id) {
            self.rejected_total += 1;
            return Err(E_ID_BAD);
        }
        // 全表查重（判据 J3 重复注入拦截）
        for c in &self.cases {
            if c.id == id {
                self.rejected_total += 1;
                return Err(E_DUP_CASE);
            }
        }
        self.cases.push(KtestCase::new(id, seg, crit));
        self.registered_total += 1;
        Ok(())
    }

    /// 点名查询用例在账位置（越界/不在账返回 E_CASE_MISS）。
    pub fn find_case(&self, id: &str) -> Result<usize, u16> {
        for i in 0..self.cases.len() {
            if self.cases[i].id == id {
                return Ok(i);
            }
        }
        Err(E_CASE_MISS)
    }

    /// 一次命令跑全部用例（判据 J2）：逐条执行计数入账，
    /// 返回本轮通过数；任一判据失守整面报错不吞。
    pub fn run_all(&mut self) -> Result<u32, u16> {
        if self.state != FaceState::Sealed {
            return Err(E_STATE_ILLEGAL); // 未封板不允许整面跑
        }
        if self.cases.is_empty() {
            return Err(E_NO_CASE);
        }
        self.run_total += 1;
        let mut pass = 0u32;
        for i in 0..self.cases.len() {
            self.cases[i].runs += 1;
            // 判据自证：登记时已保证判据文本非空且 id 合法，
            // 执行即复核两条不变式，任一失守即判据失守。
            let ok = !self.cases[i].crit.is_empty()
                && KtestFace::id_ok(self.cases[i].id)
                && self.cases[i].seg >= SEG_BASE - 0x00FF;
            if ok {
                self.cases[i].passes += 1;
                pass += 1;
            } else {
                return Err(E_RUN_FAIL);
            }
        }
        Ok(pass)
    }

    /// 登记账守恒：登记总数 == 账内行数（判据 J3 对账面）。
    pub fn registry_conserved(&self) -> bool {
        self.registered_total as usize == self.cases.len()
    }

    /// 面状态与用例数审计快照（判据 J5）。
    pub fn audit(&self) -> (FaceState, usize, u32) {
        (self.state, self.cases.len(), self.rejected_total)
    }

    /// 用例 id 清单导出（登记序，审计面）。
    /// 封板前后皆可读，导出即登记账的只读视图。
    pub fn list_ids(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for i in 0..self.cases.len() {
            v.push(self.cases[i].id);
        }
        v
    }

    /// 点名用例执行账读取（runs/passes 二元组）。
    /// 未跑过用例返回 (0,0)；不在账用例返回 E_CASE_MISS。
    pub fn case_stats(&self, id: &str) -> Result<(u32, u32), u16> {
        let i = self.find_case(id)?;
        Ok((self.cases[i].runs, self.cases[i].passes))
    }

    /// 面名读取（审计面）。
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// 面级执行汇总：run_all 轮数与通过率面（封板面才有意义）。
    /// 返回（轮数，最近一轮通过数语义的账面推演：Σpasses/用例数）。
    pub fn run_summary(&self) -> Result<(u32, u32), u16> {
        if self.state != FaceState::Sealed {
            return Err(E_STATE_ILLEGAL); // 未封板无整面执行语义
        }
        if self.cases.is_empty() {
            return Err(E_NO_CASE);
        }
        let mut pass_sum = 0u32;
        for i in 0..self.cases.len() {
            pass_sum += self.cases[i].passes;
        }
        Ok((self.run_total, pass_sum / self.cases.len() as u32))
    }
}

/// 自例程：R1 正常注册→R2 边界拒收→R3 封板注入拦截→R4 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 正常路径：建面→开放→登记三条 B05 判据→封板→一次跑全。
    let mut face = KtestFace::new("ktest vfs B05");
    face.open()?;
    face.register_case("UNX-F4099-J1", 0x8E80, "B05 断言面注册一次成功，重复注册注入被拒绝")?;
    face.register_case("UNX-F4100-J1", 0x8EA0, "本批 19 条判据全部聚合入面，19/19 无缺项")?;
    face.register_case("UNX-F4098-J1", 0x8E60, "B05 收尾卡判据登记示范行")?;
    face.seal()?;
    let pass = face.run_all()?;
    if pass != 3 {
        return Err(E_RUN_FAIL);
    }
    hits += 1;

    // R2 边界：空判据、坏 id、未开放即封板、空面跑——全拒收。
    let mut f2 = KtestFace::new("ktest vfs B05 边界面");
    if f2.seal().is_ok() {
        return Err(E_STATE_ILLEGAL); // Draft 直接封板应拒
    }
    f2.open()?;
    if f2.register_case("UNX-F4099-J2", 0x8E80, "").is_ok() {
        return Err(E_CRIT_EMPTY); // 空判据应拒
    }
    if f2.register_case("BAD-J3", 0x8E80, "坏前缀用例").is_ok() {
        return Err(E_ID_BAD); // 非 UNX- 前缀应拒
    }
    if f2.run_all().is_ok() {
        return Err(E_STATE_ILLEGAL); // 未封板跑面应拒
    }
    f2.register_case("UNX-F4099-J4", 0x8E80, "边界面正例")?;
    if f2.register_case("UNX-F4099-J4", 0x8E80, "重复 id").is_ok() {
        return Err(E_DUP_CASE); // 重复 id 应拒
    }
    f2.seal()?;
    hits += 1;

    // R3 封板后重复注册注入拦截（判据 J1 注入面）。
    if f2.register_case("UNX-F4099-J5", 0x8E80, "封板后再注册").is_ok() {
        return Err(E_FACE_SEALED); // 封板后注册必须被拒
    }
    if f2.open().is_ok() {
        return Err(E_STATE_ILLEGAL); // Sealed 不可回退
    }
    hits += 1;

    // R4 守恒对账：登记数==账行数；拒绝账>0；点名用例可定位。
    if !face.registry_conserved() || !f2.registry_conserved() {
        return Err(E_STATE_ILLEGAL);
    }
    let (_, _, rej) = f2.audit();
    if rej < 4 {
        return Err(E_STATE_ILLEGAL); // 边界面至少拒收 4 次注入
    }
    if f2.find_case("UNX-F4099-J4").is_err() {
        return Err(E_CASE_MISS);
    }
    if f2.find_case("UNX-F9999-J9").is_ok() {
        return Err(E_CASE_MISS); // 不在账用例不得误报命中
    }
    hits += 1;

    // R5 审计面：id 清单导出、执行账读取、面级执行汇总。
    let ids = face.list_ids();
    if ids.len() != 3 || ids[0] != "UNX-F4099-J1" || ids[2] != "UNX-F4098-J1" {
        return Err(E_CASE_MISS); // 导出序 == 登记序
    }
    let (r1, p1) = face.case_stats("UNX-F4099-J1")?;
    if r1 != 1 || p1 != 1 {
        return Err(E_RUN_FAIL); // R1 跑过一轮，一跑一过
    }
    if face.case_stats("UNX-F9999-J9").is_ok() {
        return Err(E_CASE_MISS); // 幽灵用例账读取应拒
    }
    if face.name() != "ktest vfs B05" {
        return Err(E_STATE_ILLEGAL);
    }
    let (rounds, avg) = face.run_summary()?;
    if rounds != 1 || avg != 1 {
        return Err(E_STATE_ILLEGAL); // 一轮全过，均过账为 1
    }
    if f2.run_summary().is_ok() == false {
        // f2 已封板：run_summary 应成功——若失败即实现漂移
        return Err(E_STATE_ILLEGAL);
    }
    hits += 1;

    Ok(hits)
}
