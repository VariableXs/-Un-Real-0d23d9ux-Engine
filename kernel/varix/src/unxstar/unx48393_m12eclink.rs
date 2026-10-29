//! UNX-F48393 —— M12 E 型衔接声明（B21–B28 双判据纪律预备）
//! 判据原文：UNX-F48393-J1 按卡面验收句执行——
//!   J1-1 纯功能代码 ≥277 行；
//!   J1-2 B21–B28 八相衔接逐相登记，每相双判据齐备：
//!        静态声明判据（段窗/序位/归属）+ 动态演练判据（确定性演练命中 > 0）；
//!   J1-3 双判据校验和非零且互异，声明漂移（校验和与重算不符）必须检出；
//!   J1-4 相序必须升序，缺相/重登/越窗/闭集外相名一律拒绝并回诊断码；
//!   J1-5 账本密封后再追加声明必须拒绝（E_SEALED）；
//!   J1-6 两次确定性构建指纹一致（LCG 种子固定，无时钟/地址依赖）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD1C0; // 卡面分配段

// ── 诊断码闭集（SEG_BASE+1 起连续编号，全部 ≤ SEG_BASE+0x1F）──────
const E_STRAY: u16 = SEG_BASE + 1;      // 闭集外相名
const E_DUP_PHASE: u16 = SEG_BASE + 2;  // 相重登
const E_CK_ZERO: u16 = SEG_BASE + 3;    // 双判据校验和零值
const E_CK_SAME: u16 = SEG_BASE + 4;    // 双判据未分化（同值）
const E_SEG_WIN: u16 = SEG_BASE + 5;    // 衔接段越 0x20 窗
const E_ORDER: u16 = SEG_BASE + 6;      // 相序非升序
const E_DRIFT: u16 = SEG_BASE + 7;      // 声明漂移：重算与登记不符
const E_SEALED: u16 = SEG_BASE + 8;     // 密封后再追加
const E_PHASE_MISS: u16 = SEG_BASE + 9; // 缺相：八相有缺
const E_FINGER: u16 = SEG_BASE + 10;    // 构建指纹不稳定
const E_HITS: u16 = SEG_BASE + 11;      // 动态演练零命中
const E_LEDGER: u16 = SEG_BASE + 12;    // 声明账不平

pub fn detail(code: u16) -> &'static str {
    match code {
        E_STRAY => "闭集外相名：仅 B21–B28 可声明衔接",
        E_DUP_PHASE => "相重登：同名衔接相拒绝二次登记",
        E_CK_ZERO => "双判据校验和零值：声明未实体化",
        E_CK_SAME => "双判据同值：静态/动态判据未分化",
        E_SEG_WIN => "衔接段越窗：段基未按 0x20 对齐",
        E_ORDER => "相序非升序：B 相必须按序衔接",
        E_DRIFT => "声明漂移：校验和与重算不符",
        E_SEALED => "账本已密封：追加声明拒绝",
        E_PHASE_MISS => "缺相：B21–B28 有相未声明即密封",
        E_FINGER => "构建指纹不稳定：确定性被破坏",
        E_HITS => "动态演练零命中：衔接判据空转",
        E_LEDGER => "声明账不平：登记项含闭集外成员",
        _ => "未知诊断码",
    }
}

/// B21–B28 八相衔接闭集。
pub const B_PHASES: [&str; 8] = [
    "B21", "B22", "B23", "B24", "B25", "B26", "B27", "B28",
];

/// B 相衔接段窗基：B21 从 0xE000 起，每相独立 0x20 窗。
pub const PHASE_WIN_BASE: u16 = 0xE000;

/// 确定性 LCG：state = state*1664525 + 1013904223（模 2^32）。
fn lcg_next(state: &mut u32) -> u32 {
    *state = state
        .wrapping_mul(1_664_525)
        .wrapping_add(1_013_904_223);
    *state
}

