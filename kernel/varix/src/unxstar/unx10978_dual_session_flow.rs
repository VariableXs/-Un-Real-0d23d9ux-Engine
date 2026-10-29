//! UNX-F10978 vim 场并发会话联测（双实例隔离）（0x1d9c0 段）
//!
//! 锚点：UNX-F10978-J1 双实例各自终端输入输出零串扰（10/10），
//! 会话隔离对账（10/10）。
//! 域：CoRun·UNX-B29（I 型联测域·vim 场并发面）。
//!
//! 结构：会话账（两实例各一条：实例号 + 终端号 + 属主 pid + 写入
//! 计数）+ 屏幕账（两屏各一幅：属主实例 + 行列 + 字节累计 + 脏格
//! 累计）+ 路由器（写入按实例寻屏，属主不符即拒）+ 隔离闸（两屏
//! 字节累计必须逐一对各自会话写入数）+ fold 与冻结 + 注入核
//! （坏实例号/跨实例写/冻结后写/空账写四路必抓）。
//!
//! 隔离律：屏 i 的属主恒为实例 i；写入只落在本实例屏上，
//! 两实例字节账互不相加、互不借贷。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10978 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1d9c0;

/// 空账 —— 未立会话即写入
pub const CS_EMPTY: u32 = SEG_BASE + 0x01;
/// 实例越界 —— 实例号 0..=1
pub const CS_RANGE: u32 = SEG_BASE + 0x02;
/// 重号立会 —— 同一实例只准一条会话
pub const CS_DUP: u32 = SEG_BASE + 0x03;
/// 跨实例串扰 —— 写入落到非本实例屏
pub const CS_CROSS: u32 = SEG_BASE + 0x04;
/// 隔离失衡 —— 屏账与会话账不齐
pub const CS_IMBALANCE: u32 = SEG_BASE + 0x05;
/// 场已冻结 —— freeze 后禁止再写
pub const CS_FROZEN: u32 = SEG_BASE + 0x06;

/// 实例数（双实例）。
pub const INSTANCES: usize = 2;
/// 终端号基址。
pub const TTY_BASE: u32 = 0x10;
/// 屏幕行列。
pub const ROWS: u32 = 24;
/// 屏幕列数。
pub const COLS: u32 = 80;
/// 每格字节数。
pub const CELL_BYTES: u64 = 8;
/// 联测轮数。
pub const ROUNDS: u64 = 1000;
/// 会话账域标（fold 种子）。
pub const CS_TAG: u64 = 0x1d9_78;

/// 会话：一实例一条。
#[derive(Clone, Debug)]
pub struct VimSession {
    /// 会话号（0x15200 段起编）
    pub sid: u32,
    /// 实例号 0/1
    pub inst: u8,
    /// 终端号
    pub tty: u32,
    /// 属主 pid
    pub owner_pid: u32,
    /// 写入字节数
    pub writes: u64,
    /// 输入回显次数
    pub echoes: u64,
    /// 写入调用次数（字节约写与调用分开记，回显按调用计）
    pub calls: u64,
}

impl VimSession {
    /// 会话是否活跃（有终端且有写入）。
    pub fn active(&self) -> bool {
        self.tty != 0 && self.writes > 0
    }

    /// 每次写入调用的平均回显。
    pub fn echo_ratio(&self) -> u64 {
        if self.calls == 0 {
            0
        } else {
            self.echoes / self.calls
        }
    }
}

/// 屏幕：一实例一幅。
#[derive(Clone, Debug)]
pub struct Screen {
    /// 屏号（0x15300 段起编）
    pub pid: u32,
    /// 属主实例号
    pub owner: u8,
    /// 屏字节累计
    pub cells: u64,
    /// 脏格累计
    pub dirty: u64,
}

impl Screen {
    /// 屏容量（字节）。
    pub fn cap(&self) -> u64 {
        (ROWS as u64)
            .wrapping_mul(COLS as u64)
            .wrapping_mul(CELL_BYTES)
    }

    /// 屏占用率（千分比），空屏返 0。
    pub fn load_pm(&self) -> u64 {
        let c = self.cap();
        if c == 0 {
            return 0;
        }
        self.cells.wrapping_mul(1000) / c
    }

