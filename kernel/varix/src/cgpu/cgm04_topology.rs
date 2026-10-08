//! CGPU-F1924 · 多显示器拓扑（CGPU-M 域 · 显示输出 · 批次 M01 · 单 04）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1924`
//!
//! 拓扑：多屏拓扑（排列/主屏/克隆/扩展——拓扑模型）；拓扑变更（布局变更
//! 事件——变更传播）；持久（拓扑持久（重启保持——持久复用 F1834 模式——
//! 模式复用）；与 OS（OS 显示设置协同——协同声明；测试（模型/变更/持久/
//! 协同四组）。判据：拓扑模型、传播、持久复用、协同、四组、判据。
//!
//! # 要点一：拓扑模型是闭集三角色，不是自由矩形堆
//!
//! 每屏恰一角色 [`TopoRole`]：Primary 恰一（主屏）；Extend 独立矩形
//! （扩展）；Clone 挂靠源屏（克隆——同源同矩形）。排列即角色+矩形的
//! 全集：任何拓扑状态都能被 [`validate`] 显性裁决，表外状态不存在。
//!
//! # 要点二：变更传播走单调事件账，倒挂即拒
//!
//! [`Topology::apply`] 校验通过才落账：seq 单调 +1，事件入账（封顶
//! [`EVENT_CAP`] 滚动淘汰）；[`Topology::replay`] 按 last_seen 回放差集，
//! last_seen > 当前 seq 即 [`codes::PROPAGATION_STALE`]——消费端时钟倒挂
//! 不是可容忍状态，是显性码拒绝。
//!
//! # 要点三：持久复用 F1834 模式（模式复用声明）
//!
//! [`PERSIST_PATTERN`]：序列化 + 校验和 + 重启保持——与 F1834 显示配置
//! 持久同一语义（版本字节 + FNV 校验和 + 恢复后 seq/cells 逐字段全等，
//! 事件史为会话内面不持久）；F1834 落位后由其提供共享实现，本单先按
//! 同一语义自持，切换点只有 serialize/restore 两个签名。
//!
//! # 要点四：OS 协同是声明面，不是直写面
//!
//! [`OS_COORD`]：拓扑变更向 OS 显示设置单向通报，OS 侧改动经变更事件
//! 回流；内核不直写 OS 设置存储——本模块类型面上没有「写 OS 设置」的
//! API（诚实边界：声明归档，协同实现归 OS 集成层）。
//!
//! # 要点五：诊断码延续 0x54xx 域段
//!
//! cgm01 占 0x5401~0x5407；cgm02 占 0x5408~0x540C；cgm03 占
//! 0x540D~0x5412；本单占 0x5413~0x5418；判据防自判死断言互异。
//!
//! # 要点六：测试四组（模型/变更/持久/协同）判据逐条对账
//!
//! 见 `cgm04_topology_checks.rs`：模型组（角色闭集/主屏恰一/矩形不重叠/
//! 克隆语义）、变更组（事件账单调/回放差集/倒挂拒）、持久组（往返全等/
//! 篡改与版本拒/重启保持）、协同组（声明逐字 grep + 类型面无直写）。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap`/`expect`/索引越界：变长访问先长度对账，字节解码
//! 手工拼装，失败路径走 `Result` 与显性 VmCode。

use crate::cgpu::cgm01_display::VmCode;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（cgm04 续占 0x5413..0x5418）
// ---------------------------------------------------------------------------

/// M 域诊断码（cgm04 段）。
pub mod codes {
    use super::VmCode;

    /// 拓扑结构损坏（空集/超员/ID 重复或为零/角色字节表外）。
    pub const TOPOLOGY_MALFORMED: VmCode = VmCode(0x5413);

    /// 主屏不恰一（零主屏或多主屏）。
    pub const PRIMARY_DUPLICATE: VmCode = VmCode(0x5414);

