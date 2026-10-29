//! UNX-F11289 诊断输出兼容矩阵（0x1fbd0 段）
//!
//! 纯功能模型：诊断工具×场景矩阵共 4 工具（ldd / ld.so --list / readelf /
//! objdump）× 6 场景（缺库 / 版本缺失 / 静态件 / PIE 动态件 / 坏 so / 空环境）
//! = 24 格。每格登记该工具在该场景下实际打出的「关键行码」集合与退出码；
//! 期望侧是一张 24 项常量表，一致率按千分比记账，只有 24 格全登记且一致率
//! 1000‰ 才允许封盘。
//!
//! 规则面：关键行码取闭集 12 个（vdso / libc / not found / statically linked /
//! GLIBC 版本缺失 / invalid ELF / ET_DYN / ET_EXEC / NEEDED / VERNEED /
//! exit 127 / 空环境标记）；一码一格只许登记一次；trace 类工具（ldd 与
//! --list）不得声明「执行了目标件」——它们只装载依赖不执行；期望与实际的
//! 差异不静默，逐格立账（差异号 + 首个偏差行码），但不直接判败，由一致率
//! 在封盘时统一裁定。
//!
//! 拒止面：DX_ETOOL（工具号越界）、DX_ESCEN（场景号越界）、DX_EBADLINE
//! （关键行码不在闭集）、DX_EDUP（同格重复登记）、DX_EEXEC（trace 工具声称
//! 执行了目标件）、DX_EFROZEN（封盘后登记）、DX_EMISS（封盘时缺格）、
//! DX_EMISMATCH（封盘时一致率不足 1000‰）、DX_ECLOSED（二次封盘）、
//! DX_EEMPTY（空矩阵封盘前的空账查询）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x1fbd0 独占 0x10）。
pub const SEG_BASE: u32 = 0x1fbd0;

/// 工具号越界
pub const DX_ETOOL: u32 = SEG_BASE + 0x01;
/// 场景号越界
pub const DX_ESCEN: u32 = SEG_BASE + 0x02;
/// 关键行码不在闭集
pub const DX_EBADLINE: u32 = SEG_BASE + 0x03;
/// 同格重复登记
pub const DX_EDUP: u32 = SEG_BASE + 0x04;
/// trace 工具声称执行了目标件
pub const DX_EEXEC: u32 = SEG_BASE + 0x05;
/// 封盘后登记
pub const DX_EFROZEN: u32 = SEG_BASE + 0x06;
/// 封盘时缺格
pub const DX_EMISS: u32 = SEG_BASE + 0x07;
/// 封盘时一致率不足
pub const DX_EMISMATCH: u32 = SEG_BASE + 0x08;
/// 二次封盘
pub const DX_ECLOSED: u32 = SEG_BASE + 0x09;
/// 空矩阵
pub const DX_EEMPTY: u32 = SEG_BASE + 0x0a;

/// 工具数（ldd / --list / readelf / objdump）。
pub const TOOLS: usize = 4;
/// 场景数（缺库/版本/静态/PIE/坏so/空环境）。
pub const SCENES: usize = 6;
/// 矩阵格数。
pub const CELLS: usize = TOOLS * SCENES;
/// 单格关键行上限。
pub const MAX_LINES: usize = 6;
/// 关键行码上界（闭集 1..=12）。
pub const LINE_MAX: u16 = 12;
/// 一致率满值（千分比）。
pub const PERMILLE_FULL: u32 = 1000;

/// 工具号：ldd。
pub const T_LDD: u8 = 0;
/// 工具号：ld.so --list 直调。
pub const T_LIST: u8 = 1;
/// 工具号：readelf。
pub const T_READELF: u8 = 2;
/// 工具号：objdump。
pub const T_OBJDUMP: u8 = 3;

/// 场景号：缺库。
pub const S_MISSING: u8 = 0;
/// 场景号：GLIBC 版本缺失。
pub const S_VERMISS: u8 = 1;
/// 场景号：静态件。
pub const S_STATIC: u8 = 2;
/// 场景号：PIE 动态件。
pub const S_PIE: u8 = 3;
/// 场景号：坏 so。
pub const S_BADSO: u8 = 4;
/// 场景号：空环境。
pub const S_EMPTYENV: u8 = 5;