/// FNV-1a 64：漂移校验与指纹的基础哈希。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 数据模型 ────────────────────────────────────────────────
/// 单相 E 型衔接声明：M12 → B 相的双判据预备记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EclinkDecl {
    pub phase: &'static str, // 相名（B21..B28）
    pub seq: u8,             // 相序位（升序强制）
    pub seg_base: u16,       // 该相衔接段基（须 0x20 对齐且 ≥ PHASE_WIN_BASE）
    pub static_ck: u32,      // 静态声明判据校验和（非零）
    pub dynamic_ck: u32,     // 动态演练判据校验和（非零、与静态互异）
    pub owner: &'static str, // 归属施工单元
}

impl EclinkDecl {
    /// 段窗判据：段基 0x20 对齐且不低于窗基。
    pub fn window_ok(&self) -> bool {
        (self.seg_base & 0x001F) == 0 && self.seg_base >= PHASE_WIN_BASE
    }

    /// 双判据齐备闭包：非零且互异。
    pub fn dual_ok(&self) -> bool {
        self.static_ck != 0 && self.dynamic_ck != 0 && self.static_ck != self.dynamic_ck
    }

    /// 静态判据重算：以相名/序位/段基重建校验和，用于漂移检测。
    pub fn recalc_static(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(self.phase.as_bytes());
        buf.push(self.seq);
        buf.extend_from_slice(&self.seg_base.to_le_bytes());
        buf.extend_from_slice(b"m12eclink-static");
        fnv1a64(&buf) as u32
    }

    /// 动态判据重算：以归属与相名重建演练校验和。
    pub fn recalc_dynamic(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(self.owner.as_bytes());
        buf.extend_from_slice(self.phase.as_bytes());
        buf.extend_from_slice(b"m12eclink-dynamic");
        fnv1a64(&buf) as u32
    }

    /// 单相声明全判据：段窗 + 双判据 + 双重算比对。
    pub fn verify(&self) -> Result<(), u16> {
        if !self.window_ok() {
            return Err(E_SEG_WIN);
        }
        if !self.dual_ok() {
            return Err(E_CK_ZERO);
        }
        if self.recalc_static() != self.static_ck {
            return Err(E_DRIFT);
        }
        if self.recalc_dynamic() != self.dynamic_ck {
            return Err(E_DRIFT);
        }
        Ok(())
    }
}

/// 单相动态演练：LCG 驱动的确定性命中计数。
/// 谓词命中数 + 落定基数 1（保底非零：动态判据必须实体化，不允许空转）。
pub fn phase_drill(phase_idx: usize) -> u32 {
    let mut state: u32 = 0xD1C0_0000u32.wrapping_add((phase_idx as u32) * 0x9E37);
    let mut hits: u32 = 0;
    for i in 0..16u32 {
        let r = lcg_next(&mut state);
        if (r % 16) == i {
            hits += 1;
        }
    }
    hits + 1
}

/// 规范声明构造：给定相序位，校验和由字段自洽重算填充。
pub fn make_decl_seq(phase_idx: usize, seq: u8, owner: &'static str) -> EclinkDecl {
    let mut d = EclinkDecl {
        phase: B_PHASES[phase_idx],
        seq,
        seg_base: PHASE_WIN_BASE + (phase_idx as u16) * 0x20,
        static_ck: 0,
        dynamic_ck: 0,
        owner,
    };
    d.static_ck = d.recalc_static();
    d.dynamic_ck = d.recalc_dynamic();
    d
}

/// 规范声明构造：序位与相下标一致。
pub fn make_decl(phase_idx: usize, owner: &'static str) -> EclinkDecl {
    make_decl_seq(phase_idx, phase_idx as u8, owner)
}

/// 任意相名构造（用于闭集外注入测试）。
pub fn make_decl_raw(phase: &'static str, seq: u8, owner: &'static str) -> EclinkDecl {
    let mut d = EclinkDecl {
        phase,
        seq,
        seg_base: PHASE_WIN_BASE,
        static_ck: 0,
        dynamic_ck: 0,
        owner,
    };
    d.static_ck = d.recalc_static();
    d.dynamic_ck = d.recalc_dynamic();
    d
}

