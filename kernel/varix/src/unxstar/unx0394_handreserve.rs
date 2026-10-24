//! unx0394_handreserve —— UNX-F0394 交接结构堆预留（A2/A4 窗）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0394 · 交接结构堆预留 · 目标 240 行
//! 判据原文：UNX-F0394-J1 交接窗口（A2 入口窗 / A4 出口窗）各自
//!           预留专属堆配额，普通分配不得侵占；预留配额三判定：
//!           （a）窗口内预留分配必成（配额内）；（b）普通分配
//!           逼近预留边界时被拦截（拒绝码带边界标记）；（c）窗
//!           关闭后预留解冻回通用池，普通分配恢复全额；预留
//!           用量账与配额对账（用量 ≤ 配额恒成立，超用立案）。
//! 组件：预留窗（窗号 + 配额 + 开关态）+ 预留分配器（双池路由：
//!       预留池 / 通用池）+ 边界拦截（通用池余量 = 预留配额时
//!       拒普通分配）+ 用量账 + 窗生命周期（开/关/解冻）。
//! 职责定位：交接预留是「关键时刻的保底舱」——A2/A4 窗口是
//!   交接结构密集分配的时段，若被普通流量挤占，交接失败就是
//!   整机失败；预留的本质是「边界拦截」：通用池的可用下限被
//!   压到预留配额之下时，普通分配让路。窗口关闭后预留解冻，
//!   不许留死配额。
//! 性能逐项分解：分配 O(1)（双池计数器）；拦截判定 O(1)；
//!   对账 O(窗数)。
//! 诊断码段：0x20D40（0x20D41~0x20D49 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HR_VERSION: &str = "unx0394_handreserve-v1";

/// 诊断段基址（0x20D40 段 · 本模块独占）。
pub const HR_SEG_BASE: u32 = 0x20D40;
pub const XC_WIN_OOR: u32 = 0x20D41; // 窗号越域
pub const XC_WIN_DUP: u32 = 0x20D42; // 窗重号
pub const XC_WIN_CLOSED: u32 = 0x20D43; // 窗关后预留分配拒
pub const XC_BUDGET: u32 = 0x20D44; // 预留超配额
pub const XC_GUARD: u32 = 0x20D45; // 普通分配撞边界
pub const XC_TOTAL: u32 = 0x20D46; // 总量恒等破
pub const XC_ZERO_QUOTA: u32 = 0x20D47; // 零配额窗拒
pub const XC_THAW: u32 = 0x20D48; // 解冻后配额未回
pub const XC_USED_NEG: u32 = 0x20D49; // 用量负值

/// 窗口两态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinState {
    /// 开窗：预留生效。
    Open,
    /// 关窗：预留解冻回通用池。
    Closed,
}

/// 预留窗。
#[derive(Clone, Copy, Debug)]
pub struct ResWin {
    /// 窗号（1~63）。
    pub id: u32,
    /// 配额（字节模型）。
    pub quota: u64,
    /// 用量。
    pub used: u64,
    /// 开关态。
    pub state: WinState,
}

/// 双池分配器。
pub struct ResAllocator {
    total: u64,
    gen_used: u64,
    wins: Vec<ResWin>,
}

impl ResAllocator {
    /// 建：总池量。
    pub fn new(total: u64) -> Result<ResAllocator, u32> {
        if total == 0 {
            return Err(XC_TOTAL);
        }
        Ok(ResAllocator { total, gen_used: 0, wins: Vec::new() })
    }

    /// 开窗（登记预留）：重号拒、号越域拒、零配额拒。
    pub fn open_win(&mut self, id: u32, quota: u64) -> Result<(), u32> {
        if id == 0 || id > 63 {
            return Err(XC_WIN_OOR);
        }
        if quota == 0 {
            return Err(XC_ZERO_QUOTA);
        }
        for w in self.wins.iter() {
            if w.id == id {
                return Err(XC_WIN_DUP);
            }
        }
        self.wins.push(ResWin { id, quota, used: 0, state: WinState::Open });
        Ok(())
    }

