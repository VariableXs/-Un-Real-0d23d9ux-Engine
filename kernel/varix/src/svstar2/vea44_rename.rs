//! VE-F0044 · 资源重命名语义（VE-A 域 · A02b 资源视图组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0044`
//!
//! **职责定位（锚点原文）**：GPU 资源重命名的语义规范（同显存新内容替换
//! 旧内容的安全时序），重命名与帧图的联动（A31 屏障自动覆盖）。
//!
//! **数据结构（锚点原文）**：重命名器。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 时序违例 → 帧图拒绝（旧世代未归零引用时重命名被帧图拒绝）；
//! - 旧内容读取 → 检测（对旧世代的读取请求检出并阻断）；
//! - 语义漂移 → 修正（世代号回退等违例自动纠正为单调语义）。
//!
//! **性能逐项分解（锚点原文）**：O(1)。重命名 O(1) 世代推进；读取判定
//! O(1) 代际比对。
//!
//! **跨批对接点（锚点原文）**：A31 帧图联动——重命名请求携带帧图节点，
//! 帧图按世代差插入屏障。
//!
//! **无障碍与隐私（锚点原文）**：重命名状态读屏可达。
//!
//! **判据（锚点原文）**：安全时序、帧图联动、旧读检测、语义规范、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0x2200 基址（全域核查无占用）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const RENAME_VERSION: &str = "vea44_rename-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0x2200;

// ---- 诊断码 ----
pub const XC_UNKNOWN_RES: u32 = SEG_BASE + 1; // 未登记资源
pub const XC_TIMING: u32 = SEG_BASE + 2; // 时序违例（旧世代引用未归零）
pub const XC_OLD_READ: u32 = SEG_BASE + 3; // 旧世代读取检出
pub const XC_REGRESS: u32 = SEG_BASE + 4; // 世代回退（语义漂移修正对象）

/// 单资源重命名账。
#[derive(Clone, Copy, Debug)]
struct RenameLedger {
    res_id: u32,
    /// 当前世代（每次重命名 +1，单调不减）。
    gen: u32,
    /// 旧世代在飞引用数（归零后才允许下一次重命名——安全时序的核心）。
    old_refs: u32,
    /// 旧内容读取检出次数。
    old_reads: u32,
}

impl RenameLedger {
    fn new(res_id: u32) -> RenameLedger {
        RenameLedger { res_id, gen: 1, old_refs: 0, old_reads: 0 }
    }
}

/// 重命名器：世代账本 + 时序闸门。
pub struct Renamer {
    ledgers: Vec<RenameLedger>,
    cap: usize,
    last_code: u32,
    /// 时序违例拒绝次数（帧图联动统计）。
    timing_rejects: u32,
    /// 语义漂移修正次数。
    drift_fixes: u32,
    /// 帧图联动：屏障建议输出（最近一次重命名的世代差）。
    last_barrier_gen: u32,
}

impl Renamer {
    pub fn new(cap: usize) -> Renamer {
        Renamer {
            ledgers: Vec::new(),
            cap,
            last_code: 0,
            timing_rejects: 0,
            drift_fixes: 0,
            last_barrier_gen: 0,
        }
    }

    fn idx(&self, res_id: u32) -> Option<usize> {
        self.ledgers.iter().position(|l| l.res_id == res_id)
    }

    /// 登记资源（初代 1）。
    pub fn register(&mut self, res_id: u32) -> Result<(), u32> {
        if res_id == 0 || self.idx(res_id).is_some() || self.ledgers.len() >= self.cap {
            self.last_code = XC_UNKNOWN_RES;
            return Err(self.last_code);
        }
        self.ledgers.push(RenameLedger::new(res_id));
        Ok(())
    }

    /// 重命名请求（同显存新内容替换旧内容）。
    /// 安全时序：旧世代引用必须已归零；帧图联动输出世代差供屏障插入。
    pub fn rename(&mut self, res_id: u32) -> Result<u32, u32> {
        let i = match self.idx(res_id) {
            Some(i) => i,
            None => {
                self.last_code = XC_UNKNOWN_RES;
                return Err(self.last_code);
            }
        };
        if self.ledgers[i].old_refs > 0 {
            // 时序违例：旧世代仍被引用，帧图拒绝本次重命名。
            self.timing_rejects += 1;
            self.last_code = XC_TIMING;
            return Err(XC_TIMING);
        }
        let old_gen = self.ledgers[i].gen;
        self.ledgers[i].gen += 1;
        self.last_barrier_gen = self.ledgers[i].gen - old_gen;
        Ok(self.ledgers[i].gen)
    }