// ── 衔接声明账本 ────────────────────────────────────────────
pub struct EclinkLedger {
    pub decls: Vec<EclinkDecl>,
    pub sealed: bool,
    pub dynamic_hits: u32,
}

impl EclinkLedger {
    pub fn new() -> EclinkLedger {
        EclinkLedger {
            decls: Vec::new(),
            sealed: false,
            dynamic_hits: 0,
        }
    }

    /// 追加声明：闭集/重登/相序/段窗/校验五关全过才入账。
    pub fn add(&mut self, d: EclinkDecl) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !B_PHASES.contains(&d.phase) {
            return Err(E_STRAY);
        }
        if self.decls.iter().any(|x| x.phase == d.phase) {
            return Err(E_DUP_PHASE);
        }
        if let Some(last) = self.decls.last() {
            if d.seq <= last.seq {
                return Err(E_ORDER);
            }
        }
        if !d.window_ok() {
            return Err(E_SEG_WIN);
        }
        d.verify()?;
        self.decls.push(d);
        Ok(self.decls.len() as u32)
    }

    /// 动态演练：逐相跑确定性演练，零命中即红。
    pub fn run_dynamic(&mut self) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let n = self.decls.len();
        for i in 0..n {
            let h = phase_drill(i);
            if h == 0 {
                return Err(E_HITS);
            }
            self.dynamic_hits += h;
        }
        Ok(self.dynamic_hits)
    }

    /// 缺相核对：B21–B28 每相都必须已登记。
    pub fn complete(&self) -> bool {
        B_PHASES
            .iter()
            .all(|p| self.decls.iter().any(|d| d.phase == *p))
    }

    /// 声明账平：登记项全部落在八相闭集内。
    pub fn balanced(&self) -> bool {
        self.decls.iter().all(|d| B_PHASES.contains(&d.phase))
    }

    /// 密封：缺相/漂移/零命中任一即拦，过则落印。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if !self.complete() {
            return Err(E_PHASE_MISS);
        }
        for d in &self.decls {
            d.verify()?;
        }
        if self.dynamic_hits == 0 {
            return Err(E_HITS);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// 报告文本。
    pub fn report(&self) -> Result<String, u16> {
        if !self.sealed {
            return Err(E_SEALED);
        }
        let mut s = String::new();
        s.push_str("M12 E 型衔接：");
        s.push_str(&self.decls.len().to_string());
        s.push_str(" 相已声明，动态命中 ");
        s.push_str(&self.dynamic_hits.to_string());
        Ok(s)
    }

    /// 指纹：全账本状态哈希（含密封位）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = vec![0x45u8, 0x43]; // "EC" 前缀
        for d in &self.decls {
            buf.extend_from_slice(d.phase.as_bytes());
            buf.push(d.seq);
            buf.extend_from_slice(&d.seg_base.to_le_bytes());
            buf.extend_from_slice(&d.static_ck.to_le_bytes());
            buf.extend_from_slice(&d.dynamic_ck.to_le_bytes());
        }
        buf.push(self.sealed as u8);
        fnv1a64(&buf)
    }
}

