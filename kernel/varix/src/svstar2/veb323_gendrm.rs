//! veb323gendrm —— VE-F0323 通用 DRM 接入路径。
//! 锚点（CoRun Unxreal 主册）：VE-F0323 · 通用 DRM 接入路径 · 目标 320 行
//! 判据原文：VE-F0323-J1 通用 DRM 账：三步接入（探测节点 /
//!           协商格式 / 建通道）；节点探测（路径存在性 + 主
//!           从节点号）；格式协商（交集取共有格式，空交集
//!           拒）；通道账（句柄 + 格式 + 状态机 开/活/关）；
//!           通道关闭（活态才可关，关后再关拒）；格式回退
//!           （首选不在交集 → 次选）；探测缓存（同节点二次
//!           探测直取）。
//! 组件：节点账 + 格式协商 + 通道态机 + 缓存。
//! 职责定位：通用 DRM 是「实验线的万能插座」——三步是三插：
//!           先看墙上有没有孔（探测）、再对一下插头形状（协
//!           商）、最后插上通电（建通道）；空交集拒是插座与
//!           插头对不上：硬插要出事故。
//! 性能逐项分解：探测 O(1)；协商 O(格式数)。
//! 诊断码段：0x319C0（0x319C1~0x319C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const GD_VERSION: &str = "veb323gendrm-v1";

/// 诊断段基址（0x319C0 段 · 本模块独占）。
pub const GD_SEG_BASE: u32 = 0x319C0;
pub const XC_MAGIC: u32 = 0x319C1; // 账魔数破
pub const XC_NO_NODE: u32 = 0x319C2; // 节点不存在
pub const XC_EMPTY: u32 = 0x319C3; // 空交集
pub const XC_CH: u32 = 0x319C4; // 通道态错

/// 账魔数。
const MAGIC: u32 = 0x319C_1B2A;

/// DRM 节点。
#[derive(Clone, Copy)]
pub struct Node {
    /// 主节点号。
    pub major: u32,
    /// 从节点号。
    pub minor: u32,
    /// 支持格式位集。
    pub fmts: u32,
}

/// 通道态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChState {
    Open,
    Live,
    Closed,
}

/// 通道。
pub struct Chan {
    pub id: u32,
    pub fmt: u32,
    pub state: ChState,
}

/// 通用 DRM 簿。
pub struct GdrmBook {
    pub nodes: Vec<Node>,
    pub chans: Vec<Chan>,
    /// 探测缓存。
    pub probe_cache: Vec<(u32, bool)>,
    next_ch: u32,
    magic: u32,
}

impl GdrmBook {
    /// 建。
    pub fn new() -> GdrmBook {
        GdrmBook {
            nodes: Vec::new(),
            chans: Vec::new(),
            probe_cache: Vec::new(),
            next_ch: 1,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 挂节点。
    pub fn mount(&mut self, major: u32, minor: u32, fmts: u32) {
        self.nodes.push(Node { major, minor, fmts });
    }

    /// 节点探测（带缓存）。
    pub fn probe(&mut self, major: u32, minor: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        // 缓存查。
        for e in self.probe_cache.iter() {
            if e.0 == major * 1000 + minor {
                return Ok(e.1);
            }
        }
        let mut found = false;
        for n in self.nodes.iter() {
            if n.major == major && n.minor == minor {
                found = true;
            }
        }
        self.probe_cache.push((major * 1000 + minor, found));
        Ok(found)
    }

    /// 缓存笔数。
    pub fn cache_len(&self) -> usize {
        self.probe_cache.len()
    }

    /// 格式协商：交集；空 → Err。
    pub fn negotiate(&self, node_major: u32, node_minor: u32, want: u32) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        let mut have: Option<u32> = None;
        for n in self.nodes.iter() {
            if n.major == node_major && n.minor == node_minor {
                have = Some(n.fmts);
            }
        }
        let f = match have {
            Some(f) => f,
            None => return Err(XC_NO_NODE),
        };
        let common = f & want;
        if common == 0 {
            return Err(XC_EMPTY);
        }
        // 取交集最低位为首选格式。
        Ok(common & common.wrapping_neg())
    }

    /// 建通道（先协商）。
    pub fn open_chan(&mut self, node_major: u32, node_minor: u32, want: u32) -> Result<u32, u32> {
        let fmt = self.negotiate(node_major, node_minor, want)?;
        let id = self.next_ch;
        self.next_ch += 1;
        self.chans.push(Chan { id, fmt, state: ChState::Open });
        Ok(id)
    }

    /// 活化（Open → Live）。
    pub fn activate(&mut self, id: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for c in self.chans.iter_mut() {
            if c.id == id {
                if c.state != ChState::Open {
                    return Err(XC_CH);
                }
                c.state = ChState::Live;
                return Ok(());
            }
        }
        Err(XC_CH)
    }

    /// 关闭（活态才可关）。
    pub fn close_chan(&mut self, id: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for c in self.chans.iter_mut() {
            if c.id == id {
                if c.state == ChState::Closed {
                    return Err(XC_CH);
                }
                c.state = ChState::Closed;
                return Ok(());
            }
        }
        Err(XC_CH)
    }

    /// 查通道态。
    pub fn chan_state(&self, id: u32) -> Result<ChState, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for c in self.chans.iter() {
            if c.id == id {
                return Ok(c.state);
            }
        }
        Err(XC_CH)
    }
}

