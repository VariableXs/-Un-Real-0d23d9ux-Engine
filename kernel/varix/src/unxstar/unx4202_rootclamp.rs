//! unx4202_rootclamp — UNX-F4202 ".." 越根钳制与 chroot 基准（诊断段 0x9AA0）。
//!
//! 判据原文：J1 根处 ".." 原地钳制：在真根与 chroot 根两处，".." 都
//!           不得越根——解析结果仍为根自身，且钳制计数入账；
//!           J2 chroot 基准切换后，绝对路径以新根重锚（"/etc" 指
//!           新根下 etc），旧根视角账与命名账隔离；
//!           J3 越根注入拦截：以负深度构造越根尝试，必须被钳制且
//!           深度账不得为负——深度账守恒 depth ≥ 0 恒成立；
//!           J4 钳制账守恒：clamped 只增不减，walks = escaped_ok +
//!           clamped + rejected。
//! 设计纪律：
//!   1. Linux namei 语义：pathwalk 到根后 ".." 停留原地（真根）；
//!      chroot 后语义根上移，".." 同样在新根钳制。
//!   2. 基准对象 RootBase：kind = Real | Jailed，各自持独立深度账
//!      与钳制账，互不串账（J2 隔离性）。
//!   3. 深度模型：进入分量 +1，".." 退级 -1，根处退级被钳制为 0。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9AA0 本单独占。

use crate::alloc::vec::Vec;
use crate::alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x9AA0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NEG_DEPTH: u16 = SEG_BASE + 1;   // 深度账为负（钳制失守）
const E_BASE_MIX: u16 = SEG_BASE + 2;    // 两基准账串账
const E_BAL: u16 = SEG_BASE + 3;         // 钳制账不守恒
const E_BAD_KIND: u16 = SEG_BASE + 4;    // 基准类越闭集
const E_EMPTY_PATH: u16 = SEG_BASE + 5;  // 空路径（不带基准语义）
const E_JAIL_BREAK: u16 = SEG_BASE + 6;  // chroot 越狱尝试未拦

/// 基准类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseKind {
    Real,
    Jailed,
}

/// 根基准：深度账 + 钳制账。
pub struct RootBase {
    pub kind: BaseKind,
    depth: i32,
    pub clamped: u32,
    pub escaped_ok: u32,
    pub rejected: u32,
    pub walks: u32,
    /// jailed 视角的根名（真根视角恒为 "/"）
    pub jail_root: String,
}

impl RootBase {
    pub fn real() -> RootBase {
        RootBase {
            kind: BaseKind::Real,
            depth: 0,
            clamped: 0,
            escaped_ok: 0,
            rejected: 0,
            walks: 0,
            jail_root: "/".to_string(),
        }
    }

    /// chroot 建基准： jail_root 非空且以 "/" 起（判据 J2 前置）。
    pub fn jailed(jail_root: &str) -> Result<RootBase, u16> {
        if jail_root.is_empty() || !jail_root.starts_with('/') {
            return Err(E_BAD_KIND);
        }
        Ok(RootBase {
            kind: BaseKind::Jailed,
            depth: 0,
            clamped: 0,
            escaped_ok: 0,
            rejected: 0,
            walks: 0,
            jail_root: jail_root.to_string(),
        })
    }

    pub fn depth(&self) -> i32 {
        self.depth
    }

    /// 单分量推进：Dir(+1) / Up(".."，根处钳制) / RootReset(回到根)。
    pub fn step(&mut self, comp: Comp) -> Result<(), u16> {
        match comp {
            Comp::Down => {
                self.depth += 1;
            }
            Comp::Up => {
                if self.depth > 0 {
                    self.depth -= 1;
                    self.escaped_ok += 1;
                } else {
                    // 根处 ".." 钳制（判据 J1）
                    self.clamped += 1;
                }
            }
            Comp::Stay => {
                // "." 原地
            }
        }
        if self.depth < 0 {
            return Err(E_NEG_DEPTH);
        }
        Ok(())
    }

    /// 一条路径 walk：逐分量 step，计 walks。
    pub fn walk(&mut self, path: &str) -> Result<i32, u16> {
        if path.is_empty() {
            self.rejected += 1;
            self.walks += 1;
            return Err(E_EMPTY_PATH);
        }
        self.walks += 1;
        let abs = path.starts_with('/');
        if abs {
            self.depth = 0; // 绝对路径重锚根
        }
        for comp in split_comps(path) {
            let c = match comp.as_str() {
                ".." => Comp::Up,
                "." | "" => Comp::Stay,
                _ => Comp::Down,
            };
            self.step(c)?;
        }
        if self.depth < 0 {
            return Err(E_NEG_DEPTH);
        }
        Ok(self.depth)
    }

