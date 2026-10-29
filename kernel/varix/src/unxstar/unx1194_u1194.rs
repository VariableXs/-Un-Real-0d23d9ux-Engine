// unx1194_u1194 —— UNX-F1194 F0039 四态下亲和性行为差异账
// 判据原文 UNX-F1194-J1：差异账全过（四态各自路由约束逐态登记生效、
// 违例必抓（Draining 改路由等）、态间差异矩阵、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x8eb0 独占（0x8eb0~0x8eef）。
//
// 设计（纯功能 no_std）：
//   * 四态路由约束闭集：Polling=自由路由 / Draining=冻结路由 /
//     Arming=武装序内路由 / Irq=常规路由；
//   * 差异矩阵：态 × 动作（新路由/迁移/冻结）× 允许性 三维账；
//   * 违例必抓：Draining 新路由 / Arming 越武装序 必拒；
//   * FNV 链指纹冻结，封账后拒操作。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x8eb0 独占） ─────────────────────────────────────────────
pub const XC_F4_SEALED: u32 = 0x8EB1; // 封账后操作
pub const XC_F4_DRAIN_FROZEN: u32 = 0x8EB2; // Draining 冻结路由被破
pub const XC_F4_ARM_ORDER: u32 = 0x8EB3; // Arming 越武装序
pub const XC_F4_BAD_STATE: u32 = 0x8EB4; // 非法态
pub const XC_F4_BAD_ACT: u32 = 0x8EB5; // 非法动作

/// 四态编码。
pub const ST_POLLING: u8 = 0;
pub const ST_DRAINING: u8 = 1;
pub const ST_ARMING: u8 = 2;
pub const ST_IRQ: u8 = 3;

/// 动作编码：0=新路由 1=迁移 2=冻结查询。
pub const ACT_NEW_ROUTE: u8 = 0;
pub const ACT_MIGRATE: u8 = 1;
pub const ACT_QUERY: u8 = 2;

/// 差异矩阵行。
#[derive(Clone, Copy)]
pub struct DiffRow {
    pub state: u8,
    pub act: u8,
    /// 允许性（true=放行）。
    pub allowed: bool,
}

