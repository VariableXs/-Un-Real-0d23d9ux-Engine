//! hkbind — WP-202 · B-905 快捷键仲裁（MD2 篇 9.4）。
//!
//! 判据 B-905：冲突拒绝指名，词典唯一。
//! MD2 原文（9.4）："快捷键总表（交互词典的键盘侧）三列：保留字表（系统
//! 级：切换输入法、截图、交接入口、亮度音量）、应用申请表（应用 bind 时
//! 声明，冲突按先注册先得拒绝并指名，判例 26）、用户自定义表（设置中心，
//! 覆盖应用申请表）。仲裁顺序：用户自定义 > 保留字 > 应用申请。"
//! （IME 组合期分流见 composesw——B-904 在仲裁入口先行。）
//!
//! 宿主可测形态：三表固化（保留字四条 + 应用申请 + 用户自定义）+ 仲裁
//! 顺序（用户 > 保留 > 应用）+ 冲突**先注册先得拒绝并指名**（拒绝理由带
//! 占用者名——判例 26）+ **词典唯一**（同一 (scancode, mods) 生效绑定恰一条）。

use crate::checks::CheckSet;

/// 快捷键组合容量（应用申请表 + 用户自定义表）。
pub const BIND_CAP: usize = 16;

/// 修饰键位掩码（Ctrl/Alt/Shift 物理位模型）。
pub const MOD_CTRL: u8 = 1;
pub const MOD_ALT: u8 = 2;
pub const MOD_SHIFT: u8 = 4;

/// 绑定条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    pub scancode: u16,
    pub mods: u8,
    pub owner: &'static str,
}

/// 绑定来源（三列词典）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindSource {
    /// 用户自定义表（设置中心——最高优先）。
    User,
    /// 保留字表（系统级）。
    Reserved,
    /// 应用申请表。
    App,
}

/// 仲裁结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Arbitration {
    pub owner: &'static str,
    pub source: BindSource,
}

/// 保留字表（MD2 9.4 固化：切换输入法、截图、交接入口、亮度音量）。
pub const RESERVED: [Binding; 4] = [
    Binding { scancode: 57, mods: 0, owner: "切换输入法" },   // 空格模型位
    Binding { scancode: 88, mods: MOD_CTRL, owner: "截图" },  // F12 模型位
    Binding { scancode: 1, mods: MOD_ALT, owner: "交接入口" },
    Binding { scancode: 59, mods: 0, owner: "亮度音量" },
];

/// 快捷键词典：三列仲裁。
pub struct HotkeyDict {
    pub app_binds: [Option<Binding>; BIND_CAP],
    pub user_binds: [Option<Binding>; BIND_CAP],
    pub n_app: usize,
    pub n_user: usize,
}

impl HotkeyDict {
    pub const fn new() -> HotkeyDict {
        HotkeyDict { app_binds: [None; BIND_CAP], user_binds: [None; BIND_CAP], n_app: 0, n_user: 0 }
    }

    fn find_user(&self, sc: u16, mods: u8) -> Option<Binding> {
        self.user_binds.iter().take(self.n_user).find(|b| b.map(|b| b.scancode == sc && b.mods == mods).unwrap_or(false)).copied().flatten()
    }

    fn find_app(&self, sc: u16, mods: u8) -> Option<Binding> {
        self.app_binds.iter().take(self.n_app).find(|b| b.map(|b| b.scancode == sc && b.mods == mods).unwrap_or(false)).copied().flatten()
    }

    fn find_reserved(sc: u16, mods: u8) -> Option<Binding> {
        RESERVED.iter().find(|b| b.scancode == sc && b.mods == mods).copied()
    }