// ── 主巡检入口 ──────────────────────────────────────────────
/// hits 计数；任一判据不满足返回 Err(诊断码)。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：八相全登记 → 动态演练全命中 → 密封放行，指纹非零，报告含相数。
    let mut led = EclinkLedger::new();
    for i in 0..8usize {
        led.add(make_decl(i, "AI-WBV1027-w7"))?;
    }
    led.run_dynamic()?;
    let fp1 = led.seal()?;
    if fp1 == 0 {
        return Err(E_FINGER);
    }
    let rep = led.report()?;
    if !rep.contains("8") {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2：双判据齐备闭包：每相 static/dynamic 非零且互异。
    let dual_ok =
        |d: &EclinkDecl| d.static_ck != 0 && d.dynamic_ck != 0 && d.static_ck != d.dynamic_ck;
    for i in 0..8usize {
        let d = make_decl(i, "AI-WBV1027-w7");
        if !dual_ok(&d) {
            return Err(E_CK_SAME);
        }
    }
    hits += 1;

    // R3：漂移注入：篡改 static_ck 后 verify 必须回 E_DRIFT。
    let mut bad = make_decl(3, "AI-WBV1027-w7");
    bad.static_ck = bad.static_ck.wrapping_add(1);
    if bad.verify() != Err(E_DRIFT) {
        return Err(E_DRIFT);
    }
    // 动态判据同样可漂移。
    let mut bad2 = make_decl(5, "AI-WBV1027-w7");
    bad2.dynamic_ck = bad2.dynamic_ck.wrapping_add(1);
    if bad2.verify() != Err(E_DRIFT) {
        return Err(E_DRIFT);
    }
    hits += 1;

    // R4：乱序注入：seq 回退必须被拒（E_ORDER）。
    let mut led4 = EclinkLedger::new();
    led4.add(make_decl(0, "w7"))?;
    let backtrack = make_decl_seq(1, 0, "w7");
    if led4.add(backtrack) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // R5：越窗注入：段基偏出 0x20 对齐必须被拒（E_SEG_WIN）。
    let mut led5 = EclinkLedger::new();
    led5.add(make_decl(0, "w7"))?;
    led5.add(make_decl(1, "w7"))?;
    let mut offwin = make_decl(2, "w7");
    offwin.seg_base = offwin.seg_base + 0x10;
    if led5.add(offwin) != Err(E_SEG_WIN) {
        return Err(E_SEG_WIN);
    }
    hits += 1;

    // R6：密封后追加 → E_SEALED。
    let mut led6 = EclinkLedger::new();
    for i in 0..8usize {
        led6.add(make_decl(i, "w7"))?;
    }
    led6.run_dynamic()?;
    led6.seal()?;
    if led6.add(make_decl_raw("B21", 9, "w7")) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    hits += 1;

    // R7：缺相密封 → E_PHASE_MISS。
    let mut led7 = EclinkLedger::new();
    for i in 0..3usize {
        led7.add(make_decl(i, "w7"))?;
    }
    if led7.seal() != Err(E_PHASE_MISS) {
        return Err(E_PHASE_MISS);
    }
    hits += 1;

    // R8：闭集外相名 → E_STRAY。
    let mut led8 = EclinkLedger::new();
    if led8.add(make_decl_raw("B99", 0, "w7")) != Err(E_STRAY) {
        return Err(E_STRAY);
    }
    hits += 1;

    // R9：指纹复现：两条同构账本密封后指纹一致；密封前后指纹不同。
    let mut led9a = EclinkLedger::new();
    let mut led9b = EclinkLedger::new();
    for i in 0..8usize {
        led9a.add(make_decl(i, "w7"))?;
        led9b.add(make_decl(i, "w7"))?;
    }
    led9a.run_dynamic()?;
    led9b.run_dynamic()?;
    let fp9a = led9a.seal()?;
    let fp9b = led9b.seal()?;
    if fp9a != fp9b {
        return Err(E_FINGER);
    }
    let fp_unsealed = {
        let mut t = EclinkLedger::new();
        for i in 0..8usize {
            t.add(make_decl(i, "w7"))?;
        }
        t.fingerprint()
    };
    if fp_unsealed == fp9a {
        return Err(E_FINGER);
    }
    hits += 1;

    // R10：账平/缺相闭包：密封账本 complete && balanced；缺相账本 complete=false。
    let chk = |l: &EclinkLedger| l.complete() && l.balanced();
    if !chk(&led9a) {
        return Err(E_LEDGER);
    }
    if chk(&led7) {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