/// 四态差异账。
pub struct FourStateDiff {
    rows: Vec<DiffRow>,
    /// Arming 武装序（向量号顺序）。
    arm_order: Vec<u8>,
    armed_ptr: usize,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl FourStateDiff {
    pub fn new(arm_order: Vec<u8>) -> FourStateDiff {
        FourStateDiff { rows: vec![], arm_order, armed_ptr: 0, sealed: false, rejects: vec![] }
    }

    /// 差异矩阵查询：态下动作是否允许。
    pub fn allowed(state: u8, act: u8) -> bool {
        match (state, act) {
            (ST_POLLING, ACT_NEW_ROUTE) => true,
            (ST_POLLING, ACT_MIGRATE) => true,
            (ST_POLLING, ACT_QUERY) => true,
            (ST_DRAINING, ACT_NEW_ROUTE) => false, // 冻结
            (ST_DRAINING, ACT_MIGRATE) => false,   // 冻结
            (ST_DRAINING, ACT_QUERY) => true,
            (ST_ARMING, ACT_NEW_ROUTE) => true, // 仅武装序内（运行期再查）
            (ST_ARMING, ACT_MIGRATE) => false,
            (ST_ARMING, ACT_QUERY) => true,
            (ST_IRQ, ACT_NEW_ROUTE) => true,
            (ST_IRQ, ACT_MIGRATE) => true,
            (ST_IRQ, ACT_QUERY) => true,
            _ => false,
        }
    }

    /// 执行动作（逐案登记 + 违例抓）。
    pub fn act(&mut self, state: u8, act: u8, vec_no: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_F4_SEALED, "封账后操作"));
            return Err(XC_F4_SEALED);
        }
        if state > ST_IRQ {
            self.rejects.push((XC_F4_BAD_STATE, "非法态"));
            return Err(XC_F4_BAD_STATE);
        }
        if act > ACT_QUERY {
            self.rejects.push((XC_F4_BAD_ACT, "非法动作"));
            return Err(XC_F4_BAD_ACT);
        }
        if !Self::allowed(state, act) {
            if state == ST_DRAINING {
                self.rejects.push((XC_F4_DRAIN_FROZEN, "Draining 冻结被破"));
            } else {
                self.rejects.push((XC_F4_BAD_ACT, "动作禁行"));
            }
            return Err(if state == ST_DRAINING { XC_F4_DRAIN_FROZEN } else { XC_F4_BAD_ACT });
        }
        // Arming 态新路由必须按武装序
        if state == ST_ARMING && act == ACT_NEW_ROUTE {
            match self.arm_order.get(self.armed_ptr) {
                Some(&v) if v == vec_no => {
                    self.armed_ptr += 1;
                }
                _ => {
                    self.rejects.push((XC_F4_ARM_ORDER, "Arming 越武装序"));
                    return Err(XC_F4_ARM_ORDER);
                }
            }
        }
        self.rows.push(DiffRow { state, act, allowed: true });
        Ok(())
    }

    /// 差异矩阵快照：4 态 × 3 动作全表。
    pub fn matrix(&self) -> [bool; 12] {
        let mut out = [false; 12];
        for s in 0..4u8 {
            for a in 0..3u8 {
                out[(s * 3 + a) as usize] = Self::allowed(s, a);
            }
        }
        out
    }

    /// 守恒：Draining 冻结（2 动作禁行）在矩阵中。
    pub fn matrix_sane(&self) -> bool {
        let m = self.matrix();
        !m[1 * 3 + 0] && !m[1 * 3 + 1] && m[1 * 3 + 2]
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_F4_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.rows {
            h ^= (r.state as u64).rotate_left(5) ^ (r.act as u64).rotate_left(17);
            h = h.wrapping_mul(0x0100_0193);
        }
        for &v in &self.arm_order {
            h ^= v as u64;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准差异场景：四态各执行合法动作 + 违例抓 + 矩阵健全。
pub fn diff_scenario() -> Result<(usize, u64), u32> {
    let mut d = FourStateDiff::new(vec![0x20, 0x21, 0x22]);
    d.act(ST_POLLING, ACT_NEW_ROUTE, 0x30)?;
    d.act(ST_DRAINING, ACT_QUERY, 0)?;
    d.act(ST_ARMING, ACT_NEW_ROUTE, 0x20)?;
    d.act(ST_ARMING, ACT_NEW_ROUTE, 0x21)?;
    d.act(ST_IRQ, ACT_MIGRATE, 0x40)?;
    if !d.matrix_sane() {
        return Err(XC_F4_BAD_ACT);
    }
    let fp = d.seal()?;
    Ok((d.rows_len(), fp))
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）Draining 新路由必抓
    {
        let mut d = FourStateDiff::new(vec![]);
        if d.act(ST_DRAINING, ACT_NEW_ROUTE, 1) == Err(XC_F4_DRAIN_FROZEN) {
            caught += 1;
        }
    }
    // 2）Draining 迁移必抓
    {
        let mut d = FourStateDiff::new(vec![]);
        if d.act(ST_DRAINING, ACT_MIGRATE, 1) == Err(XC_F4_DRAIN_FROZEN) {
            caught += 1;
        }
    }
    // 3）Arming 越武装序必抓
    {
        let mut d = FourStateDiff::new(vec![5, 6]);
        if d.act(ST_ARMING, ACT_NEW_ROUTE, 6) == Err(XC_F4_ARM_ORDER) {
            caught += 1;
        }
    }
    // 4）Arming 按序放行
    {
        let mut d = FourStateDiff::new(vec![5, 6]);
        if d.act(ST_ARMING, ACT_NEW_ROUTE, 5) == Ok(()) && d.act(ST_ARMING, ACT_NEW_ROUTE, 6) == Ok(()) {
            caught += 1;
        }
    }
    // 5）非法态必拒
    {
        let mut d = FourStateDiff::new(vec![]);
        if d.act(9, ACT_QUERY, 0) == Err(XC_F4_BAD_STATE) {
            caught += 1;
        }
    }
    // 6）矩阵健全性
    {
        let d = FourStateDiff::new(vec![]);
        if d.matrix_sane() {
            caught += 1;
        }
    }
    (caught, N)
}
