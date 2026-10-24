//! VE-F3848 · 读屏与表面
//!
//! 表面语义投影高亮对齐 + 公告路由复用 + 跨表面导航序 + 断链 P0。
//! 投影对齐红线：读屏焦点与表面内视觉焦点错位。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xC4xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xC400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CC4Code(pub u16);

impl CC4Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const PROJ_MISALIGN: Self = Self(SEG_BASE + 1);
    pub const ROUTE_BROKEN: Self = Self(SEG_BASE + 2);
    pub const CROSS_ORDER: Self = Self(SEG_BASE + 3);
    pub const PROJ_BROKEN: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::PROJ_MISALIGN => "投影对齐错位（P1）",
            Self::ROUTE_BROKEN => "公告路由断",
            Self::CROSS_ORDER => "跨表面导航序乱",
            Self::PROJ_BROKEN => "投影断链（P0）",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 表面条目。
#[derive(Debug, Clone, Copy)]
pub struct Surface {
    pub id: u32,
    /// 导航序。
    pub order: u32,
    /// 读屏焦点节点。
    pub sr_focus_node: u32,
    /// 表面内视觉焦点节点。
    pub visual_focus_node: u32,
    /// 投影是否连通。
    pub projection_alive: bool,
    /// 公告路由是否可达。
    pub route_alive: bool,
}

impl Surface {
    /// 对齐：读屏焦点 == 视觉焦点。
    pub fn aligned(&self) -> bool {
        self.sr_focus_node == self.visual_focus_node
    }
}

/// 表面协同器。
#[derive(Debug, Clone)]
pub struct SurfaceBridge {
    pub surfaces: Vec<Surface>,
}

impl SurfaceBridge {
    pub fn new() -> Self {
        Self {
            surfaces: Vec::new(),
        }
    }

    /// 对齐扫描。
    pub fn all_aligned(&self) -> bool {
        self.surfaces.iter().all(|s| s.aligned())
    }

    /// 路由完整。
    pub fn routes_intact(&self) -> bool {
        self.surfaces.iter().all(|s| s.route_alive)
    }

    /// 投影连通。
    pub fn projections_alive(&self) -> bool {
        self.surfaces.iter().all(|s| s.projection_alive)
    }

    /// 跨表面序单调。
    pub fn cross_order_ok(&self) -> bool {
        let mut prev = 0u32;
        for s in self.surfaces.iter() {
            if s.order < prev {
                return false;
            }
            prev = s.order;
        }
        true
    }

    /// 全检。
    pub fn full_check(&self) -> Result<(), CC4Code> {
        if !self.projections_alive() {
            return Err(CC4Code::PROJ_BROKEN);
        }
        if !self.all_aligned() {
            return Err(CC4Code::PROJ_MISALIGN);
        }
        if !self.routes_intact() {
            return Err(CC4Code::ROUTE_BROKEN);
        }
        if !self.cross_order_ok() {
            return Err(CC4Code::CROSS_ORDER);
        }
        Ok(())
    }
}

impl Default for SurfaceBridge {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves148_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves148_surface");

    fn ready() -> SurfaceBridge {
        let mut b = SurfaceBridge::new();
        b.surfaces.push(Surface {
            id: 1,
            order: 1,
            sr_focus_node: 10,
            visual_focus_node: 10,
            projection_alive: true,
            route_alive: true,
        });
        b.surfaces.push(Surface {
            id: 2,
            order: 2,
            sr_focus_node: 20,
            visual_focus_node: 20,
            projection_alive: true,
            route_alive: true,
        });
        b
    }

    // 01 对齐
    {
        let b = ready();
        chk(&mut set, "C3848-01", b.all_aligned(), "err");
    }
    // 02 错位
    {
        let mut b = ready();
        b.surfaces[0].visual_focus_node = 99;
        chk(
            &mut set,
            "C3848-02",
            b.full_check() == Err(CC4Code::PROJ_MISALIGN),
            "err",
        );
    }
    // 03 路由完整
    {
        let b = ready();
        chk(&mut set, "C3848-03", b.routes_intact(), "err");
    }
    // 04 路由断
    {
        let mut b = ready();
        b.surfaces[1].route_alive = false;
        chk(
            &mut set,
            "C3848-04",
            b.full_check() == Err(CC4Code::ROUTE_BROKEN),
            "err",
        );
    }
    // 05 投影连通
    {
        let b = ready();
        chk(&mut set, "C3848-05", b.projections_alive(), "err");
    }
    // 06 投影断 P0
    {
        let mut b = ready();
        b.surfaces[0].projection_alive = false;
        chk(
            &mut set,
            "C3848-06",
            b.full_check() == Err(CC4Code::PROJ_BROKEN),
            "err",
        );
    }
    // 07 跨表面序
    {
        let b = ready();
        chk(&mut set, "C3848-07", b.cross_order_ok(), "err");
    }
    // 08 序乱
    {
        let mut b = ready();
        b.surfaces[1].order = 0;
        chk(
            &mut set,
            "C3848-08",
            b.full_check() == Err(CC4Code::CROSS_ORDER),
            "err",
        );
    }
    // 09 全检通过
    {
        let b = ready();
        chk(&mut set, "C3848-09", b.full_check().is_ok(), "err");
    }
    // 10 空集
    {
        let b = SurfaceBridge::new();
        chk(&mut set, "C3848-10", b.full_check().is_ok(), "err");
    }
    // 11 对齐语义
    {
        let s = Surface {
            id: 1,
            order: 1,
            sr_focus_node: 5,
            visual_focus_node: 5,
            projection_alive: true,
            route_alive: true,
        };
        chk(&mut set, "C3848-11", s.aligned(), "err");
    }
    // 12 多表面对齐
    {
        let b = ready();
        chk(&mut set, "C3848-12", b.surfaces.len() == 2 && b.all_aligned(), "err");
    }

    set
}
