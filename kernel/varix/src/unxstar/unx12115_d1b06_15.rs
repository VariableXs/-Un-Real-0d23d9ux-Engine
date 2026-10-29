//! UNX-F12115 越界与截断防御矩阵（0x1a080 段）
//!
//! 卡面：越界/截断防御矩阵——五类场景（复制越界/追加越界/格式截断/
//! 前缀越界/整数无容）各绑一个守卫码，派发必须命中正确守卫；
//! 错派与未绑场景显性拒。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 场景闭集：五场景枚举闭集，越码必拒。
//! - J2 场景-守卫绑定：每场景恰绑一守卫码，绑定行唯一。
//! - J3 派发命中：派发场景回绑定守卫码。
//! - J4 错派显性：派发结果与绑定不符判红。
//! - J5 未绑显性：未绑定场景派发必拒。
//! - J6 重行拒：同场景二次绑定必拒。
//! - J7 印账冻结：seal 后绑定必拒。
//! - J8 注入五况全抓。
//!
//! 结构：DefScene 五场景枚举 + DefRow 绑定行 + DefBook 矩阵册 +
//! 派发核 + 折叠指纹（211 进制卷积）+ 注入核（越场景/重行/未绑派发/
//! 错派红/印后绑定）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a080 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a080;
pub const DM_BAD_SCENE: u32 = SEG_BASE + 0x01; // 越场景
pub const DM_DUP_ROW: u32 = SEG_BASE + 0x02; // 重行
pub const DM_UNBOUND: u32 = SEG_BASE + 0x03; // 未绑派发
pub const DM_MISMATCH: u32 = SEG_BASE + 0x04; // 错派
pub const DM_FROZEN: u32 = SEG_BASE + 0x05; // 印账后绑定

/// 场景码。
pub const SC_COPY_OVER: u32 = 1; // 复制越界
pub const SC_APPEND_OVER: u32 = 2; // 追加越界
pub const SC_FMT_TRUNC: u32 = 3; // 格式截断
pub const SC_PREFIX_OOB: u32 = 4; // 前缀越界
pub const SC_INT_NOROOM: u32 = 5; // 整数无容

/// 守卫码（各场景对应防御守卫）。
pub const G_COPY: u32 = SEG_BASE + 0x10;
pub const G_APPEND: u32 = SEG_BASE + 0x11;
pub const G_FMT: u32 = SEG_BASE + 0x12;
pub const G_PREFIX: u32 = SEG_BASE + 0x13;
pub const G_INT: u32 = SEG_BASE + 0x14;

/// 场景枚举。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefScene {
    CopyOver = 1,
    AppendOver = 2,
    FmtTrunc = 3,
    PrefixOob = 4,
    IntNoRoom = 5,
}

impl DefScene {
    pub fn code(&self) -> u32 {
        *self as u32
    }
    /// 从原始码构造：越码必拒。
    pub fn from_code(code: u32) -> Result<DefScene, u32> {
        match code {
            1 => Ok(DefScene::CopyOver),
            2 => Ok(DefScene::AppendOver),
            3 => Ok(DefScene::FmtTrunc),
            4 => Ok(DefScene::PrefixOob),
            5 => Ok(DefScene::IntNoRoom),
            _ => Err(DM_BAD_SCENE),
        }
    }
}

/// 绑定行。
#[derive(Clone, Copy)]
pub struct DefRow {
    pub scene: DefScene,
    pub guard: u32,
}

/// 矩阵册。
pub struct DefBook {
    pub rows: Vec<DefRow>,
    pub reds: Vec<(u32, u32)>, // 错派红账
    pub sealed: bool,
}

impl DefBook {
    pub fn new() -> DefBook {
        DefBook { rows: Vec::new(), reds: Vec::new(), sealed: false }
    }