    /// 应用申请 bind：冲突**先注册先得拒绝并指名**。
    /// 冲突面：与保留字冲突、与已注册应用冲突、与用户自定义冲突。
    /// 返回 Err(占用者名)。
    pub fn app_bind(&mut self, sc: u16, mods: u8, owner: &'static str) -> Result<(), &'static str> {
        if let Some(b) = Self::find_reserved(sc, mods) {
            return Err(b.owner); // 指名：保留字占用
        }
        if let Some(b) = self.find_user(sc, mods) {
            return Err(b.owner); // 指名：用户自定义占用
        }
        if let Some(b) = self.find_app(sc, mods) {
            return Err(b.owner); // 指名：先注册的应用占用
        }
        if self.n_app >= BIND_CAP {
            return Err("词典满");
        }
        self.app_binds[self.n_app] = Some(Binding { scancode: sc, mods, owner });
        self.n_app += 1;
        Ok(())
    }

    /// 用户自定义：**覆盖应用申请表**（用户 > 应用），但与保留字冲突被拒
    /// （系统级功能不可被用户覆盖——保留字的宪法位）。
    pub fn user_bind(&mut self, sc: u16, mods: u8, owner: &'static str) -> Result<(), &'static str> {
        if let Some(b) = Self::find_reserved(sc, mods) {
            return Err(b.owner);
        }
        if self.n_user >= BIND_CAP {
            return Err("词典满");
        }
        self.user_binds[self.n_user] = Some(Binding { scancode: sc, mods, owner });
        self.n_user += 1;
        Ok(())
    }

    /// 仲裁：用户自定义 > 保留字 > 应用申请。
    pub fn arbitrate(&self, sc: u16, mods: u8) -> Option<Arbitration> {
        if let Some(b) = self.find_user(sc, mods) {
            return Some(Arbitration { owner: b.owner, source: BindSource::User });
        }
        if let Some(b) = Self::find_reserved(sc, mods) {
            return Some(Arbitration { owner: b.owner, source: BindSource::Reserved });
        }
        if let Some(b) = self.find_app(sc, mods) {
            return Some(Arbitration { owner: b.owner, source: BindSource::App });
        }
        None
    }

    /// 词典唯一性对账：生效绑定（用户 ∪ 保留 ∪ 应用，按优先级去重）无重复主键。
    pub fn dict_unique(&self) -> bool {
        // 应用表内部无重复主键（app_bind 冲突拒绝保证）——对账面复核
        for i in 0..self.n_app {
            for j in (i + 1)..self.n_app {
                let a = self.app_binds[i].unwrap();
                let b = self.app_binds[j].unwrap();
                if a.scancode == b.scancode && a.mods == b.mods {
                    return false;
                }
            }
        }
        for i in 0..self.n_user {
            for j in (i + 1)..self.n_user {
                let a = self.user_binds[i].unwrap();
                let b = self.user_binds[j].unwrap();
                if a.scancode == b.scancode && a.mods == b.mods {
                    return false;
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------- 对练

/// 仲裁对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct HkDrillSummary {
    pub rounds: u32,
    pub binds: u64,
    pub rejects: u64,
    /// 冲突拒绝必指名（Err 非空）
    pub reject_named: bool,
    /// 仲裁顺序正确（用户 > 保留 > 应用）
    pub order_correct: bool,
    /// 词典唯一
    pub unique: bool,
}

/// 随机 bind × 仲裁对练。
pub fn run_hk_drills(seed: u64, rounds: u32) -> HkDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = HkDrillSummary::default();
    sum.rounds = rounds;
    sum.reject_named = true;
    sum.order_correct = true;
    sum.unique = true;
    let mut dict = HotkeyDict::new();
    for i in 0..rounds {
        let sc = (g.next() % 128) as u16;
        let mods = (g.next() % 8) as u8;
        let owner = if g.next() % 2 == 0 { "appA" } else { "appB" };
        // 固定先注册一个应用绑定做冲突源（sc=10, mods=0 → "先注册者"）
        if i == 0 {
            let _ = dict.app_bind(10, 0, "先注册者");
        }
        match dict.app_bind(sc, mods, owner) {
            Ok(()) => {
                sum.binds += 1;
                // 注册成功后仲裁必须指回注册者（App 层）
                if let Some(a) = dict.arbitrate(sc, mods) {
                    let ok = match a.source {
                        BindSource::App => a.owner == owner,
                        _ => true, // 保留字/用户层更高（不发生在本对练的用户层）
                    };
                    if !ok {
                        sum.order_correct = false;
                    }
                }
            }
            Err(name) => {
                sum.rejects += 1;
                if name.is_empty() {
                    sum.reject_named = false;
                }
            }
        }
        if !dict.dict_unique() {
            sum.unique = false;
        }
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_hkbind_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-905 快捷键仲裁");
    {
        // 保留字表固化（系统级四条）
        set.add(
            "B-905 保留字表固化",
            RESERVED.len() == 4
                && RESERVED.iter().any(|b| b.owner == "切换输入法")
                && RESERVED.iter().any(|b| b.owner == "截图")
                && RESERVED.iter().any(|b| b.owner == "交接入口")
                && RESERVED.iter().any(|b| b.owner == "亮度音量"),
            "系统级：切换输入法/截图/交接入口/亮度音量（MD2 9.4）",
        );
    }
    {
        // 仲裁顺序：用户 > 保留 > 应用
        let mut dict = HotkeyDict::new();
        let _ = dict.app_bind(20, 0, "应用甲");
        let _ = dict.user_bind(20, 0, "用户乙");
        let a = dict.arbitrate(20, 0).unwrap();
        set.add(
            "B-905 仲裁顺序 用户>应用",
            a.source == BindSource::User && a.owner == "用户乙",
            "用户自定义表覆盖应用申请表",
        );
    }
    {
        // 保留字高于应用
        let mut dict = HotkeyDict::new();
        let r = dict.app_bind(RESERVED[1].scancode, RESERVED[1].mods, "应用甲");
        set.add(
            "B-905 保留字高于应用",
            r.is_err(),
            "应用申请与保留字冲突 → 拒绝",
        );
    }
    {
        // 冲突拒绝指名（判例 26）
        let mut dict = HotkeyDict::new();
        let _ = dict.app_bind(30, 0, "先注册者");
        let r = dict.app_bind(30, 0, "后申请者");
        set.add(
            "B-905 冲突拒绝指名",
            r == Err("先注册者"),
            "先注册先得拒绝并指名（判例 26）",
        );
    }
    {
        // 保留字冲突拒绝指名保留字名
        let mut dict = HotkeyDict::new();
        let r = dict.app_bind(88, MOD_CTRL, "应用甲");
        set.add(
            "B-905 保留字冲突指名",
            r == Err("截图"),
            "拒绝理由带占用者名",
        );
    }
    {
        // 用户绑定不可覆盖保留字
        let mut dict = HotkeyDict::new();
        let r = dict.user_bind(RESERVED[0].scancode, RESERVED[0].mods, "用户甲");
        set.add(
            "B-905 保留字不可覆盖",
            r == Err("切换输入法"),
            "系统级功能不被用户自定义覆盖（保留字的宪法位）",
        );
    }
    {
        // 词典唯一
        let mut dict = HotkeyDict::new();
        let _ = dict.app_bind(40, 0, "甲");
        let _ = dict.app_bind(41, 0, "乙");
        let _ = dict.user_bind(42, 0, "丙");
        set.add(
            "B-905 词典唯一",
            dict.dict_unique(),
            "同一 (scancode, mods) 生效绑定恰一条",
        );
    }
    {
        // 仲裁对练
        let sum = run_hk_drills(0xB905, 60);
        set.add(
            "B-905 快捷键仲裁对练",
            sum.rounds == 60 && sum.reject_named && sum.order_correct && sum.unique && sum.rejects > 0,
            "冲突拒绝指名，词典唯一（判据原文）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f805_arbitration_order() {
        let mut dict = HotkeyDict::new();
        let _ = dict.app_bind(20, 0, "应用甲");
        assert_eq!(dict.arbitrate(20, 0).unwrap().source, BindSource::App);
        let _ = dict.user_bind(20, 0, "用户乙");
        let a = dict.arbitrate(20, 0).unwrap();
        assert_eq!(a.source, BindSource::User);
        assert_eq!(a.owner, "用户乙");
    }

    #[test]
    fn f805_reject_named() {
        let mut dict = HotkeyDict::new();
        let _ = dict.app_bind(30, 0, "先注册者");
        assert_eq!(dict.app_bind(30, 0, "后申请者"), Err("先注册者"));
        assert_eq!(dict.app_bind(RESERVED[2].scancode, RESERVED[2].mods, "甲"), Err("交接入口"));
        assert_eq!(dict.user_bind(RESERVED[3].scancode, RESERVED[3].mods, "乙"), Err("亮度音量"));
    }

    #[test]
    fn f805_reserved_stable() {
        let dict = HotkeyDict::new();
        let a = dict.arbitrate(RESERVED[1].scancode, RESERVED[1].mods).unwrap();
        assert_eq!(a.source, BindSource::Reserved);
        assert_eq!(a.owner, "截图");
    }

    #[test]
    fn f805_drill_deterministic() {
        let a = run_hk_drills(11, 40);
        let b = run_hk_drills(11, 40);
        assert_eq!(a, b);
        assert!(a.reject_named && a.order_correct && a.unique);
    }
}