    /// 独立矩形重叠（Primary/Extend 之间轴对齐相交）。
    pub const ARRANGE_OVERLAP: VmCode = VmCode(0x5415);

    /// 克隆语义违约（源不存在/源仍是克隆/矩形与源不一致）。
    pub const CLONE_CONFLICT: VmCode = VmCode(0x5416);

    /// 传播序号倒挂（last_seen 超前于事件账）。
    pub const PROPAGATION_STALE: VmCode = VmCode(0x5417);

    /// 持久恢复不匹配（版本不符/校验和不符/角色字节表外于恢复面）。
    pub const RESTORE_MISMATCH: VmCode = VmCode(0x5418);
}

// ---------------------------------------------------------------------------
// 二、模型常量与复用声明
// ---------------------------------------------------------------------------

/// 单拓扑屏幕数上限（含主屏）。
pub const MAX_SCREENS: usize = 8;

/// 事件账封顶（滚动淘汰最旧）。
pub const EVENT_CAP: usize = 64;

/// 持久载荷魔数。
pub const PERSIST_MAGIC: &[u8; 4] = b"VTOP";

/// 持久载荷版本。
pub const PERSIST_VERSION: u8 = 1;

/// 「无克隆源」哨兵值。
pub const CLONE_NONE: u16 = 0xFFFF;

/// 持久模式复用声明（F1834 显示配置持久——序列化+校验和+重启保持；
/// F1834 落位后由其提供共享实现，本单先按同一语义自持）。
pub const PERSIST_PATTERN: &str =
    "复用 F1834 显示配置持久模式：序列化+校验和+重启保持（拓扑事件史为会话内面，不持久）";

/// OS 显示设置协同声明（协同面在 OS 集成层——内核不直写 OS 设置存储）。
pub const OS_COORD: &str =
    "OS 显示设置协同：拓扑变更向 OS 显示设置单向通报，OS 侧改动经变更事件回流；内核不直写 OS 设置存储——声明归档";

// ---------------------------------------------------------------------------
// 三、拓扑模型（闭集三角色）
// ---------------------------------------------------------------------------

/// 屏幕角色闭集：主屏 / 扩展 / 克隆。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopoRole {
    /// 主屏（恰一）。
    Primary,
    /// 扩展屏（独立矩形）。
    Extend,
    /// 克隆屏（同源同矩形）。
    Clone,
}

impl TopoRole {
    /// 持久编码（闭集 0/1/2）。
    pub fn to_byte(self) -> u8 {
        match self {
            TopoRole::Primary => 0u8,
            TopoRole::Extend => 1u8,
            TopoRole::Clone => 2u8,
        }
    }

    /// 持久解码（表外字节 → None）。
    pub fn from_byte(b: u8) -> Option<TopoRole> {
        match b {
            0u8 => Some(TopoRole::Primary),
            1u8 => Some(TopoRole::Extend),
            2u8 => Some(TopoRole::Clone),
            _ => None,
        }
    }
}

/// 单屏拓扑格元：身份 + 角色 + 排列矩形 + 克隆源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenCell {
    /// 显示标识（非零，拓扑内唯一）。
    pub id: u16,
    /// 角色（闭集）。
    pub role: TopoRole,
    /// 排列矩形左上 x。
    pub x: i32,
    /// 排列矩形左上 y。
    pub y: i32,
    /// 排列矩形宽。
    pub w: u32,
    /// 排列矩形高。
    pub h: u32,
    /// 克隆源显示标识（非克隆屏为 [`CLONE_NONE`]）。
    pub clone_of: u16,
}

/// 轴对齐矩形相交判定（i32 中间量防溢出；退化面积为零不算相交）。
pub fn rect_overlap(a: &ScreenCell, b: &ScreenCell) -> bool {
    let ax1 = a.x;
    let ay1 = a.y;
    let ax2 = a.x + a.w as i32;
    let ay2 = a.y + a.h as i32;
    let bx1 = b.x;
    let by1 = b.y;
    let bx2 = b.x + b.w as i32;
    let by2 = b.y + b.h as i32;
    ax1 < bx2 && bx1 < ax2 && ay1 < by2 && by1 < ay2
}