/// 关键行码：linux-vdso.so.1 行。
pub const K_VDSO: u16 = 1;
/// 关键行码：libc.so.6 => 解析行。
pub const K_LIBC: u16 = 2;
/// 关键行码：not found。
pub const K_NOTFOUND: u16 = 3;
/// 关键行码：statically linked。
pub const K_STATIC: u16 = 4;
/// 关键行码：version `GLIBC_2.xx' not found。
pub const K_VERMISS: u16 = 5;
/// 关键行码：invalid ELF header。
pub const K_BADELF: u16 = 6;
/// 关键行码：ET_DYN。
pub const K_ETDYN: u16 = 7;
/// 关键行码：ET_EXEC。
pub const K_EXEC: u16 = 8;
/// 关键行码：(NEEDED) 行。
pub const K_NEEDED: u16 = 9;
/// 关键行码：(VERNEED) 版本表行。
pub const K_VERSION: u16 = 10;
/// 关键行码：exit 127。
pub const K_EXIT127: u16 = 11;
/// 关键行码：空环境（environ 只有三项）。
pub const K_ENVEMPTY: u16 = 12;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 期望关键行码集合（场景主序：scene*TOOLS + tool）。
///
/// 每格为 12 位行码位图；位序与 K_* 常量一一对应（K_VDSO 为第 0 位）。
pub const EXPECT: [u32; CELLS] = [
    // 场景 0 缺库：ldd/list 报 not found，读工具只见 NEEDED
    0x107, 0x407, 0x100, 0x100,
    // 场景 1 版本缺失：ldd/list 报版本缺失，读工具可见 VERNEED
    0x113, 0x413, 0x300, 0x300,
    // 场景 2 静态件：ldd/list 单行 statically linked，读工具只见 ET_EXEC
    0x008, 0x008, 0x080, 0x080,
    // 场景 3 PIE：ldd/list 走 vdso+libc，读工具见 ET_DYN
    0x043, 0x043, 0x140, 0x040,
    // 场景 4 坏 so：一律 invalid ELF，ldd 另报 not found
    0x024, 0x020, 0x020, 0x020,
    // 场景 5 空环境：解析工具带空环境标记，读工具只读 NEEDED
    0x803, 0x803, 0x100, 0x100,
];

/// 期望退出码（场景主序）：--list 在缺库/版本/坏 so 三景退 127。
pub const EXPECT_EXIT: [u8; CELLS] = [
    0, 127, 0, 0,
    0, 127, 0, 0,
    0, 0, 0, 0,
    0, 0, 0, 0,
    0, 127, 0, 0,
    0, 0, 0, 0,
];

/// 一格登记：工具 / 场景 / 行码位图 / 退出码 / 是否执行 / 是否一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub tool: u8,
    pub scene: u8,
    pub mask: u32,
    pub exit: u8,
    pub executed: bool,
    pub matched: bool,
}

/// 诊断矩阵账。
#[derive(Clone, Debug)]
pub struct DiagMatrix {
    cells: [Option<Cell>; CELLS],
    filled: usize,
    matched: usize,
    diffs: u32,
    first_bad: u16,
    closed: bool,
    root: Option<u64>,
    pub accepts: u64,
    pub rejects: u64,
    pub exec_claims: u64,
}

impl DiagMatrix {
    /// 空矩阵。
    pub fn new() -> DiagMatrix {
        DiagMatrix {
            cells: [None; CELLS],
            filled: 0,
            matched: 0,
            diffs: 0,
            first_bad: 0,
            closed: false,
            root: None,
            accepts: 0,
            rejects: 0,
            exec_claims: 0,
        }
    }