    /// 读取请求：携带调用方观察到的世代。
    /// 观察世代 < 当前世代 = 旧内容读取（检出阻断）；等于当前 = 放行；
    /// 大于当前 = 世代回退违例的镜像（观察方账本被篡改），按漂移修正。
    pub fn read(&mut self, res_id: u32, observed_gen: u32) -> Result<u32, u32> {
        let i = match self.idx(res_id) {
            Some(i) => i,
            None => {
                self.last_code = XC_UNKNOWN_RES;
                return Err(self.last_code);
            }
        };
        let cur = self.ledgers[i].gen;
        if observed_gen == cur {
            return Ok(cur);
        }
        if observed_gen < cur {
            self.ledgers[i].old_reads += 1;
            self.last_code = XC_OLD_READ;
            return Err(XC_OLD_READ);
        }
        // observed > cur：观察方世代超前（不可能发生的时序）→ 按漂移修正：
        // 以账本正源为准回执当前世代（修正动作计入统计）。
        self.drift_fixes += 1;
        self.last_code = XC_REGRESS;
        Ok(cur)
    }

    /// 旧世代引用记账（帧图在旧世代缓冲上起飞时 +1，归零时 -1）。
    pub fn ref_acquire(&mut self, res_id: u32) -> bool {
        match self.idx(res_id) {
            Some(i) => {
                self.ledgers[i].old_refs += 1;
                true
            }
            None => false,
        }
    }

    /// 旧世代引用释放（归零后解锁重命名）。
    pub fn ref_release(&mut self, res_id: u32) -> bool {
        match self.idx(res_id) {
            Some(i) if self.ledgers[i].old_refs > 0 => {
                self.ledgers[i].old_refs -= 1;
                true
            }
            _ => false,
        }
    }

    pub fn current_gen(&self, res_id: u32) -> Option<u32> {
        self.idx(res_id).map(|i| self.ledgers[i].gen)
    }

    pub fn last_barrier_gen(&self) -> u32 {
        self.last_barrier_gen
    }
    pub fn timing_rejects(&self) -> u32 {
        self.timing_rejects
    }
    pub fn drift_fixes(&self) -> u32 {
        self.drift_fixes
    }
    pub fn last_code(&self) -> u32 {
        self.last_code
    }
    pub fn len(&self) -> usize {
        self.ledgers.len()
    }

    /// 读屏状态行。
    pub fn status_line(&self) -> String {
        let inflight = self.ledgers.iter().filter(|l| l.old_refs > 0).count();
        alloc::format!(
            "RENAME 在册{} 在飞引用资源{} 时序拒{} 漂移修正{}",
            self.ledgers.len(),
            inflight,
            self.timing_rejects,
            self.drift_fixes
        )
    }
}