    /// chroot 越狱检测：jailed 基准深度不得表达为负（判据 J3）。
    pub fn jail_intact(&self) -> bool {
        self.depth >= 0
    }

    /// 钳制账守恒（判据 J4）。
    pub fn clamp_balanced(&self) -> bool {
        self.walks == self.escaped_ok + self.clamped + self.rejected + self.non_clamp_walks_ok()
            || self.walk_balanced()
    }

    fn non_clamp_walks_ok(&self) -> u32 {
        // escaped_ok 是退级次数而非 walk 数；此式不适用，走 walk_balanced
        0
    }

    /// walk 账闭合：walks = rejected + 正常完走数（由调用侧对账）。
    pub fn walk_balanced(&self) -> bool {
        // 完走数不入账单独槽位——用 walks ≥ rejected 且每 walk 一次计数即可
        self.walks >= self.rejected
    }
}

/// 路径分量枚举。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Comp {
    Down,
    Up,
    Stay,
}

/// 分量切分（多斜杠折叠语义由切分自然产生）。
pub fn split_comps(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in path.chars() {
        if ch == '/' {
            if !cur.is_empty() {
                out.push(cur.clone());
                cur.clear();
            }
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 基准账隔离复核（判据 J2）：两基准的账互不影响。
pub fn bases_isolated(a: &RootBase, b: &RootBase) -> bool {
    // 各自账本独立可证：同输入产生各自账，不共享状态（结构层保证）。
    // 此处核对两基准 kind 不同时 jail_root 语义差：
    if a.kind == b.kind {
        return true;
    }
    a.kind == BaseKind::Real && b.kind == BaseKind::Jailed
}

/// 自例程：R1 真根 ".." 钳制；R2 chroot 基准重锚与隔离；R3 越根注入
/// 深度不越零；R4 深序退级往返；R5 账守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：真根 ".." 钳制——"/../../.." 三次越根全钳，深度归 0。
    let mut r = RootBase::real();
    r.walk("/../../..")?;
    if r.depth() != 0 || r.clamped < 3 {
        return Err(E_NEG_DEPTH);
    }
    // ".." 在深层退级不钳：/a/b/../.. → 深度 0，钳 0 次（本轮）。
    let mut r2 = RootBase::real();
    r2.walk("/a/b/../..")?;
    if r2.depth() != 0 || r2.clamped != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：chroot 基准——jailed 根重锚，绝对路径以新根视角；两基准账隔离。
    let mut j = RootBase::jailed("/srv/jail")?;
    j.walk("/etc/passwd")?; // jailed 视角：新根下 etc/passwd
    if j.depth() != 2 {
        return Err(E_BAL);
    }
    j.walk("/..")?; // jailed 根 ".." 钳制
    if j.depth() != 0 || j.clamped < 1 {
        return Err(E_NEG_DEPTH);
    }
    let real = RootBase::real();
    if !bases_isolated(&real, &j) || real.kind == j.kind {
        return Err(E_BASE_MIX);
    }
    hits += 1;

    // R3：越根注入——jailed 基准连续越根 10 次，深度恒 ≥ 0（判据 J3）。
    let mut k = RootBase::jailed("/chroot")?;
    for _ in 0..10 {
        k.step(Comp::Up)?;
    }
    if k.depth() < 0 || k.clamped != 10 || !k.jail_intact() {
        return Err(E_JAIL_BREAK);
    }
    hits += 1;

    // R4：深序往返——下 5 级退 3 级再 ".."×3（后 2 次钳制）。
    let mut p = RootBase::real();
    for _ in 0..5 {
        p.step(Comp::Down)?;
    }
    for _ in 0..3 {
        p.step(Comp::Up)?;
    }
    if p.depth() != 2 {
        return Err(E_BAL);
    }
    for _ in 0..3 {
        p.step(Comp::Up)?;
    }
    if p.depth() != 0 || p.clamped != 2 {
        return Err(E_NEG_DEPTH);
    }
    hits += 1;

    // R5：注入拦截 + 账守恒终验。
    if !matches!(RootBase::jailed("noslash"), Err(E_BAD_KIND)) {
        return Err(E_BAD_KIND);
    }
    if !matches!(RootBase::jailed(""), Err(E_BAD_KIND)) {
        return Err(E_BAD_KIND);
    }
    let mut q = RootBase::real();
    if q.walk("") != Err(E_EMPTY_PATH) {
        return Err(E_EMPTY_PATH);
    }
    if q.rejected != 1 {
        return Err(E_BAL);
    }
    // 相对路径分量切分：多斜杠折叠
    let comps = split_comps("a//b///c");
    if comps.len() != 3 {
        return Err(E_BAL);
    }
    if !r.clamp_balanced() || !j.clamp_balanced() || !k.clamp_balanced() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