    fn note(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已登记格数。
    pub fn filled(&self) -> usize {
        self.filled
    }

    /// 一致格数。
    pub fn matched(&self) -> usize {
        self.matched
    }

    /// 差异累计。
    pub fn diffs(&self) -> u32 {
        self.diffs
    }

    /// 首个偏差行码（无偏差为 0）。
    pub fn first_bad(&self) -> u16 {
        self.first_bad
    }

    /// 一致率（千分比，四舍五入到整数格）。
    pub fn permille(&self) -> u32 {
        ((self.matched as u32).wrapping_mul(PERMILLE_FULL)) / (CELLS as u32)
    }

    /// 取一格。
    pub fn cell(&self, scene: u8, tool: u8) -> Option<Cell> {
        let idx = (scene as usize).wrapping_mul(TOOLS).wrapping_add(tool as usize);
        match self.cells.get(idx) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 行码 → 位。
    fn bit(code: u16) -> u32 {
        if code == 0 || code > LINE_MAX {
            return 0;
        }
        1u32 << (code.wrapping_sub(1))
    }

    /// 登记一格。校验序：封盘 → 工具 → 场景 → 执行声明 → 行码 → 重格。
    pub fn submit(
        &mut self,
        tool: u8,
        scene: u8,
        tokens: &[u16],
        exit: u8,
        executed: bool,
    ) -> Result<u64, u32> {
        if self.closed {
            return Err(self.note(DX_EFROZEN));
        }
        if tool as usize >= TOOLS {
            return Err(self.note(DX_ETOOL));
        }
        if scene as usize >= SCENES {
            return Err(self.note(DX_ESCEN));
        }
        if executed && (tool == T_LDD || tool == T_LIST) {
            self.exec_claims = self.exec_claims.wrapping_add(1);
            return Err(self.note(DX_EEXEC));
        }
        if tokens.is_empty() || tokens.len() > MAX_LINES {
            return Err(self.note(DX_EBADLINE));
        }
        let mut mask = 0u32;
        let mut i = 0usize;
        while i < tokens.len() {
            match tokens.get(i) {
                Some(t) => {
                    if *t == 0 || *t > LINE_MAX {
                        return Err(self.note(DX_EBADLINE));
                    }
                    mask |= Self::bit(*t);
                }
                None => break,
            }
            i = i.wrapping_add(1);
        }
        let idx = (scene as usize).wrapping_mul(TOOLS).wrapping_add(tool as usize);
        match self.cells.get(idx) {
            Some(Some(_)) => return Err(self.note(DX_EDUP)),
            Some(None) => {}
            None => return Err(self.note(DX_ETOOL)),
        }
        let want_mask = match EXPECT.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        let want_exit = match EXPECT_EXIT.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        let matched = mask == want_mask && exit == want_exit;
        if !matched {
            self.diffs = self.diffs.wrapping_add(1);
            if self.first_bad == 0 {
                // 首个偏差行：实际有而期望无的行优先，否则取期望有而实际无的首行。
                let extra = mask & !want_mask;
                let missing = want_mask & !mask;
                let mut code = 0u16;
                let mut k = 1u16;
                while k <= LINE_MAX {
                    let b = Self::bit(k);
                    if extra & b != 0 {
                        code = k;
                        break;
                    }
                    if missing & b != 0 && code == 0 {
                        code = k;
                    }
                    k = k.wrapping_add(1);
                }
                self.first_bad = code;
            }
        }
        let cell = Cell { tool, scene, mask, exit, executed, matched };
        match self.cells.get_mut(idx) {
            Some(slot) => *slot = Some(cell),
            None => return Err(self.note(DX_ETOOL)),
        }
        self.filled = self.filled.wrapping_add(1);
        if matched {
            self.matched = self.matched.wrapping_add(1);
        }
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.digest())
    }

    /// 矩阵指纹（FNV-1a，按格序）。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < CELLS {
            match self.cells.get(i) {
                Some(Some(c)) => {
                    h = (h ^ c.tool as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ c.scene as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ c.mask as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ c.exit as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ c.executed as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ c.matched as u64).wrapping_mul(FNV_PRIME);
                }
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        h = (h ^ self.filled as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.matched as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 逐工具一致格数。
    pub fn tool_total(&self, tool: u8) -> usize {
        let mut n = 0usize;
        let mut sc = 0u8;
        while (sc as usize) < SCENES {
            if let Some(c) = self.cell(sc, tool) {
                if c.matched {
                    n = n.wrapping_add(1);
                }
            }
            sc = sc.wrapping_add(1);
        }
        n
    }

    /// 逐场景一致格数。
    pub fn scene_total(&self, scene: u8) -> usize {
        let mut n = 0usize;
        let mut t = 0u8;
        while (t as usize) < TOOLS {
            if let Some(c) = self.cell(scene, t) {
                if c.matched {
                    n = n.wrapping_add(1);
                }
            }
            t = t.wrapping_add(1);
        }
        n
    }

    /// 声明执行过目标件的格数（trace 语义要求恒为 0）。
    pub fn executed_cells(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CELLS {
            match self.cells.get(i) {
                Some(Some(c)) => {
                    if c.executed {
                        n = n.wrapping_add(1);
                    }
                }
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 封盘：满格 + 一致率 1000‰，缺一即拒。
    pub fn close(&mut self) -> Result<u64, u32> {
        if self.closed {
            return Err(self.note(DX_ECLOSED));
        }
        if self.filled == 0 {
            return Err(self.note(DX_EEMPTY));
        }
        if self.filled != CELLS {
            return Err(self.note(DX_EMISS));
        }
        if self.matched != CELLS || self.permille() != PERMILLE_FULL {
            return Err(self.note(DX_EMISMATCH));
        }
        let v = self.digest();
        self.root = Some(v);
        self.closed = true;
        Ok(v)
    }

    /// 封盘幂等（未封则先封）。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.closed {
            return match self.root {
                Some(r) => Ok(r),
                None => Ok(self.digest()),
            };
        }
        self.close()
    }

    /// 定锚值。
    pub fn root_value(&self) -> Option<u64> {
        self.root
    }

    /// 重放：重算矩阵指纹并与定锚比对。
    pub fn replay(&mut self) -> Result<u64, u32> {
        let root = match self.root {
            Some(r) => r,
            None => return Err(self.note(DX_EMISMATCH)),
        };
        if self.digest() != root {
            return Err(self.note(DX_EMISMATCH));
        }
        Ok(root)
    }
}

/// 每格实际打出的关键行（工具×场景），模拟基准机渲染结果。
pub fn tokens_of(tool: u8, scene: u8) -> ([u16; MAX_LINES], usize) {
    match (scene, tool) {
        (S_MISSING, T_LDD) => ([K_VDSO, K_LIBC, K_NOTFOUND, K_NEEDED, 0, 0], 4),
        (S_MISSING, T_LIST) => ([K_VDSO, K_LIBC, K_NOTFOUND, K_EXIT127, 0, 0], 4),
        (S_MISSING, T_READELF) => ([K_NEEDED, 0, 0, 0, 0, 0], 1),
        (S_MISSING, T_OBJDUMP) => ([K_NEEDED, 0, 0, 0, 0, 0], 1),
        (S_VERMISS, T_LDD) => ([K_VDSO, K_LIBC, K_VERMISS, K_NEEDED, 0, 0], 4),
        (S_VERMISS, T_LIST) => ([K_VDSO, K_LIBC, K_VERMISS, K_EXIT127, 0, 0], 4),
        (S_VERMISS, T_READELF) => ([K_NEEDED, K_VERSION, 0, 0, 0, 0], 2),
        (S_VERMISS, T_OBJDUMP) => ([K_NEEDED, K_VERSION, 0, 0, 0, 0], 2),
        (S_STATIC, T_LDD) => ([K_STATIC, 0, 0, 0, 0, 0], 1),
        (S_STATIC, T_LIST) => ([K_STATIC, 0, 0, 0, 0, 0], 1),
        (S_STATIC, T_READELF) => ([K_EXEC, 0, 0, 0, 0, 0], 1),
        (S_STATIC, T_OBJDUMP) => ([K_EXEC, 0, 0, 0, 0, 0], 1),
        (S_PIE, T_LDD) => ([K_VDSO, K_LIBC, K_ETDYN, 0, 0, 0], 3),
        (S_PIE, T_LIST) => ([K_VDSO, K_LIBC, K_ETDYN, 0, 0, 0], 3),
        (S_PIE, T_READELF) => ([K_ETDYN, K_NEEDED, 0, 0, 0, 0], 2),
        (S_PIE, T_OBJDUMP) => ([K_ETDYN, 0, 0, 0, 0, 0], 1),
        (S_BADSO, T_LDD) => ([K_BADELF, K_NOTFOUND, 0, 0, 0, 0], 2),
        (S_BADSO, T_LIST) => ([K_BADELF, 0, 0, 0, 0, 0], 1),
        (S_BADSO, T_READELF) => ([K_BADELF, 0, 0, 0, 0, 0], 1),
        (S_BADSO, T_OBJDUMP) => ([K_BADELF, 0, 0, 0, 0, 0], 1),
        (S_EMPTYENV, T_LDD) => ([K_VDSO, K_LIBC, K_ENVEMPTY, 0, 0, 0], 3),
        (S_EMPTYENV, T_LIST) => ([K_VDSO, K_LIBC, K_ENVEMPTY, 0, 0, 0], 3),
        (S_EMPTYENV, T_READELF) => ([K_NEEDED, 0, 0, 0, 0, 0], 1),
        (S_EMPTYENV, T_OBJDUMP) => ([K_NEEDED, 0, 0, 0, 0, 0], 1),
        _ => ([0u16; MAX_LINES], 0),
    }
}

/// 建满 24 格（期望值一致）。
pub fn build_matrix() -> (DiagMatrix, bool) {
    let mut m = DiagMatrix::new();
    let mut scene = 0u8;
    while (scene as usize) < SCENES {
        let mut tool = 0u8;
        while (tool as usize) < TOOLS {
            let (toks, n) = tokens_of(tool, scene);
            if n == 0 || n > MAX_LINES {
                return (m, false);
            }
            let idx = (scene as usize).wrapping_mul(TOOLS).wrapping_add(tool as usize);
            let exit = match EXPECT_EXIT.get(idx) {
                Some(v) => *v,
                None => 0,
            };
            if m.submit(tool, scene, &toks[..n], exit, false).is_err() {
                return (m, false);
            }
            tool = tool.wrapping_add(1);
        }
        scene = scene.wrapping_add(1);
    }
    let good = m.filled() == CELLS && m.matched() == CELLS;
    (m, good)
}

/// 填格（可指定一格用错行），用于差异立账与缺格封盘两类注入。
fn fill_upto(m: &mut DiagMatrix, stop: usize, break_idx: usize) -> bool {
    let mut idx = 0usize;
    while idx < stop {
        let tool = (idx % TOOLS) as u8;
        let scene = (idx / TOOLS) as u8;
        let (toks, n) = tokens_of(tool, scene);
        let ok = if idx == break_idx {
            m.submit(tool, scene, &[K_STATIC], 0, false).is_ok()
        } else {
            let exit = match EXPECT_EXIT.get(idx) {
                Some(v) => *v,
                None => 0,
            };
            m.submit(tool, scene, &toks[..n], exit, false).is_ok()
        };
        if !ok {
            return false;
        }
        idx = idx.wrapping_add(1);
    }
    true
}

/// 四类注入：号越界 / trace 执行声明 / 行码与重格 / 缺格与差异封盘。
pub fn inject_four() -> [bool; 4] {
    // ① 工具号与场景号越界
    let mut a0 = DiagMatrix::new();
    let r0 = a0.submit(TOOLS as u8, 0, &[K_VDSO], 0, false).err() == Some(DX_ETOOL)
        && a0.submit(0, SCENES as u8, &[K_VDSO], 0, false).err() == Some(DX_ESCEN);

    // ② trace 工具执行声明被拒（ldd / --list），readelf 声明执行合法
    let mut a1 = DiagMatrix::new();
    let r1 = a1.submit(T_LDD, 0, &[K_VDSO], 0, true).err() == Some(DX_EEXEC)
        && a1.submit(T_LIST, 0, &[K_VDSO], 0, true).err() == Some(DX_EEXEC)
        && a1.submit(T_READELF, 0, &[K_NEEDED], 0, true).is_ok()
        && a1.exec_claims == 2;

    // ③ 行码越界 / 空行集 / 同格重登
    let mut a2 = DiagMatrix::new();
    let r2 = a2.submit(0, 0, &[LINE_MAX + 1], 0, false).err() == Some(DX_EBADLINE)
        && a2.submit(0, 0, &[], 0, false).err() == Some(DX_EBADLINE)
        && a2.submit(0, 0, &[K_VDSO], 0, false).is_ok()
        && a2.submit(0, 0, &[K_VDSO], 0, false).err() == Some(DX_EDUP);

    // ④ 缺格封盘拒 / 差异封盘拒（差异立账后一致率不足）
    let mut a3 = DiagMatrix::new();
    let short = fill_upto(&mut a3, CELLS - 1, usize::MAX);
    let miss = a3.close().err() == Some(DX_EMISS);
    let mut a4 = DiagMatrix::new();
    let full = fill_upto(&mut a4, CELLS, 7);
    let bad = a4.close().err() == Some(DX_EMISMATCH)
        && a4.diffs() == 1
        && a4.matched() == CELLS - 1
        && a4.first_bad() != 0;
    let r3 = short && miss && full && bad;

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11289 诊断输出兼容矩阵（4 工具×6 场景）。
pub fn run_unx11289_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11289.diag_matrix");
    let (mut m, ok) = build_matrix();

    // DX1 满格登记
    s.add_dyn(
        "DX1-满格登记",
        ok && m.filled() == CELLS && m.accepts == CELLS as u64 && m.rejects == 0,
        &format!("fill={} acc={} rej={}", m.filled(), m.accepts, m.rejects),
    );

    // DX2 一致率 1000‰
    s.add_dyn(
        "DX2-一致率千分",
        m.permille() == PERMILLE_FULL && m.matched() == CELLS,
        &format!("pm={} matched={}", m.permille(), m.matched()),
    );

    // DX3 逐工具 6/6
    let t0 = m.tool_total(T_LDD);
    let t1 = m.tool_total(T_LIST);
    let t2 = m.tool_total(T_READELF);
    let t3 = m.tool_total(T_OBJDUMP);
    s.add_dyn(
        "DX3-逐工具合计",
        t0 == SCENES && t1 == SCENES && t2 == SCENES && t3 == SCENES,
        &format!("ldd={} list={} readelf={} objdump={}", t0, t1, t2, t3),
    );

    // DX4 逐场景 4/4
    let mut scenes_ok = true;
    let mut sc = 0u8;
    while (sc as usize) < SCENES {
        if m.scene_total(sc) != TOOLS {
            scenes_ok = false;
        }
        sc = sc.wrapping_add(1);
    }
    s.add("DX4-逐场景合计", scenes_ok, "六场景各 4 格全一致");

    // DX5 trace 零执行
    s.add(
        "DX5-trace零执行",
        m.executed_cells() == 0 && m.exec_claims == 0,
        "24 格无一格声明执行目标件",
    );

    // DX6 缺库场景关键行
    let c0 = m.cell(S_MISSING, T_LDD);
    let c1 = m.cell(S_MISSING, T_LIST);
    s.add_dyn(
        "DX6-缺库关键行",
        c0.map(|c| c.mask) == Some(0x107)
            && c1.map(|c| c.exit) == Some(127)
            && c1.map(|c| c.mask) == Some(0x407),
        &format!("ldd={:#x} list={:#x}", c0.map(|c| c.mask).unwrap_or(0),
                 c1.map(|c| c.mask).unwrap_or(0)),
    );

    // DX7 静态件单行
    let s0 = m.cell(S_STATIC, T_LDD);
    let s2 = m.cell(S_STATIC, T_READELF);
    s.add_dyn(
        "DX7-静态件单行",
        s0.map(|c| c.mask) == Some(0x008) && s2.map(|c| c.mask) == Some(0x080),
        &format!("ldd={:#x} readelf={:#x}", s0.map(|c| c.mask).unwrap_or(0),
                 s2.map(|c| c.mask).unwrap_or(0)),
    );

    // DX8 坏 so 错误行
    let b0 = m.cell(S_BADSO, T_LDD);
    let b1 = m.cell(S_BADSO, T_LIST);
    s.add_dyn(
        "DX8-坏so错误行",
        b0.map(|c| c.mask) == Some(0x024) && b1.map(|c| c.mask) == Some(0x020),
        &format!("ldd={:#x} list={:#x}", b0.map(|c| c.mask).unwrap_or(0),
                 b1.map(|c| c.mask).unwrap_or(0)),
    );

    // DX9 空环境场景
    let e0 = m.cell(S_EMPTYENV, T_LDD);
    let e2 = m.cell(S_EMPTYENV, T_READELF);
    s.add(
        "DX9-空环境场景",
        e0.map(|c| c.mask) == Some(0x803) && e2.map(|c| c.mask) == Some(0x100),
        "ldd 带空环境标记，readelf 只读 NEEDED",
    );

    // DX10 差异立账（不静默）
    let mut d = DiagMatrix::new();
    let filled = fill_upto(&mut d, CELLS, 7);
    s.add_dyn(
        "DX10-差异立账",
        filled && d.diffs() == 1 && d.matched() == CELLS - 1
            && d.first_bad() == K_STATIC && d.close().err() == Some(DX_EMISMATCH),
        &format!("diffs={} first={} match={}", d.diffs(), d.first_bad(), d.matched()),
    );

    // DX11 缺格与空账封盘拒
    let mut q = DiagMatrix::new();
    let empty = q.close().err() == Some(DX_EEMPTY);
    let mut q2 = DiagMatrix::new();
    let short_fill = fill_upto(&mut q2, CELLS - 1, usize::MAX);
    s.add(
        "DX11-缺格封盘拒",
        empty && short_fill && q2.close().err() == Some(DX_EMISS) && q2.filled() == CELLS - 1,
        "空账与 23 格两路皆拒封盘",
    );

    // DX12 封盘幂等 + 重放
    let a = m.freeze().unwrap_or(0);
    let b = m.freeze().unwrap_or(0);
    let rep = m.replay().unwrap_or(0);
    s.add_dyn(
        "DX12-封盘幂等与重放",
        a == b && a != 0 && rep == a && m.root_value() == Some(a),
        &format!("a={:#x} b={:#x} rep={:#x}", a, b, rep),
    );

    // DX13 注入四拒
    let hits = inject_four();
    s.add(
        "DX13-注入四拒",
        hits[0] && hits[1] && hits[2] && hits[3],
        "号越界/trace执行/行码重格/缺格差异四路皆拒",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11289_matrix_flow() {
        let (mut m, ok) = build_matrix();
        assert!(ok);
        assert_eq!(m.filled(), CELLS);
        assert_eq!(m.matched(), CELLS);
        assert_eq!(m.permille(), 1000);
        assert_eq!(m.cell(S_MISSING, T_LIST).map(|c| c.exit), Some(127));
        assert_eq!(m.cell(S_STATIC, T_LDD).map(|c| c.mask), Some(0x008));
        let root = m.freeze().ok();
        assert!(root.is_some());
        assert_eq!(m.freeze().ok(), root);
        assert_eq!(m.replay().ok(), root);
    }

    #[test]
    fn f11289_matrix_gates() {
        let mut m = DiagMatrix::new();
        assert_eq!(m.close(), Err(DX_EEMPTY));
        assert_eq!(m.submit(TOOLS as u8, 0, &[K_VDSO], 0, false), Err(DX_ETOOL));
        assert_eq!(m.submit(0, SCENES as u8, &[K_VDSO], 0, false), Err(DX_ESCEN));
        assert_eq!(m.submit(0, 0, &[0], 0, false), Err(DX_EBADLINE));
        assert_eq!(m.submit(0, 0, &[LINE_MAX + 1], 0, false), Err(DX_EBADLINE));
        assert_eq!(m.submit(0, 0, &[], 0, false), Err(DX_EBADLINE));
        assert_eq!(m.submit(T_LDD, 0, &[K_VDSO], 0, true), Err(DX_EEXEC));
        assert_eq!(m.rejects, 7);
        assert!(m.submit(0, 0, &[K_VDSO, K_LIBC, K_NOTFOUND, K_NEEDED], 0, false).is_ok());
        assert_eq!(m.submit(0, 0, &[K_VDSO], 0, false), Err(DX_EDUP));
        assert_eq!(m.filled(), 1);
        assert_eq!(m.matched(), 1);
        assert_eq!(m.rejects, 8);
        assert_eq!(m.close(), Err(DX_EMISS));
    }

    #[test]
    fn f11289_matrix_rules() {
        // 差异不静默：错行仍可登记，但一致率归零并记首个偏差
        let mut d = DiagMatrix::new();
        assert!(fill_upto(&mut d, CELLS, 7));
        assert_eq!(d.diffs(), 1);
        assert_eq!(d.matched(), CELLS - 1);
        assert_eq!(d.permille(), 958);
        assert_eq!(d.first_bad(), K_STATIC);
        assert_eq!(d.close(), Err(DX_EMISMATCH));
        // 满格一致才可封盘
        let (mut g, _) = build_matrix();
        assert_eq!(g.close(), Ok(g.digest()));
        assert_eq!(g.close(), Err(DX_ECLOSED));
        assert_eq!(g.submit(0, 0, &[K_VDSO], 0, false), Err(DX_EFROZEN));
        // 工具聚合
        assert_eq!(g.tool_total(T_LDD), SCENES);
        assert_eq!(g.scene_total(S_BADSO), TOOLS);
        assert_eq!(g.executed_cells(), 0);
    }

    #[test]
    fn f11289_matrix_injection() {
        let hits = inject_four();
        assert!(hits[0] && hits[1] && hits[2] && hits[3]);
    }

    #[test]
    fn f11289_checks_green() {
        let cs = run_unx11289_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
    }
}