/// 拓扑结构校验（角色闭集裁决；表外状态不存在）。
pub fn validate(cells: &[ScreenCell]) -> Result<(), VmCode> {
    if cells.is_empty() || cells.len() > MAX_SCREENS {
        return Err(codes::TOPOLOGY_MALFORMED);
    }
    // ID 唯一且非零。
    let mut i = 0usize;
    while i < cells.len() {
        if cells[i].id == 0u16 {
            return Err(codes::TOPOLOGY_MALFORMED);
        }
        let mut j = i + 1usize;
        while j < cells.len() {
            if cells[i].id == cells[j].id {
                return Err(codes::TOPOLOGY_MALFORMED);
            }
            j += 1usize;
        }
        i += 1usize;
    }
    // 主屏恰一。
    let mut primaries = 0usize;
    for c in cells.iter() {
        if c.role == TopoRole::Primary {
            primaries += 1usize;
        }
    }
    if primaries != 1usize {
        return Err(codes::PRIMARY_DUPLICATE);
    }
    // 克隆语义：源存在、源非克隆、矩形与源全等。
    let mut i = 0usize;
    while i < cells.len() {
        let c = &cells[i];
        if c.role == TopoRole::Clone {
            if c.clone_of == CLONE_NONE {
                return Err(codes::CLONE_CONFLICT);
            }
            let mut src: Option<&ScreenCell> = None;
            for d in cells.iter() {
                if d.id == c.clone_of {
                    src = Some(d);
                }
            }
            match src {
                None => return Err(codes::CLONE_CONFLICT),
                Some(d) => {
                    if d.role == TopoRole::Clone {
                        return Err(codes::CLONE_CONFLICT);
                    }
                    if d.x != c.x || d.y != c.y || d.w != c.w || d.h != c.h {
                        return Err(codes::CLONE_CONFLICT);
                    }
                }
            }
        } else if c.clone_of != CLONE_NONE {
            // 非克隆屏不得携带克隆源。
            return Err(codes::CLONE_CONFLICT);
        }
        i += 1usize;
    }
    // Primary/Extend 两两不重叠（克隆屏豁免——同源同矩形即克隆语义）。
    let mut i = 0usize;
    while i < cells.len() {
        if cells[i].role == TopoRole::Clone {
            i += 1usize;
            continue;
        }
        let mut j = i + 1usize;
        while j < cells.len() {
            if cells[j].role != TopoRole::Clone && rect_overlap(&cells[i], &cells[j]) {
                return Err(codes::ARRANGE_OVERLAP);
            }
            j += 1usize;
        }
        i += 1usize;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、变更传播（单调事件账）
// ---------------------------------------------------------------------------

/// 拓扑变更种类闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopoEventKind {
    /// 布局变更（矩形/克隆关系变化）。
    LayoutChanged,
    /// 主屏变更。
    PrimaryChanged,
    /// 屏幕加入。
    ScreenAdded,
    /// 屏幕移除。
    ScreenRemoved,
}

/// 布局变更事件（单调序号）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopoEvent {
    /// 事件序号（与账本 seq 一致）。
    pub seq: u64,
    /// 变更种类。
    pub kind: TopoEventKind,
}

/// 多显示器拓扑（模型 + 单调事件账）。
#[derive(Debug, Clone)]
pub struct Topology {
    seq: u64,
    cells: Vec<ScreenCell>,
    events: Vec<TopoEvent>,
}

impl Topology {
    /// 建账：校验通过才成立（seq 从 0 起）。
    pub fn new(cells: Vec<ScreenCell>) -> Result<Topology, VmCode> {
        validate(&cells)?;
        Ok(Topology {
            seq: 0u64,
            cells,
            events: Vec::new(),
        })
    }