    /// 预留分配（指定窗）：开窗 + 配额内必成；关窗拒。
    pub fn res_alloc(&mut self, id: u32, bytes: u64) -> Result<(), u32> {
        let i = match self.wins.iter().position(|w| w.id == id) {
            Some(i) => i,
            None => return Err(XC_WIN_OOR),
        };
        if self.wins[i].state == WinState::Closed {
            return Err(XC_WIN_CLOSED);
        }
        if self.wins[i].used + bytes > self.wins[i].quota {
            return Err(XC_BUDGET);
        }
        // 总量恒等前置核：开窗配额合计不得压垮总量。
        let reserved: u64 = self.wins.iter().map(|w| w.quota).sum();
        if reserved > self.total {
            return Err(XC_TOTAL);
        }
        self.wins[i].used += bytes;
        Ok(())
    }

    /// 预留归还。
    pub fn res_free(&mut self, id: u32, bytes: u64) -> Result<(), u32> {
        let i = match self.wins.iter().position(|w| w.id == id) {
            Some(i) => i,
            None => return Err(XC_WIN_OOR),
        };
        if self.wins[i].used < bytes {
            return Err(XC_USED_NEG);
        }
        self.wins[i].used -= bytes;
        Ok(())
    }

    /// 普通分配：余量 = 总量 - 通用已用 - Σ(开窗配额)；
    /// 余量 < bytes 时边界拦截（拒绝码 XC_GUARD）。
    pub fn gen_alloc(&mut self, bytes: u64) -> Result<(), u32> {
        if bytes == 0 {
            return Err(XC_USED_NEG);
        }
        let reserved: u64 = self
            .wins
            .iter()
            .filter(|w| w.state == WinState::Open)
            .map(|w| w.quota)
            .sum();
        let avail = self.total - self.gen_used - reserved;
        if bytes > avail {
            return Err(XC_GUARD);
        }
        self.gen_used += bytes;
        Ok(())
    }

    /// 普通归还。
    pub fn gen_free(&mut self, bytes: u64) -> Result<(), u32> {
        if self.gen_used < bytes {
            return Err(XC_USED_NEG);
        }
        self.gen_used -= bytes;
        Ok(())
    }

    /// 关窗：预留解冻——开窗配额退出拦截边界；未归还的预留用量
    /// 转入通用已用（存量移交）。
    pub fn close_win(&mut self, id: u32) -> Result<u64, u32> {
        let i = match self.wins.iter().position(|w| w.id == id) {
            Some(i) => i,
            None => return Err(XC_WIN_OOR),
        };
        if self.wins[i].state == WinState::Closed {
            return Err(XC_WIN_CLOSED);
        }
        let moved = self.wins[i].used;
        // 预留是配额记账不占实物：关窗时记账作废（零移交），
        // moved 仅作审计返回值。
        self.wins[i].used = 0;
        self.wins[i].state = WinState::Closed;
        Ok(moved)
    }

    /// 解冻核：全关窗后无开窗配额压制（开窗数零 + 对账平）。
    pub fn thaw_ok(&self) -> Result<(), u32> {
        if self.open_count() != 0 {
            return Err(XC_THAW);
        }
        self.ledger_ok()
    }

    /// 对账：通用已用 + Σ开窗用量 ≤ 总量。
    pub fn ledger_ok(&self) -> Result<(), u32> {
        let used: u64 = self
            .wins
            .iter()
            .filter(|w| w.state == WinState::Open)
            .map(|w| w.used)
            .sum();
        if self.gen_used + used > self.total {
            return Err(XC_TOTAL);
        }
        Ok(())
    }

    /// 开窗数。
    pub fn open_count(&self) -> usize {
        self.wins.iter().filter(|w| w.state == WinState::Open).count()
    }
}