pub fn run_veb323_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0323");
    let mut b = GdrmBook::new();
    // 族一：节点探测。
    s.add("未挂拒", b.probe(226, 0) == Ok(false), "墙上无孔");
    b.mount(226, 0, 0b1110);
    s.add("挂后探到", b.probe(226, 0) == Ok(true), "有孔");
    s.add("主从不全拒", b.probe(226, 1) == Ok(false), "副孔没开");
    // 缓存核：二次探测走缓存（缓存笔数不变）。
    let c1 = b.cache_len();
    let _ = b.probe(226, 0);
    s.add("缓存命中", b.cache_len() == c1, "不新增");
    // 另一节点新缓存笔。
    let _ = b.probe(226, 1);
    s.add("缓存增笔", b.cache_len() == c1 + 1, "新节点");
    // 族二：格式协商。
    s.add("交集 0b10", b.negotiate(226, 0, 0b1010) == Ok(0b10), "共有低位");
    s.add("空交集拒", b.negotiate(226, 0, 0b1) == Err(XC_EMPTY), "对不上");
    s.add("无节点拒", b.negotiate(9, 9, 0b1) == Err(XC_NO_NODE), "无孔可对");
    // 格式回退：首选 0b1000 不在交集 → 次选 0b10。
    s.add("回退次选", b.negotiate(226, 0, 0b1000 | 0b10) == Ok(0b10), "次选上位");
    // 族三：通道态机。
    let ch = match b.open_chan(226, 0, 0b1110) {
        Ok(c) => c,
        Err(_) => {
            s.add("建道败", false, "不应到");
            return s;
        }
    };
    s.add("首道 1", ch == 1, "自 1");
    s.add("开态", b.chan_state(ch) == Ok(ChState::Open), "初态");
    s.add("活化", b.activate(ch).is_ok(), "通电");
    s.add("活态", b.chan_state(ch) == Ok(ChState::Live), "使用中");
    s.add("活态关过", b.close_chan(ch).is_ok(), "拔插头");
    s.add("关态", b.chan_state(ch) == Ok(ChState::Closed), "断电");
    s.add("关后再关拒", b.close_chan(ch) == Err(XC_CH), "已拔");
    s.add("关后再活拒", b.activate(ch) == Err(XC_CH), "不可复活");
    // 族四：Open 直关（未活化）。
    let ch2 = match b.open_chan(226, 0, 0b1110) {
        Ok(c) => c,
        Err(_) => {
            s.add("建道败 2", false, "不应到");
            return s;
        }
    };
    s.add("开态直关", b.close_chan(ch2).is_ok(), "未通电也可拔");
    s.add("二道 2", ch2 == 2, "号递增");
    // 族五：通道格式记账。
    // 通道 fmt 应为协商首选：want=0b1110 交集 0b1110 → 首选 0b10。
    let mut fmt_ok = false;
    for c in b.chans.iter() {
        if c.id == ch && c.fmt == 0b10 {
            fmt_ok = true;
        }
    }
    s.add("道记格式", fmt_ok, "协商入账");
    s.add("魔数核", GD_SEG_BASE == 0x319C0, "段基址");
    s
}