    /// 当前序号。
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// 当前拓扑只读视图。
    pub fn cells(&self) -> &[ScreenCell] {
        &self.cells
    }

    /// 主屏标识（结构不变量保证恰一；账本只在 validate 过的态上）。
    pub fn primary_id(&self) -> Option<u16> {
        let mut found = None;
        for c in self.cells.iter() {
            if c.role == TopoRole::Primary {
                found = Some(c.id);
            }
        }
        found
    }

    /// 应用拓扑变更：校验通过才落账（seq 单调 +1，事件封顶滚动）。
    pub fn apply(
        &mut self,
        cells: Vec<ScreenCell>,
        kind: TopoEventKind,
    ) -> Result<TopoEvent, VmCode> {
        validate(&cells)?;
        self.cells = cells;
        self.seq = match self.seq.checked_add(1u64) {
            Some(v) => v,
            None => return Err(codes::PROPAGATION_STALE),
        };
        let ev = TopoEvent {
            seq: self.seq,
            kind,
        };
        if self.events.len() >= EVENT_CAP {
            // 封顶滚动：淘汰最旧（账非空才 remove，零 panic）。
            if !self.events.is_empty() {
                self.events.remove(0usize);
            }
        }
        self.events.push(ev);
        Ok(ev)
    }

    /// 传播回放：返回 last_seen 之后的事件差集；倒挂显性拒。
    pub fn replay(&self, last_seen: u64) -> Result<Vec<TopoEvent>, VmCode> {
        if last_seen > self.seq {
            return Err(codes::PROPAGATION_STALE);
        }
        let mut out = Vec::new();
        for ev in self.events.iter() {
            if ev.seq > last_seen {
                out.push(*ev);
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 五、持久（复用 F1834 模式：序列化+校验和+重启保持）
// ---------------------------------------------------------------------------

/// FNV-1a 64（与判据侧第二实现互为双源）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3u64);
    }
    h
}

impl Topology {
    /// 序列化（版本 + FNV 校验和；事件史不持久——会话内面）。
    ///
    /// 布局：magic(4) ver(1) seq(8) count(2) | 每屏 id(2) role(1) x(4)
    /// y(4) w(4) h(4) clone_of(2) | fnv(8)。
    pub fn serialize(&self) -> Vec<u8> {
        let per = 21usize;
        let body = 15usize + per * self.cells.len();
        let mut out = Vec::with_capacity(body + 8usize);
        for b in PERSIST_MAGIC.iter() {
            out.push(*b);
        }
        out.push(PERSIST_VERSION);
        let k: u64 = self.seq;
        let mut sh = 0u32;
        while sh < 64u32 {
            out.push((k >> sh) as u8);
            sh += 8u32;
        }
        let n = self.cells.len() as u16;
        out.push((n & 0xFFu16) as u8);
        out.push((n >> 8) as u8);
        for c in self.cells.iter() {
            let mut v: u64 = c.id as u64;
            out.push((v & 0xFFu64) as u8);
            out.push((v >> 8) as u8);
            out.push(c.role.to_byte());
            v = (c.x as i64) as u64;
            sh = 0u32;
            while sh < 32u32 {
                out.push((v >> sh) as u8);
                sh += 8u32;
            }
            v = (c.y as i64) as u64;
            sh = 0u32;
            while sh < 32u32 {
                out.push((v >> sh) as u8);
                sh += 8u32;
            }
            v = c.w as u64;
            sh = 0u32;
            while sh < 32u32 {
                out.push((v >> sh) as u8);
                sh += 8u32;
            }
            v = c.h as u64;
            sh = 0u32;
            while sh < 32u32 {
                out.push((v >> sh) as u8);
                sh += 8u32;
            }
            v = c.clone_of as u64;
            out.push((v & 0xFFu64) as u8);
            out.push((v >> 8) as u8);
        }
        let h = fnv1a(&out);
        sh = 0u32;
        while sh < 64u32 {
            out.push((h >> sh) as u8);
            sh += 8u32;
        }
        out
    }

    /// 恢复（重启保持）：魔数/长度损坏 → [`codes::TOPOLOGY_MALFORMED`]；
    /// 版本/校验和/角色字节不符 → [`codes::RESTORE_MISMATCH`]；语义校验
    /// 复用 [`validate`]；恢复后 seq/cells 与持久前逐字段全等。
    pub fn restore(bytes: &[u8]) -> Result<Topology, VmCode> {
        if bytes.len() < 23usize {
            return Err(codes::TOPOLOGY_MALFORMED);
        }
        let mut i = 0usize;
        for m in PERSIST_MAGIC.iter() {
            if bytes[i] != *m {
                return Err(codes::TOPOLOGY_MALFORMED);
            }
            i += 1usize;
        }
        if bytes[4usize] != PERSIST_VERSION {
            return Err(codes::RESTORE_MISMATCH);
        }
        let body = bytes.len() - 8usize;
        let want = fnv1a(&bytes[..body]);
        let mut got: u64 = 0u64;
        let mut sh = 0u32;
        while sh < 64u32 {
            got |= (bytes[body + (sh >> 3) as usize] as u64) << sh;
            sh += 8u32;
        }
        if got != want {
            return Err(codes::RESTORE_MISMATCH);
        }
        let mut seq: u64 = 0u64;
        sh = 0u32;
        while sh < 64u32 {
            seq |= (bytes[5usize + (sh >> 3) as usize] as u64) << sh;
            sh += 8u32;
        }
        let count = (bytes[13usize] as usize) | ((bytes[14usize] as usize) << 8);
        let per = 21usize;
        if count == 0usize || count > MAX_SCREENS {
            return Err(codes::TOPOLOGY_MALFORMED);
        }
        if body != 15usize + per * count {
            return Err(codes::TOPOLOGY_MALFORMED);
        }
        let mut cells: Vec<ScreenCell> = Vec::with_capacity(count);
        let mut p = 15usize;
        let mut k = 0usize;
        while k < count {
            let id = (bytes[p] as u16) | ((bytes[p + 1usize] as u16) << 8);
            let role_b = bytes[p + 2usize];
            let role = match TopoRole::from_byte(role_b) {
                None => return Err(codes::RESTORE_MISMATCH),
                Some(r) => r,
            };
            let mut x: u64 = 0u64;
            let mut s2 = 0u32;
            while s2 < 32u32 {
                x |= (bytes[p + 3usize + (s2 >> 3) as usize] as u64) << s2;
                s2 += 8u32;
            }
            let mut y: u64 = 0u64;
            s2 = 0u32;
            while s2 < 32u32 {
                y |= (bytes[p + 7usize + (s2 >> 3) as usize] as u64) << s2;
                s2 += 8u32;
            }
            let mut w: u64 = 0u64;
            s2 = 0u32;
            while s2 < 32u32 {
                w |= (bytes[p + 11usize + (s2 >> 3) as usize] as u64) << s2;
                s2 += 8u32;
            }
            let mut hh: u64 = 0u64;
            s2 = 0u32;
            while s2 < 32u32 {
                hh |= (bytes[p + 15usize + (s2 >> 3) as usize] as u64) << s2;
                s2 += 8u32;
            }
            let clo = (bytes[p + 19usize] as u16) | ((bytes[p + 20usize] as u16) << 8);
            cells.push(ScreenCell {
                id,
                role,
                x: x as i32,
                y: y as i32,
                w: w as u32,
                h: hh as u32,
                clone_of: clo,
            });
            p += per;
            k += 1usize;
        }
        validate(&cells)?;
        Ok(Topology {
            seq,
            cells,
            events: Vec::new(),
        })
    }
}