// =====================================================================
// 自检（判据锚点：安全时序 / 帧图联动 / 旧读检测 / 语义规范）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0044 域自检。
pub fn run_vea44_checks() -> CheckSet {
    let mut set = CheckSet::new("vea44");

    // C44-01 世代单调推进：重命名 gen 1→2→3
    {
        let mut r = Renamer::new(8);
        let _ = r.register(5);
        let g1 = r.rename(5);
        let g2 = r.rename(5);
        let ok = g1 == Ok(2) && g2 == Ok(3);
        chk(&mut set, "C44-01 世代单调", ok, "世代推进失准");
    }

    // C44-02 安全时序：旧世代在飞时重命名被拒
    {
        let mut r = Renamer::new(8);
        let _ = r.register(5);
        let _ = r.ref_acquire(5);
        let ok = r.rename(5) == Err(XC_TIMING) && r.timing_rejects() == 1;
        chk(&mut set, "C44-02 时序拒绝", ok, "在飞引用未拦截");
    }

    // C44-03 引用归零后重命名解锁
    {
        let mut r = Renamer::new(8);
        let _ = r.register(5);
        let _ = r.ref_acquire(5);
        let _ = r.ref_release(5);
        let ok = matches!(r.rename(5), Ok(2));
        chk(&mut set, "C44-03 归零解锁", ok, "归零未解锁");
    }

    // C44-04 旧读检测：观察旧世代 = 阻断 + 计数
    {
        let mut r = Renamer::new(8);
        let _ = r.register(5);
        let _ = r.rename(5); // gen=2
        let ok = r.read(5, 1) == Err(XC_OLD_READ);
        chk(&mut set, "C44-04 旧读检出", ok, "旧读未检出");
    }

    // C44-05 当前世代读取放行
    {
        let mut r = Renamer::new(8);
        let _ = r.register(5);
        let ok = r.read(5, 1).is_ok();
        chk(&mut set, "C44-05 当前读过", ok, "当前世代被误拦");
    }

    // C44-06 语义漂移修正：观察世代超前按正源回执
    {
        let mut r = Renamer::new(8);
        let _ = r.register(5);
        let g = r.read(5, 999);
        let ok = g == Ok(1) && r.drift_fixes() == 1 && r.last_code() == XC_REGRESS;
        chk(&mut set, "C44-06 漂移修正", ok, "漂移未修正");
    }

    // C44-07 帧图联动：屏障世代差输出（重命名一次 = 差 1）
    {
        let mut r = Renamer::new(8);
        let _ = r.register(5);
        let _ = r.rename(5);
        let ok = r.last_barrier_gen() == 1;
        chk(&mut set, "C44-07 屏障差", ok, "屏障世代差失准");
    }

    // C44-08 未登记资源全路径拒绝
    {
        let mut r = Renamer::new(8);
        let ok = r.rename(9) == Err(XC_UNKNOWN_RES)
            && r.read(9, 1) == Err(XC_UNKNOWN_RES)
            && !r.ref_acquire(9);
        chk(&mut set, "C44-08 未登记拒", ok, "未登记被放行");
    }

    // C44-09 重复登记拒绝
    {
        let mut r = Renamer::new(8);
        let _ = r.register(5);
        let ok = r.register(5) == Err(XC_UNKNOWN_RES);
        chk(&mut set, "C44-09 重复登记", ok, "重复登记被放行");
    }

    // C44-10 引用释放下溢不回绕
    {
        let mut r = Renamer::new(8);
        let _ = r.register(5);
        let ok = !r.ref_release(5);
        // 且不影响后续重命名（账未破坏）。
        let ok = ok && matches!(r.rename(5), Ok(2));
        chk(&mut set, "C44-10 下溢不回绕", ok, "下溢破坏账本");
    }

    // C44-11 在飞引用计数精确：两次 acquire 需两次 release
    {
        let mut r = Renamer::new(8);
        let _ = r.register(5);
        let _ = r.ref_acquire(5);
        let _ = r.ref_acquire(5);
        let _ = r.ref_release(5);
        let blocked = r.rename(5) == Err(XC_TIMING);
        let _ = r.ref_release(5);
        let unlocked = matches!(r.rename(5), Ok(2));
        chk(&mut set, "C44-11 引用精确", blocked && unlocked, "引用计数失准");
    }

    // C44-12 表满显性
    {
        let mut r = Renamer::new(1);
        let _ = r.register(5);
        let ok = r.register(6) == Err(XC_UNKNOWN_RES);
        chk(&mut set, "C44-12 表满显性", ok, "表满被静默");
    }

    // C44-13 零 id 拒绝
    {
        let mut r = Renamer::new(8);
        let ok = r.register(0) == Err(XC_UNKNOWN_RES);
        chk(&mut set, "C44-13 零 id 拒", ok, "零 id 被放行");
    }

    // C44-14 确定性
    {
        let seq = || -> (u32, u32, u32) {
            let mut r = Renamer::new(8);
            let _ = r.register(5);
            let _ = r.ref_acquire(5);
            let _ = r.rename(5);
            let _ = r.read(5, 1);
            (r.current_gen(5).unwrap_or(0), r.timing_rejects(), r.drift_fixes())
        };
        chk(&mut set, "C44-14 确定性", seq() == seq(), "两次不一致");
    }

    // C44-15 读屏行四要素
    {
        let r = Renamer::new(8);
        let line = r.status_line();
        let ok = line.contains("在册") && line.contains("在飞") && line.contains("时序拒")
            && line.contains("漂移修正");
        chk(&mut set, "C44-15 读屏要素", ok, "读屏行缺要素");
    }


    // C44-90 码位互异且段独占（判据自身：整仓唯一段 0x2200 内六码）
    {
        let codes = [XC_UNKNOWN_RES, XC_TIMING, XC_OLD_READ, XC_REGRESS];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x2200);
        chk(&mut set, "C44-90 码位互异段独占", all_diff && in_segment && codes.len() == 6,
            "诊断码互异或越段");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea44_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea44_rename_timing_gate() {
        let mut r = Renamer::new(8);
        assert!(r.register(5).is_ok());
        assert!(r.ref_acquire(5));
        assert_eq!(r.rename(5), Err(XC_TIMING), "旧世代在飞时重命名被拒");
        assert!(r.ref_release(5));
        assert_eq!(r.rename(5), Ok(2), "归零后解锁");
        assert_eq!(r.last_barrier_gen(), 1, "帧图屏障世代差");
    }

    #[test]
    fn vea44_old_read_and_drift() {
        let mut r = Renamer::new(8);
        assert!(r.register(5).is_ok());
        let _ = r.rename(5);
        assert_eq!(r.read(5, 1), Err(XC_OLD_READ), "旧世代读取检出");
        assert_eq!(r.read(5, 999), Ok(2), "观察超前按正源回执");
        assert_eq!(r.drift_fixes(), 1);
    }

    #[test]
    fn vea44_status_line_and_regress() {
        let mut r = Renamer::new(8);
        assert!(r.register(7).is_ok());
        assert!(r.status_line().contains("在册"));
        assert!(RENAME_VERSION.starts_with("vea44_rename"));
        assert_eq!(r.current_gen(7), Some(1));
    }

    #[test]
    fn vea44_checks_all_green() {
        let set = run_vea44_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0044 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
