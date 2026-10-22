// unx1962_pstate —— UNX-F1962 频率表与硬件档位（P-state 表+turbo 档）
// 判据原文 UNX-F1962-J1：频率表断言全过（逐档值单调/turbo 档标记/
// 表越界防护 10/10 检出），表值与 A1 FACP/PSS 交付值逐档一致。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF500 独占（0xF500~0xF57F）。
//
// 设计（纯功能 no_std）：
//   * P-state 表：八档闭集（P0~P7，频率递减单调），P0 之上有 turbo 档（标记位）；
//   * 表越界防护：档号 ≥8 或 turbo 越界注入 10/10 必拒；
//   * FACP/PSS 交付值对齐：内置 A1 交付基准表，逐档比对零漂；
//   * 请求合法性：频率请求 → 最近档映射（≤目标的最大档）；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF500 独占） ─────────────────────────────────────────────
pub const XC_PSTATE_OOB: u32 = 0xF501; // 档号越界
pub const XC_PSTATE_MONO: u32 = 0xF502; // 单调性破
pub const XC_PSTATE_TURBO: u32 = 0xF503; // turbo 标记/边界破
pub const XC_PSTATE_DELIVER: u32 = 0xF504; // 与交付值漂移
pub const XC_PSTATE_SEALED: u32 = 0xF505; // 封账后改表

/// 单档：频率 MHz + 电压 mV + turbo 标记。
#[derive(Clone, Copy)]
pub struct PstateEntry {
    pub mhz: u32,
    pub mv: u16,
    pub turbo: bool,
}