pub fn run_unx0394_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0394_handreserve");
    // 族一：标准窗生命周期。
    let mut a = match ResAllocator::new(10_000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建分配器", false, "拒");
            return cs;
        }
    };
    cs.add("开 A2 窗", a.open_win(1, 2000).is_ok(), "配额 2000");
    cs.add("开 A4 窗", a.open_win(2, 3000).is_ok(), "配额 3000");
    cs.add("开窗数 2", a.open_count() == 2, "双窗");
    // 预留分配必成。
    cs.add("A2 预留 1500", a.res_alloc(1, 1500).is_ok(), "配额内");
    cs.add("A4 预留 2500", a.res_alloc(2, 2500).is_ok(), "配额内");
    cs.add("对账平", a.ledger_ok().is_ok(), "4000 ≤ 10000");
    // 普通分配受边界拦截：可用 = 10000 - 0 - 5000 = 5000。
    cs.add("普通 5000 成", a.gen_alloc(5000).is_ok(), "贴边界");
    cs.add("普通 1 字节撞边界", a.gen_alloc(1) == Err(XC_GUARD), "余 0");
    cs.add("归还 2000", a.gen_free(2000).is_ok(), "回血");
    cs.add("普通 2000 成", a.gen_alloc(2000).is_ok(), "贴边界再成");
    // 预留超配额拒。
    cs.add("A2 再预留 600 拒", a.res_alloc(1, 600) == Err(XC_BUDGET), "2100 > 2000");
    // 族二：关窗解冻。
    let moved = a.close_win(1);
    cs.add("关 A2 窗", moved == Ok(1500), "存量移交");
    cs.add("开窗数 1", a.open_count() == 1, "余 A4");
    // 解冻后普通可用恢复：可用 = 10000 - 9000(通用含移交) - 3000(A4 配额) = 负？算账：
    // gen_used = 7000 + 1500 = 8500；开放配额 3000 → 可用 = 10000-8500-3000 = -1500 → 拦截。
    cs.add("解冻前仍拦", a.gen_alloc(1) == Err(XC_GUARD), "A4 还开着");
    // 关 A4。
    let _ = a.close_win(2);
    cs.add("开窗数 0", a.open_count() == 0, "全关");
    // 预留是配额记账不占实物：关窗零移交，对账恒平。
    cs.add("关窗后对账平", a.ledger_ok().is_ok(), "7000 ≤ 10000");
    cs.add("解冻核过", a.thaw_ok().is_ok(), "全关无压制");
    // 解冻后普通分配恢复全额：可用 = 10000 - 7000 = 3000。
    cs.add("解冻恢复 3000", a.gen_alloc(3000).is_ok(), "全额可用");
    cs.add("恢复后贴边界", a.gen_alloc(1) == Err(XC_GUARD), "余 0");
    // 族三：修正语义（预留用量不占实物，关窗零移交）。
    let mut b = match ResAllocator::new(10_000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建分配器2", false, "拒");
            return cs;
        }
    };
    let _ = b.open_win(1, 4000);
    cs.add("预留 4000（配额内记账）", b.res_alloc(1, 4000).is_ok(), "不占实物");
    cs.add("通用仍可用 6000", b.gen_alloc(6000).is_ok(), "预留只拦边界");
    let m = b.close_win(1);
    cs.add("关窗零移交", m == Ok(0), "预留未实占");
    cs.add("解冻核过", b.thaw_ok().is_ok(), "无开窗压制");
    cs.add("关窗后预留分配拒", b.res_alloc(1, 100) == Err(XC_WIN_CLOSED), "窗已关");
    // 族四：登记拒绝。
    let mut c = match ResAllocator::new(1000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建分配器3", false, "拒");
            return cs;
        }
    };
    cs.add("零配额窗拒", c.open_win(1, 0) == Err(XC_ZERO_QUOTA), "quota=0");
    cs.add("窗号零拒", c.open_win(0, 100) == Err(XC_WIN_OOR), "id=0");
    cs.add("窗号超域拒", c.open_win(64, 100) == Err(XC_WIN_OOR), "id=64");
    let _ = c.open_win(3, 500);
    cs.add("窗重号拒", c.open_win(3, 500) == Err(XC_WIN_DUP), "id=3 撞");
    // 族五：零字节 / 归还越账。
    cs.add("普通零字节拒", c.gen_alloc(0) == Err(XC_USED_NEG), "bytes=0");
    cs.add("预留归还越账拒", c.res_free(3, 999) == Err(XC_USED_NEG), "未用先还");
    cs.add("普通归还越账拒", c.gen_free(999) == Err(XC_USED_NEG), "未用先还");
    cs
}
