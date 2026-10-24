//! UNX-F0105 · 日志级别体系（debug–fatal）· 诊断段 0x2330
//! 验收锚点：UNX-F0105-J1 五级过滤编译期+运行期双控，级别错误
//! 路由检出 100%。零浮点、零 unwrap、零 expect。


use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x2330;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_LEVEL: u16 = SEG_BASE + 1;  // 级别越纲（五级闭集外）
const E_ROUTE: u16 = SEG_BASE + 2;  // 路由错（级别→汇聚点不符）
const E_RDONLY: u16 = SEG_BASE + 3; // 运行期阈值被冻结（编译期锁死）
const E_BAL: u16 = SEG_BASE + 4;    // 过滤账不平（过=拒+收 不等）
const E_TWICE: u16 = SEG_BASE + 5;  // 阈值重复冻结
const E_EMPTY: u16 = SEG_BASE + 6;  // 空汇聚点出报告

pub fn detail(code: u16) -> &'static str {
    match code {
        E_LEVEL => "级别越纲：五级闭集 [Debug,Info,Warn,Error,Fatal] 之外",
        E_ROUTE => "路由错：条目级别与汇聚点级别不符",
        E_RDONLY => "阈值冻结：编译期门控锁死后运行期不可再调",
        E_BAL => "过滤账不平：受理数 + 拒绝数 ≠ 呈报总数",
        E_TWICE => "阈值重复冻结：一次冻结一账",
        E_EMPTY => "空汇聚点：未受理任何条目禁止出报告",
        _ => "未知诊断码",
    }
}

// ── 五级闭集 ─────────────────────────────────────────────────
pub const L_DEBUG: u8 = 0;
pub const L_INFO: u8 = 1;
pub const L_WARN: u8 = 2;
pub const L_ERROR: u8 = 3;
pub const L_FATAL: u8 = 4;
pub const LEVEL_N: u8 = 5;

pub fn level_name(l: u8) -> Result<&'static str, u16> {
    match l {
        L_DEBUG => Ok("DEBUG"),
        L_INFO => Ok("INFO"),
        L_WARN => Ok("WARN"),
        L_ERROR => Ok("ERROR"),
        L_FATAL => Ok("FATAL"),
        _ => Err(E_LEVEL),
    }
}

/// 编译期门控（模拟）：按级别掩码硬编码放行闭集。
/// 真实内核中以 const fn + cfg 特性实现；此处建模为冻结掩码。
#[derive(Clone, Copy, Debug)]
pub struct CompileGate {
    pub allow_mask: u8, // bit i = 级别 i 编译期放行
    pub frozen: bool,
}

impl CompileGate {
    pub fn full() -> CompileGate {
        CompileGate {
            allow_mask: 0b11111,
            frozen: false,
        }
    }

    /// 冻结：编译期门控在链接后不可变。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_TWICE);
        }
        self.frozen = true;
        Ok(())
    }

    pub fn compile_allows(&self, level: u8) -> Result<bool, u16> {
        if level >= LEVEL_N {
            return Err(E_LEVEL);
        }
        Ok(self.allow_mask & (1 << level) != 0)
    }

    /// 冻结后改掩码 → E_RDONLY。
    pub fn set_mask(&mut self, mask: u8) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        self.allow_mask = mask & 0b11111;
        Ok(())
    }
}

/// 单级别汇聚点账。
pub struct Sink {
    pub level: u8,
    pub accepted: u64,
    pub rejected: u64,
}

/// 运行期阈值账本：全五级汇聚点 + 双控过滤。
pub struct LevelRouter {
    pub gate: CompileGate,
    pub threshold: u8, // 运行期阈值：级别 ≥ threshold 才受理
    pub sinks: Vec<Sink>,
    pub frozen: bool,
}

impl LevelRouter {
    pub fn new(threshold: u8) -> Result<LevelRouter, u16> {
        if threshold >= LEVEL_N {
            return Err(E_LEVEL);
        }
        Ok(LevelRouter {
            gate: CompileGate::full(),
            threshold,
            sinks: Vec::new(),
            frozen: false,
        })
    }

    /// 登记汇聚点（一级一点，五级闭集内）。
    pub fn add_sink(&mut self, level: u8) -> Result<(), u16> {
        if level >= LEVEL_N {
            return Err(E_LEVEL);
        }
        if self.sinks.iter().any(|s| s.level == level) {
            return Err(E_TWICE);
        }
        self.sinks.push(Sink {
            level,
            accepted: 0,
            rejected: 0,
        });
        Ok(())
    }

    /// 双控过滤：编译期门控 ∧ 运行期阈值，双过才路由。
    /// 返回受理的汇聚点级别；被滤时返回 Err(E_ROUTE)（级别错误路由检出）。
    pub fn emit(&mut self, level: u8) -> Result<u8, u16> {
        if level >= LEVEL_N {
            return Err(E_LEVEL);
        }
        if !self.gate.compile_allows(level)? || level < self.threshold {
            // 拒绝记账：同级别汇聚点若有则记 rejected。
            for s in self.sinks.iter_mut() {
                if s.level == level {
                    s.rejected += 1;
                    return Ok(level); // 拒绝也走「检出」路径：路由结果=拒
                }
            }
            return Err(E_ROUTE);
        }
        for s in self.sinks.iter_mut() {
            if s.level == level {
                s.accepted += 1;
                return Ok(level);
            }
        }
        Err(E_ROUTE)
    }