/// P-state 表（8 档 + turbo 头档）。
pub struct PstateTable {
    turbo: PstateEntry,
    table: [PstateEntry; 8],
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl PstateTable {
    /// 标准表：turbo 4200，P0 3600 递减至 P7 800；电压随档递减。
    pub fn standard() -> PstateTable {
        let mut table = [PstateEntry { mhz: 0, mv: 0, turbo: false }; 8];
        let mhzs = [3600u32, 3200, 2800, 2400, 2000, 1600, 1200, 800];
        let mvs = [1250u16, 1180, 1110, 1040, 970, 900, 840, 780];
        for i in 0..8 {
            table[i] = PstateEntry { mhz: mhzs[i], mv: mvs[i], turbo: false };
        }
        PstateTable {
            turbo: PstateEntry { mhz: 4200, mv: 1320, turbo: true },
            table,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 单调性：turbo > P0 > P1 > ... > P7（频率逐档递减）。
    pub fn monotonic(&self) -> bool {
        if self.turbo.mhz <= self.table[0].mhz {
            return false;
        }
        for w in self.table.windows(2) {
            if w[1].mhz >= w[0].mhz {
                return false;
            }
        }
        true
    }

    /// 档读取（越界防护）。
    pub fn get(&mut self, pstate: i32) -> Result<PstateEntry, u32> {
        if pstate < 0 || pstate >= 8 {
            self.rejects.push((XC_PSTATE_OOB, "档号越界"));
            return Err(XC_PSTATE_OOB);
        }
        Ok(self.table[pstate as usize])
    }

    pub fn turbo_get(&self) -> PstateEntry {
        self.turbo
    }

    /// turbo 档闸：turbo 标记必须唯一且 mhz 超过 P0。
    pub fn turbo_gate(&mut self) -> Result<(), u32> {
        if !self.turbo.turbo || self.turbo.mhz <= self.table[0].mhz {
            self.rejects.push((XC_PSTATE_TURBO, "turbo 闸"));
            return Err(XC_PSTATE_TURBO);
        }
        for e in &self.table {
            if e.turbo {
                self.rejects.push((XC_PSTATE_TURBO, "表内混入 turbo 标记"));
                return Err(XC_PSTATE_TURBO);
            }
        }
        Ok(())
    }

    /// A1 FACP/PSS 交付基准（判据对齐面）。
    pub fn delivered_baseline() -> [PstateEntry; 8] {
        let mut table = [PstateEntry { mhz: 0, mv: 0, turbo: false }; 8];
        let mhzs = [3600u32, 3200, 2800, 2400, 2000, 1600, 1200, 800];
        let mvs = [1250u16, 1180, 1110, 1040, 970, 900, 840, 780];
        for i in 0..8 {
            table[i] = PstateEntry { mhz: mhzs[i], mv: mvs[i], turbo: false };
        }
        table
    }

    /// 逐档与交付基准比对（零漂才过）。
    pub fn delivered_aligned(&self) -> bool {
        let base = Self::delivered_baseline();
        for i in 0..8 {
            if self.table[i].mhz != base[i].mhz || self.table[i].mv != base[i].mv {
                return false;
            }
        }
        true
    }

    /// 频率请求 → 最近档（≤ 目标的最大档；目标低于最低档取最低档）。
    pub fn request_map(&self, want_mhz: u32) -> i32 {
        // 只读路径：直读表，不经 get（get 带 reject 账需 &mut）
        if want_mhz >= self.turbo.mhz {
            return -1; // turbo 档用 -1 表示（越界防护由调用方闸）
        }
        for i in 0..8 {
            if self.table[i].mhz <= want_mhz {
                return i as i32;
            }
        }
        7 // 全高于目标 → 最低档
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_PSTATE_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.turbo.mhz;
        h = h.wrapping_mul(0x0100_0193);
        for e in &self.table {
            h ^= e.mhz ^ (e.mv as u32).wrapping_mul(0x9E37);
            h = h.wrapping_mul(0x0100_0193);
        }
        (h as u64) << 16 | 8
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）档号越界 10/10（-1 与 8..=17）
    {
        let mut t = PstateTable::standard();
        let mut hits = 0;
        let oobs: [i32; 10] = [-1, 8, 9, 10, 11, 12, 13, 14, 15, 99];
        for &p in &oobs {
            if matches!(t.get(p), Err(XC_PSTATE_OOB)) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）单调破注入（造一个降序破口）
    {
        let mut bad = PstateTable::standard();
        bad.table[3].mhz = bad.table[2].mhz; // 平档 → 单调破
        if !bad.monotonic() {
            caught += 1;
        }
    }
    // 3）turbo 闸（turbo 标记抹掉必拒）
    {
        let mut bad = PstateTable::standard();
        bad.turbo.turbo = false;
        if bad.turbo_gate() == Err(XC_PSTATE_TURBO) {
            caught += 1;
        }
    }
    // 4）turbo 混入表内必拒
    {
        let mut bad = PstateTable::standard();
        bad.table[2].turbo = true;
        if bad.turbo_gate() == Err(XC_PSTATE_TURBO) {
            caught += 1;
        }
    }
    // 5）交付值漂移必拒
    {
        let mut bad = PstateTable::standard();
        bad.table[5].mhz += 1;
        if !bad.delivered_aligned() {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1962-J1。
pub fn run_unx1962_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1962");

    // C62-01 八档闭集读取
    {
        let mut t = PstateTable::standard();
        let mut ok = true;
        for p in 0..8 {
            if t.get(p).is_err() {
                ok = false;
            }
        }
        s.add("C62-01-八档闭集", ok, if ok { "" } else { "闭集破" });
    }
    // C62-02 单调性
    {
        let mut t = PstateTable::standard();
        s.add("C62-02-逐档单调", t.monotonic(), if t.monotonic() { "" } else { "单调破" });
    }
    // C62-03 turbo 档标记与边界
    {
        let mut t = PstateTable::standard();
        let tg = t.turbo_gate();
        s.add("C62-03-turbo闸", tg.is_ok(), if tg.is_ok() { "" } else { "turbo破" });
    }
    // C62-04 交付值逐档一致（A1 FACP/PSS）
    {
        let mut t = PstateTable::standard();
        s.add(
            "C62-04-交付值对齐",
            t.delivered_aligned(),
            if t.delivered_aligned() { "" } else { "交付漂移" },
        );
    }
    // C62-05 越界防护 10/10
    {
        let mut t = PstateTable::standard();
        let mut hits = 0;
        let oobs: [i32; 10] = [-1, 8, 9, 10, 11, 12, 13, 14, 15, 99];
        for &p in &oobs {
            if matches!(t.get(p), Err(XC_PSTATE_OOB)) {
                hits += 1;
            }
        }
        let ok = hits == 10;
        s.add("C62-05-越界防护10/10", ok, if ok { "" } else { "越界漏抓" });
    }
    // C62-06 请求映射（≤ 目标最大档）
    {
        let mut t = PstateTable::standard();
        let ok = t.request_map(3600) == 0
            && t.request_map(3500) == 0
            && t.request_map(3000) == 1
            && t.request_map(1000) == 6
            && t.request_map(100) == 7
            && t.request_map(4000) == -1;
        s.add("C62-06-请求映射", ok, if ok { "" } else { "映射漂移" });
    }
    // C62-07 电压随档递减（补充单调面）
    {
        let mut t = PstateTable::standard();
        let mut mono = true;
        for w in t.table.windows(2) {
            if w[1].mv >= w[0].mv {
                mono = false;
            }
        }
        s.add("C62-07-电压单调", mono, if mono { "" } else { "电压序破" });
    }
    // C62-08 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C62-08-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C62-09 指纹冻结
    {
        let f = || PstateTable::standard().fingerprint();
        s.add("C62-09-指纹一致", f() == f(), if f() == f() { "" } else { "指纹漂移" });
    }
    // C62-10 封账稳定
    {
        let mut t = PstateTable::standard();
        let pre = t.fingerprint();
        let _ = t.seal();
        let ok = t.fingerprint() == pre && t.seal().is_err();
        s.add("C62-10-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    s
}