    /// 绑定：场景合法 → 守卫码面合法 → 重行拒。
    pub fn bind(&mut self, scene: DefScene, guard: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(DM_FROZEN);
        }
        if guard < SEG_BASE + 0x10 || guard > SEG_BASE + 0x1F {
            return Err(DM_BAD_SCENE); // 守卫码面越域
        }
        if self.rows.iter().any(|r| r.scene == scene) {
            return Err(DM_DUP_ROW);
        }
        self.rows.push(DefRow { scene, guard });
        Ok(())
    }

    /// 派发核：未绑显性拒；命中回绑定守卫码。
    pub fn dispatch(&mut self, scene: DefScene) -> Result<u32, u32> {
        if self.sealed {
            return Err(DM_FROZEN);
        }
        match self.rows.iter().find(|r| r.scene == scene) {
            None => {
                self.reds.push((DM_UNBOUND, scene.code()));
                Err(DM_UNBOUND)
            }
            Some(r) => Ok(r.guard),
        }
    }

    /// 错派核查：期望守卫与绑定不符判红。
    pub fn verify_hit(&mut self, scene: DefScene, expect: u32) -> bool {
        match self.dispatch(scene) {
            Ok(g) if g == expect => true,
            Ok(g) => {
                self.reds.push((DM_MISMATCH, scene.code()));
                let _ = g;
                false
            }
            Err(_) => false,
        }
    }

    /// 绑定行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.rows {
            h = h.wrapping_mul(211).wrapping_add(r.scene.code());
            h = h.wrapping_mul(211).wrapping_add(r.guard);
        }
        for r in &self.reds {
            h = h.wrapping_mul(211).wrapping_add(r.0);
            h = h.wrapping_mul(211).wrapping_add(r.1);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("rows={} reds={}", self.row_count(), self.reds.len())
    }

    /// 诊断面：行渲染。
    pub fn line_of(&self, scene: DefScene) -> String {
        match self.rows.iter().find(|r| r.scene == scene) {
            Some(r) => format!("R{}|G{:x}", r.scene.code(), r.guard),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = DefBook::new();
    // 1) 越场景必拒。
    got[0] = DefScene::from_code(6).err() == Some(DM_BAD_SCENE);
    // 2) 重行必拒。
    let _ = b.bind(DefScene::CopyOver, G_COPY);
    got[1] = b.bind(DefScene::CopyOver, G_COPY).err() == Some(DM_DUP_ROW);
    // 3) 未绑派发必拒。
    got[2] = b.dispatch(DefScene::FmtTrunc).err() == Some(DM_UNBOUND);
    // 4) 错派必判红（期望守卫与绑定不符；红账按基线差计，前况已入账不重算）。
    let reds_before = b.reds.len();
    got[3] = !b.verify_hit(DefScene::CopyOver, G_FMT) && b.reds.len() == reds_before + 1;
    // 5) 印账后绑定必抓。
    b.seal();
    got[4] = b.bind(DefScene::AppendOver, G_APPEND).err() == Some(DM_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12115_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12115");

    // J1 场景闭集。
    let j1 = DefScene::from_code(0).is_err()
        && DefScene::from_code(6).is_err()
        && DefScene::from_code(5).is_ok();
    s.add("J1-场景闭集", j1, "0 与 6 拒、5 过");

    // J2 五场景全绑定。
    let j2 = {
        let mut b = DefBook::new();
        let _ = b.bind(DefScene::CopyOver, G_COPY);
        let _ = b.bind(DefScene::AppendOver, G_APPEND);
        let _ = b.bind(DefScene::FmtTrunc, G_FMT);
        let _ = b.bind(DefScene::PrefixOob, G_PREFIX);
        let _ = b.bind(DefScene::IntNoRoom, G_INT);
        b.row_count() == 5 && b.summary().contains("rows=5")
    };
    s.add("J2-五绑全齐", j2, "矩阵满格");

    // J3 派发命中。
    let j3 = {
        let mut b = DefBook::new();
        let _ = b.bind(DefScene::CopyOver, G_COPY);
        let _ = b.bind(DefScene::AppendOver, G_APPEND);
        b.dispatch(DefScene::CopyOver) == Ok(G_COPY)
            && b.dispatch(DefScene::AppendOver) == Ok(G_APPEND)
    };
    s.add("J3-派发命中", j3, "场景回守卫");

    // J4 未绑派发拒且入红。
    let j4 = {
        let mut b = DefBook::new();
        let _ = b.bind(DefScene::CopyOver, G_COPY);
        b.dispatch(DefScene::IntNoRoom).err() == Some(DM_UNBOUND)
            && b.reds.len() == 1
    };
    s.add("J4-未绑拒", j4, "未绑场景必拒");

    // J5 错派判红。
    let j5 = {
        let mut b = DefBook::new();
        let _ = b.bind(DefScene::FmtTrunc, G_FMT);
        !b.verify_hit(DefScene::FmtTrunc, G_INT) && b.reds.len() == 1
    };
    s.add("J5-错派红", j5, "守卫失配入红账");

    // J6 正确命中核查。
    let j6 = {
        let mut b = DefBook::new();
        let _ = b.bind(DefScene::PrefixOob, G_PREFIX);
        b.verify_hit(DefScene::PrefixOob, G_PREFIX)
    };
    s.add("J6-命中核查", j6, "期望一致判真");

    // J7 守卫码面越域拒。
    let j7 = {
        let mut b = DefBook::new();
        b.bind(DefScene::CopyOver, 0x1234).err() == Some(DM_BAD_SCENE)
    };
    s.add("J7-守卫码面", j7, "越域守卫码必拒");

    // J8 重行拒。
    let j8 = {
        let mut b = DefBook::new();
        let _ = b.bind(DefScene::IntNoRoom, G_INT);
        b.bind(DefScene::IntNoRoom, G_INT).err() == Some(DM_DUP_ROW)
    };
    s.add("J8-重行拒", j8, "同场景二次必拒");

    // J9 行渲染。
    let j9 = {
        let mut b = DefBook::new();
        let _ = b.bind(DefScene::CopyOver, G_COPY);
        b.line_of(DefScene::CopyOver).contains("R1")
            && b.line_of(DefScene::FmtTrunc) == "missing"
    };
    s.add("J9-行渲染", j9, "绑定行可渲染");

    // J10 印账冻结。
    let j10 = {
        let mut b = DefBook::new();
        b.seal();
        b.bind(DefScene::CopyOver, G_COPY).err() == Some(DM_FROZEN)
            && b.dispatch(DefScene::CopyOver).err() == Some(DM_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后皆拒");

    // J11 指纹幂等。
    let j11 = {
        let mut b = DefBook::new();
        let _ = b.bind(DefScene::CopyOver, G_COPY);
        b.fold() == b.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账：三场景绑定。
    let mut b = DefBook::new();
    let _ = b.bind(DefScene::CopyOver, G_COPY);
    let _ = b.bind(DefScene::FmtTrunc, G_FMT);
    let _ = b.bind(DefScene::IntNoRoom, G_INT);

    s.add("摘要回读", b.summary().contains("rows=3 reds=0"), "三行无红");
    s.add("指纹非零", b.fold() != 0, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12115_matrix_flow() {
        let mut b = DefBook::new();
        assert!(b.bind(DefScene::CopyOver, G_COPY).is_ok());
        assert_eq!(b.bind(DefScene::CopyOver, G_COPY), Err(DM_DUP_ROW));
        assert_eq!(b.dispatch(DefScene::CopyOver), Ok(G_COPY));
        assert_eq!(b.dispatch(DefScene::FmtTrunc), Err(DM_UNBOUND));
    }

    #[test]
    fn unx12115_checks_green() {
        let s = run_unx12115_checks();
        assert!(s.all_passed(), "UNX-F12115 判据未全绿");
    }

    #[test]
    fn scene_codes() {
        assert_eq!(DefScene::CopyOver.code(), 1);
        assert_eq!(DefScene::IntNoRoom.code(), 5);
        assert_ne!(DefScene::FmtTrunc, DefScene::PrefixOob);
    }
}
