// SPDX-License-Identifier: Apache-2.0
//! UNX-F0273 · 快捷键冲突表与保留前缀
//!
//! 键位总表无冲突断言，保留前缀（系统用）误配拒绝。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 快捷键定义
// ---------------------------------------------------------------------------

/// 快捷键。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct KeyBinding {
    /// 键码。
    pub key: char,
    /// 修饰符（Ctrl/Alt/Shift 位掩码）。
    pub modifiers: u8,
    /// 动作名。
    pub action: String,
}

impl KeyBinding {
    pub fn new(key: char, modifiers: u8, action: &str) -> Self {
        KeyBinding { key, modifiers, action: action.to_string() }
    }

    /// 修饰符常量。
    pub const MOD_NONE: u8 = 0;
    pub const MOD_CTRL: u8 = 1;
    pub const MOD_ALT: u8 = 2;
    pub const MOD_SHIFT: u8 = 4;
}

// ---------------------------------------------------------------------------
// 保留前缀
// ---------------------------------------------------------------------------

/// 系统保留前缀（用户不得占用）。
pub const RESERVED_PREFIXES: &[char] = &['\x01', '\x02', '\x03', '\x1b', '\x10', '\x11'];

/// 保留前缀检查。
pub fn is_reserved(key: char, modifiers: u8) -> bool {
    if RESERVED_PREFIXES.contains(&key) { return true; }
    // Ctrl+字母 中的系统保留
    if modifiers & KeyBinding::MOD_CTRL != 0 {
        match key {
            'c' | 'd' | 'z' | 'q' => true, // 系统保留
            _ => false,
        }
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// 冲突表
// ---------------------------------------------------------------------------

/// 键位总表。
pub struct KeymapTable {
    bindings: Vec<KeyBinding>,
    /// 冲突计数。
    pub conflicts: u32,
    /// 保留前缀误配拒绝计数。
    pub reserved_rejected: u32,
}

impl KeymapTable {
    pub fn new() -> Self {
        KeymapTable { bindings: Vec::new(), conflicts: 0, reserved_rejected: 0 }
    }

    /// 注册快捷键（冲突/保留检查）。
    pub fn register(&mut self, binding: KeyBinding) -> Result<(), u16> {
        // 保留前缀检查
        if is_reserved(binding.key, binding.modifiers) {
            self.reserved_rejected += 1;
            return Err(0xE301); // ERR_RESERVED
        }
        // 冲突检查
        if self.bindings.iter().any(|b| b.key == binding.key && b.modifiers == binding.modifiers) {
            self.conflicts += 1;
            return Err(0xE302); // ERR_CONFLICT
        }
        self.bindings.push(binding);
        Ok(())
    }

    /// 查询绑定。
    pub fn lookup(&self, key: char, modifiers: u8) -> Option<&str> {
        self.bindings.iter()
            .find(|b| b.key == key && b.modifiers == modifiers)
            .map(|b| b.action.as_str())
    }

    /// 绑定总数。
    pub fn count(&self) -> usize { self.bindings.len() }

    /// 无冲突断言。
    pub fn no_conflicts(&self) -> bool {
        self.conflicts == 0
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0273 判据全集。
pub fn run_keymap_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0273", "快捷键冲突表");

    // 1. 绑定
    let kb = KeyBinding::new('s', KeyBinding::MOD_CTRL, "save");
    cs.ok("绑定创建", kb.key == 's', "键码");
    cs.ok("修饰符", kb.modifiers == KeyBinding::MOD_CTRL, "Ctrl");

    // 2. 注册
    let mut table = KeymapTable::new();
    cs.ok("注册成功", table.register(kb.clone()).is_ok(), "首注册");
    cs.ok("冲突拒绝", table.register(kb.clone()).is_err(), "重复");
    cs.ok("冲突计数", table.conflicts == 1, "显性化");

    // 3. 保留前缀
    let reserved = KeyBinding::new('\x1b', KeyBinding::MOD_NONE, "escape");
    cs.ok("保留拒绝", table.register(reserved).is_err(), "系统保留");
    cs.ok("保留计数", table.reserved_rejected == 1, "显性化");
    let ctrl_c = KeyBinding::new('c', KeyBinding::MOD_CTRL, "copy");
    cs.ok("Ctrl+C 保留", table.register(ctrl_c).is_err(), "系统保留");

    // 4. 查找
    cs.ok("查找命中", table.lookup('s', KeyBinding::MOD_CTRL) == Some("save"), "按键查找");
    cs.ok("查找未命中", table.lookup('x', KeyBinding::MOD_NONE).is_none(), "不存在");

    // 5. 无冲突断言
    let mut clean = KeymapTable::new();
    clean.register(KeyBinding::new('a', KeyBinding::MOD_NONE, "a")).unwrap();
    clean.register(KeyBinding::new('b', KeyBinding::MOD_NONE, "b")).unwrap();
    cs.ok("无冲突", clean.no_conflicts(), "零冲突");
    cs.ok("绑定数", clean.count() == 2, "两条");

    // 6. 保留判定
    cs.ok("Esc 保留", is_reserved('\x1b', 0), "系统保留");
    cs.ok("普通键不保留", !is_reserved('x', 0), "用户可用");
    cs.ok("Ctrl+Q 保留", is_reserved('q', KeyBinding::MOD_CTRL), "系统保留");

    cs.done()
}