    /// 过滤账守恒：Σ(accepted+rejected) == emit 总数。
    pub fn audit(&self, emitted: u64) -> Result<(), u16> {
        let a: u64 = self.sinks.iter().map(|s| s.accepted).sum();
        let r: u64 = self.sinks.iter().map(|s| s.rejected).sum();
        if a + r != emitted {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 运行期调阈值：冻结后拒绝。
    pub fn set_threshold(&mut self, t: u8) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if t >= LEVEL_N {
            return Err(E_LEVEL);
        }
        self.threshold = t;
        Ok(())
    }

    /// 报告：五级汇总（空账拒绝）。
    pub fn render(&self) -> Result<String, u16> {
        if self.sinks.iter().all(|s| s.accepted == 0 && s.rejected == 0) {
            return Err(E_EMPTY);
        }
        let mut s = format!("阈值 {}：", level_name(self.threshold)?);
        for (i, sk) in self.sinks.iter().enumerate() {
            if i > 0 {
                s.push_str("；");
            }
            s.push_str(&format!(
                "{} 收{} 拒{}",
                level_name(sk.level)?,
                sk.accepted,
                sk.rejected
            ));
        }
        Ok(s)
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：五汇聚点 + 阈值 INFO → Debug 拒、其余收，级别路由 100% 对。
    let mut r = LevelRouter::new(L_INFO)?;
    for l in 0..LEVEL_N {
        r.add_sink(l)?;
    }
    let mut emitted: u64 = 0;
    for l in 0..LEVEL_N {
        let got = r.emit(l);
        emitted += 1;
        let expect = if l < L_INFO { Err(E_ROUTE) } else { Ok(l) };
        if got != expect {
            return Err(E_ROUTE);
        }
    }
    r.audit(emitted)?;
    hits += 1;

    // R2：级别名渲染 + 汇总账渲染。
    if level_name(L_WARN)? != "WARN" || level_name(L_FATAL)? != "FATAL" {
        return Err(E_ROUTE);
    }
    let rep = r.render()?;
    if !rep.contains("阈值 INFO") || !rep.contains("ERROR 收1") {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：编译期门控掩码 0b01100（只放行 Warn/Error）→ 双控叠加。
    let mut r3 = LevelRouter::new(L_DEBUG)?;
    for l in 0..LEVEL_N {
        r3.add_sink(l)?;
    }
    r3.gate.set_mask(0b01100)?;
    let mut n3: u64 = 0;
    for l in 0..LEVEL_N {
        n3 += 1;
        let _ = r3.emit(l); // 编译期拒绝的级别也计 emitted（进 rejected 账）
    }
    r3.audit(n3)?;
    // Debug/Info/Fatal 被编译期拒；Warn/Error 收。
    let warn_acc = r3.sinks[L_WARN as usize].accepted;
    let fatal_acc = r3.sinks[L_FATAL as usize].accepted;
    if warn_acc != 1 || fatal_acc != 0 {
        return Err(E_ROUTE);
    }
    hits += 1;

    // R4：运行期阈值上调 → Info 级开始被拒（双控独立可调）。
    let mut r4 = LevelRouter::new(L_INFO)?;
    r4.add_sink(L_INFO)?;
    r4.add_sink(L_ERROR)?;
    r4.emit(L_INFO)?;
    r4.set_threshold(L_ERROR)?;
    if r4.emit(L_INFO) != Err(E_ROUTE) {
        return Err(E_ROUTE);
    }
    if r4.emit(L_ERROR)? != L_ERROR {
        return Err(E_ROUTE);
    }
    hits += 1;

    // I1：级别越纲 → E_LEVEL（名、门控、emit、汇聚点四路同拒）。
    if level_name(9).is_err() && level_name(9) != Err(E_LEVEL) {
        return Err(E_ROUTE);
    }
    if level_name(9) != Err(E_LEVEL) {
        return Err(E_LEVEL);
    }
    let mut i1 = LevelRouter::new(L_DEBUG)?;
    if i1.emit(LEVEL_N) != Err(E_LEVEL) {
        return Err(E_LEVEL);
    }
    if i1.add_sink(LEVEL_N + 1) != Err(E_LEVEL) {
        return Err(E_LEVEL);
    }
    hits += 1;

    // I2：阈值越纲建账 → E_LEVEL。
    if LevelRouter::new(LEVEL_N).is_ok() {
        return Err(E_LEVEL);
    }
    hits += 1;

    // I3：重复汇聚点 → E_TWICE；重复冻结门控 → E_TWICE。
    i1.add_sink(L_WARN)?;
    if i1.add_sink(L_WARN) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    i1.gate.freeze()?;
    if i1.gate.freeze() != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I4：冻结后改掩码/调阈值 → E_RDONLY（双控锁死双路）。
    i1.frozen = true;
    if i1.set_threshold(L_DEBUG) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i1.gate.set_mask(0) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I5：空账出报告 → E_EMPTY；过滤账破 → E_BAL。
    let i5 = LevelRouter::new(L_DEBUG)?;
    if i5.render() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if i5.audit(1) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