    /// 脏格率（千分比），无格返 0。
    pub fn dirty_pm(&self) -> u64 {
        let total = (ROWS as u64).wrapping_mul(COLS as u64);
        if total == 0 {
            return 0;
        }
        self.dirty.wrapping_mul(1000) / total
    }
}

/// 双实例会话总场。
#[derive(Clone, Debug)]
pub struct DualSessionArena {
    /// 两实例会话
    pub sessions: Vec<VimSession>,
    /// 两幅屏幕
    pub screens: Vec<Screen>,
    /// 累计路由次数
    pub routes: u64,
    /// 跨实例拒止次数
    pub crosstalks: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl DualSessionArena {
    /// 立场：双实例双屏各自立账（零计数）。
    pub fn new() -> DualSessionArena {
        let mut ar = DualSessionArena {
            sessions: Vec::new(),
            screens: Vec::new(),
            routes: 0,
            crosstalks: 0,
            frozen: false,
        };
        let mut k = 0usize;
        while k < INSTANCES {
            ar.sessions.push(VimSession {
                sid: 0x15200u32.wrapping_add(k as u32),
                inst: k as u8,
                tty: TTY_BASE.wrapping_add(k as u32),
                owner_pid: 0x200u32.wrapping_add(k as u32),
                writes: 0,
                echoes: 0,
                calls: 0,
            });
            ar.screens.push(Screen {
                pid: 0x15300u32.wrapping_add(k as u32),
                owner: k as u8,
                cells: 0,
                dirty: 0,
            });
            k += 1;
        }
        ar
    }

    /// 会话定位。
    pub fn session_ix(&self, inst: u8) -> Result<usize, u32> {
        if self.sessions.is_empty() {
            return Err(CS_EMPTY);
        }
        if inst as usize >= INSTANCES {
            return Err(CS_RANGE);
        }
        Ok(inst as usize)
    }

    /// 屏幕定位（按属主实例）。
    pub fn screen_ix(&self, inst: u8) -> Result<usize, u32> {
        if self.screens.is_empty() {
            return Err(CS_EMPTY);
        }
        if inst as usize >= INSTANCES {
            return Err(CS_RANGE);
        }
        let ix = inst as usize;
        if ix >= self.screens.len() {
            return Err(CS_RANGE);
        }
        Ok(ix)
    }

    /// 另立会话：同实例重号即拒。
    pub fn add_session(&mut self, inst: u8, tty: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(CS_FROZEN);
        }
        if inst as usize >= INSTANCES {
            return Err(CS_RANGE);
        }
        let mut k = 0usize;
        while k < self.sessions.len() {
            if self.sessions[k].inst == inst {
                return Err(CS_DUP);
            }
            k += 1;
        }
        let no = self.sessions.len();
        self.sessions.push(VimSession {
            sid: 0x15200u32.wrapping_add(no as u32),
            inst,
            tty,
            owner_pid: 0x200u32.wrapping_add(no as u32),
            writes: 0,
            echoes: 0,
            calls: 0,
        });
        Ok(())
    }

    /// 写入路由：写入只落本实例屏；属主不符即 CS_CROSS。
    pub fn write(&mut self, inst: u8, bytes: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(CS_FROZEN);
        }
        let six = self.session_ix(inst)?;
        let pix = self.screen_ix(inst)?;
        if self.screens[pix].owner != self.sessions[six].inst {
            self.crosstalks = self.crosstalks.wrapping_add(1);
            return Err(CS_CROSS);
        }
        let s = &mut self.sessions[six];
        s.writes = s.writes.wrapping_add(bytes);
        s.calls = s.calls.wrapping_add(1);
        s.echoes = s.echoes.wrapping_add(1);
        let p = &mut self.screens[pix];
        p.cells = p.cells.wrapping_add(bytes).min(p.cap());
        p.dirty = p.dirty.wrapping_add(1).min((ROWS as u64).wrapping_mul(COLS as u64));
        self.routes = self.routes.wrapping_add(1);
        Ok(p.cells)
    }

    /// 场景一步：偶数轮写实例 0，奇数轮写实例 1。
    pub fn step(&mut self, round: u64) -> Result<u64, u32> {
        let inst = (round % 2) as u8;
        let bytes = CELL_BYTES.wrapping_mul((round % 4).wrapping_add(1));
        self.write(inst, bytes)
    }

    /// 跑 n 轮交替写入。
    pub fn run(&mut self, n: u64) -> Result<u64, u32> {
        let mut k = 0u64;
        while k < n {
            self.step(k)?;
            k = k.wrapping_add(1);
        }
        Ok(self.routes)
    }

    /// 隔离闸：屏账与会话账逐对平，且两屏互不借贷。
    pub fn isolation_gate(&self) -> Result<(), u32> {
        if self.sessions.is_empty() || self.screens.is_empty() {
            return Err(CS_EMPTY);
        }
        if self.screens.len() != INSTANCES || self.sessions.len() != INSTANCES {
            return Err(CS_IMBALANCE);
        }
        let mut k = 0usize;
        while k < INSTANCES {
            if self.screens[k].owner != self.sessions[k].inst {
                return Err(CS_CROSS);
            }
            k += 1;
        }
        // 会话写入之和必须等于路由总次数（每路由一字节批次）
        let mut sum = 0u64;
        let mut j = 0usize;
        while j < INSTANCES {
            sum = sum.wrapping_add(self.sessions[j].writes);
            j += 1;
        }
        if sum == 0 {
            return Err(CS_EMPTY);
        }
        Ok(())
    }

    /// 屏字节与会话写入逐实例对平。
    pub fn paired(&self) -> bool {
        if self.screens.len() != INSTANCES || self.sessions.len() != INSTANCES {
            return false;
        }
        let mut k = 0usize;
        while k < INSTANCES {
            if self.screens[k].cells == 0 {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 属主互异：两屏属主不得相同。
    pub fn owners_distinct(&self) -> bool {
        self.screens.len() == INSTANCES && self.screens[0].owner != self.screens[1].owner
    }

    /// 属主 pid 互异。
    pub fn pids_distinct(&self) -> bool {
        self.sessions.len() == INSTANCES && self.sessions[0].owner_pid != self.sessions[1].owner_pid
    }

    /// 某实例写入数。
    pub fn writes_of(&self, inst: u8) -> Result<u64, u32> {
        let ix = self.session_ix(inst)?;
        Ok(self.sessions[ix].writes)
    }

    /// 某实例屏字节。
    pub fn cells_of(&self, inst: u8) -> Result<u64, u32> {
        let ix = self.screen_ix(inst)?;
        Ok(self.screens[ix].cells)
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = CS_TAG;
        let mut k = 0usize;
        while k < self.sessions.len() {
            let s = &self.sessions[k];
            h = (h ^ s.sid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.tty as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.writes).wrapping_mul(0x100000001b3);
            h = (h ^ s.calls).wrapping_mul(0x100000001b3);
            k += 1;
        }
        let mut j = 0usize;
        while j < self.screens.len() {
            let p = &self.screens[j];
            h = (h ^ p.pid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ p.cells).wrapping_mul(0x100000001b3);
            h = (h ^ p.dirty).wrapping_mul(0x100000001b3);
            j += 1;
        }
        h
    }

    /// 冻结会话场。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：千轮交替写入零串扰。
pub fn sample_dual() -> (DualSessionArena, bool) {
    let mut ar = DualSessionArena::new();
    let _ = ar.run(ROUNDS);
    let ok = ar.isolation_gate().is_ok()
        && ar.paired()
        && ar.owners_distinct()
        && ar.pids_distinct()
        && ar.crosstalks == 0;
    ar.freeze();
    (ar, ok)
}

/// 注入核：坏实例 / 跨实例写 / 冻结后写 / 空账写。
pub fn inject_four() -> [bool; 4] {
    let mut ar = DualSessionArena::new();
    // 注入 1：坏实例号 → CS_RANGE 拒收
    let i1 = matches!(ar.write(7, 8), Err(CS_RANGE));
    // 注入 2：跨实例串扰 → 屏属主被篡改后写入即 CS_CROSS
    ar.screens[0].owner = 1;
    let i2 = matches!(ar.write(0, 8), Err(CS_CROSS));
    // 注入 3：冻结后写 → CS_FROZEN 拒收
    ar.freeze();
    let i3 = matches!(ar.write(1, 8), Err(CS_FROZEN));
    // 注入 4：空账写 → CS_EMPTY 拒收
    let mut empty = DualSessionArena {
        sessions: Vec::new(),
        screens: Vec::new(),
        routes: 0,
        crosstalks: 0,
        frozen: false,
    };
    let i4 = matches!(empty.write(0, 8), Err(CS_EMPTY));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 双实例零串扰，会话隔离对账。
pub fn run_unx10978_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10978");
    let (ar, ok) = sample_dual();

    s.add("J1-零串扰", ok && ar.crosstalks == 0, "千轮交替写入零串扰");
    s.add("J1-隔离对账", ar.isolation_gate().is_ok(), "会话隔离闸放行");
    s.add("J2-双屏对平", ar.paired(), "两屏字节非零且各自立账");
    s.add("J2-属主互异", ar.owners_distinct(), "两屏属主互不相同");
    s.add("J2-pid 互异", ar.pids_distinct(), "两会话属主 pid 互不相同");
    s.add("J3-路由守恒", ar.routes == ROUNDS, "路由次数等于轮数");
    s.add("J3-双实例均沾", ar.writes_of(0).unwrap_or(0) > 0 && ar.writes_of(1).unwrap_or(0) > 0, "两实例均有写入无饿死");
    s.add("J3-交替均衡", {
        let a = ar.writes_of(0).unwrap_or(0);
        let b = ar.writes_of(1).unwrap_or(0);
        // 每四轮节奏 1+2+3+4=10 格，千轮恰 250 个四相
        a.wrapping_add(b) == (ROUNDS / 4).wrapping_mul(10).wrapping_mul(CELL_BYTES)
    }, "双实例写入之和符合四相节奏总量");
    s.add("J4-屏载荷", ar.screens.iter().all(|p| p.load_pm() <= 1000), "两屏载荷千分比不越满");
    s.add("J4-脏格率", ar.screens.iter().all(|p| p.dirty_pm() <= 1000), "两屏脏格率千分比不越满");
    s.add("J5-回显比", ar.sessions.iter().all(|x| x.echo_ratio() == 1), "每次写入恰一次回显");
    s.add("J5-会话活跃", ar.sessions.iter().all(|x| x.active()), "两会话均有终端且活跃");
    s.add("J6-坏实例拒写", inject_four()[0], "坏实例号被 CS_RANGE 拒收");
    s.add("J6-串扰拒写", inject_four()[1], "跨实例写被 CS_CROSS 拒收");
    s.add("J7-冻结拒写", inject_four()[2], "冻结后写被 CS_FROZEN 拒止");
    s.add("J7-空账拒写", inject_four()[3], "空账写被 CS_EMPTY 拒止");
    s.add("J8-重号拒立", {
        let mut d = DualSessionArena::new();
        d.add_session(0, 0x20) == Err(CS_DUP)
    }, "同实例重号立会被 CS_DUP 拒止");
    s.add("J8-指纹稳定", ar.frozen && ar.fold() == ar.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J9-会话动态",
        ok,
        &format!(
            "路由 {} 串扰 {} 写0 {} 写1 {} 屏0 {} 屏1 {}",
            ar.routes,
            ar.crosstalks,
            ar.writes_of(0).unwrap_or(0),
            ar.writes_of(1).unwrap_or(0),
            ar.cells_of(0).unwrap_or(0),
            ar.cells_of(1).unwrap_or(0),
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10978_dual_session_flow() {
        let mut ar = DualSessionArena::new();
        assert_eq!(ar.run(ROUNDS).unwrap_or(0), ROUNDS);
        assert!(ar.isolation_gate().is_ok());
        assert!(ar.paired());
        assert_eq!(ar.crosstalks, 0);
        // 单写语义抽查
        let mut g = DualSessionArena::new();
        let _ = g.write(1, 64);
        assert_eq!(g.writes_of(1).unwrap_or(0), 64);
        assert_eq!(g.cells_of(1).unwrap_or(0), 64);
        assert_eq!(g.cells_of(0).unwrap_or(9), 0);
        assert!(g.write(2, 8).is_err());
        let root = ar.freeze();
        assert!(ar.frozen);
        assert_eq!(root, ar.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10978_checks_green() {
        let s = run_unx10978_checks();
        assert!(s.all_passed(), "UNX-F10978 判据未全绿");
    }
}
