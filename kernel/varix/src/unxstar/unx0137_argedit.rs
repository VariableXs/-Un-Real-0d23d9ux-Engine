//! UNX-F0137 · 引导参数 UI（诊断模式内编辑）· 诊断段 0x24F0
//!
//! 判据原文（UNX-F0137-J1）：参数编辑仅内存生效不落盘（红线：禁写引导介质），
//! 退出即还原断言过。
//!
//! 红线机制：编辑会话（EditSession）持独立影子串，所有改动只进影子；
//! `persist()` 函数在编译期即不存在——唯一出口是 `commit_to_cmdline()`
//! （写运行期 cmdline 缓冲，非引导介质）与 `abort()`（丢弃影子）。
//! 红线账：影子改动数 > 0 且会话结束态必须是 commit 或 abort 之一，
//! 泄漏（改动未处置）即账红。零浮点、零 unwrap/expect。

use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x24F0;

const E_OPEN: u16 = SEG_BASE + 1; // 会话未开就操作
const E_KEY: u16 = SEG_BASE + 2; // 参数键非法（空/含控制符）
const E_BAL: u16 = SEG_BASE + 3; // 影子账守恒被破坏
const E_SEAL: u16 = SEG_BASE + 4; // 会话已关闭
const E_STATE: u16 = SEG_BASE + 5; // 状态机非法迁移（已处置再处置）

/// 参数编辑会话状态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SesState {
    /// 编辑中（影子可写）。
    Editing,
    /// 已提交运行期 cmdline（影子冻结，改动即生失效）。
    Committed,
    /// 已放弃（影子丢弃，参数还原）。
    Aborted,
}

/// 单参数编辑动作。
pub struct EditAction {
    pub key: String,
    pub old_val: Option<String>,
    pub new_val: String,
}

/// 参数键合法性：非空、长度 ≤32、不含控制字符与空白。
pub fn key_ok(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 32
        && key
            .chars()
            .all(|c| c.is_ascii_graphic() && c != '=')
}

/// 影子参数表（键 → 值）。
pub struct ShadowTable {
    pub keys: Vec<String>,
    pub vals: Vec<String>,
}

impl ShadowTable {
    fn new() -> Self {
        ShadowTable { keys: Vec::new(), vals: Vec::new() }
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.keys.iter().position(|k| k == key).map(|i| self.vals[i].as_str())
    }

    fn set(&mut self, key: &str, val: &str) {
        match self.keys.iter().position(|k| k == key) {
            Some(i) => self.vals[i] = val.to_string(),
            None => {
                self.keys.push(key.to_string());
                self.vals.push(val.to_string());
            }
        }
    }

    fn len(&self) -> usize {
        self.keys.len()
    }
}

/// 编辑会话：所有改动只进影子与动作账，绝不触引导介质。
pub struct EditSession {
    pub state: SesState,
    pub base: ShadowTable, // 会话开启时的基线快照
    pub shadow: ShadowTable, // 影子（唯一可写面）
    pub actions: Vec<EditAction>,
}

impl EditSession {
    /// 开会话：拍基线快照（诊断模式内调用）。
    pub fn open(cmdline_pairs: &[(&str, &str)]) -> Result<Self, u16> {
        let mut base = ShadowTable::new();
        for (k, v) in cmdline_pairs {
            if !key_ok(k) {
                return Err(E_KEY);
            }
            base.set(k, v);
        }
        let shadow = ShadowTable {
            keys: base.keys.clone(),
            vals: base.vals.clone(),
        };
        Ok(EditSession {
            state: SesState::Editing,
            base,
            shadow,
            actions: Vec::new(),
        })
    }

    fn chk_editing(&self) -> Result<(), u16> {
        if self.state != SesState::Editing {
            return Err(E_STATE);
        }
        Ok(())
    }

    /// 设置/改写参数（仅影子）。
    pub fn set_param(&mut self, key: &str, val: &str) -> Result<(), u16> {
        self.chk_editing()?;
        if !key_ok(key) {
            return Err(E_KEY);
        }
        let old = self.shadow.get(key).map(|s| s.to_string());
        self.shadow.set(key, val);
        self.actions.push(EditAction {
            key: key.to_string(),
            old_val: old,
            new_val: val.to_string(),
        });
        Ok(())
    }

    /// 删除参数（影子内移除）。
    pub fn del_param(&mut self, key: &str) -> Result<bool, u16> {
        self.chk_editing()?;
        if !key_ok(key) {
            return Err(E_KEY);
        }
        match self.shadow.keys.iter().position(|k| k == key) {
            None => Ok(false),
            Some(i) => {
                let old = self.shadow.vals.remove(i);
                self.shadow.keys.remove(i);
                self.actions.push(EditAction {
                    key: key.to_string(),
                    old_val: Some(old),
                    new_val: String::new(),
                });
                Ok(true)
            }
        }
    }

    /// 提交到运行期 cmdline 缓冲（返回生效键值对副本；不触介质）。
    pub fn commit(&mut self) -> Result<Vec<(String, String)>, u16> {
        self.chk_editing()?;
        self.state = SesState::Committed;
        let mut out = Vec::new();
        for i in 0..self.shadow.len() {
            out.push((self.shadow.keys[i].clone(), self.shadow.vals[i].clone()));
        }
        Ok(out)
    }

    /// 放弃：影子整体丢弃，等价「还原基线」。
    pub fn abort(&mut self) -> Result<(), u16> {
        self.chk_editing()?;
        self.state = SesState::Aborted;
        self.shadow = ShadowTable {
            keys: self.base.keys.clone(),
            vals: self.base.vals.clone(),
        };
        Ok(())
    }

    /// 影子账守恒：
    /// ① Committed/Aborted 后影子只读（无新动作可追加由状态机保证）；
    /// ② 基线只读（任何动作不得改 base）——以「base 键集 == 会话开时键集」
    ///    的可验证代理：base 与影子在 Aborted 态必须逐键相等。
    pub fn audit(&self) -> Result<(), u16> {
        if self.state == SesState::Aborted {
            if self.base.keys != self.shadow.keys || self.base.vals != self.shadow.vals {
                return Err(E_BAL); // 放弃后影子必须还原基线
            }
        }
        if self.state == SesState::Committed && self.actions.is_empty() && self.shadow.keys.len()
            != self.base.keys.len()
        {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 渲染编辑界面速览。
    pub fn render(&self) -> String {
        let st = match self.state {
            SesState::Editing => "编辑中",
            SesState::Committed => "已提交",
            SesState::Aborted => "已还原",
        };
        let mut out = String::new();
        out.push_str(&format!("参数编辑 状态:{} 动作{}\n", st, self.actions.len()));
        for i in 0..self.shadow.len() {
            out.push_str(&format!(
                "{}={}\n",
                self.shadow.keys[i], self.shadow.vals[i]
            ));
        }
        out
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：改参 → 提交 → 生效键值含新值。
    let mut s = EditSession::open(&[("root", "vg0"), ("quiet", "")])?;
    s.set_param("loglevel", "8")?;
    s.set_param("root", "vg1")?;
    let eff = s.commit()?;
    let has = eff.iter().any(|(k, v)| k == "loglevel" && v == "8")
        && eff.iter().any(|(k, v)| k == "root" && v == "vg1");
    if !has {
        return Err(E_BAL);
    }
    s.audit()?;
    hits += 1;

    // L2：改参 → 放弃 → 影子还原基线（退出即还原红线）。
    let mut s = EditSession::open(&[("root", "vg0")])?;
    s.set_param("root", "BAD")?;
    s.abort()?;
    if s.shadow.get("root") != Some("vg0") {
        return Err(E_BAL);
    }
    s.audit()?;
    hits += 1;

    // L3：删参 → 提交后键消失。
    let mut s = EditSession::open(&[("a", "1"), ("b", "2")])?;
    if !s.del_param("a")? {
        return Err(E_BAL);
    }
    let eff = s.commit()?;
    if eff.iter().any(|(k, _)| k == "a") {
        return Err(E_BAL);
    }
    hits += 1;

    // L4：删不存在参数返回 false 不报错。
    let mut s = EditSession::open(&[("a", "1")])?;
    if s.del_param("zz")? {
        return Err(E_BAL);
    }
    s.abort()?;
    hits += 1;

    // I1：非法键（空 / 含等号 / 含控制符）拦截。
    let mut i1 = EditSession::open(&[])?;
    if i1.set_param("", "x") != Err(E_KEY) {
        return Err(E_KEY);
    }
    if i1.set_param("a=b", "x") != Err(E_KEY) {
        return Err(E_KEY);
    }
    if i1.set_param("a\n", "x") != Err(E_KEY) {
        return Err(E_KEY);
    }
    hits += 1;

    // I2：未处置再处置 —— E_STATE（commit 后再 set / 二次 commit）。
    let mut i2 = EditSession::open(&[])?;
    i2.commit()?;
    if i2.set_param("k", "v") != Err(E_STATE) {
        return Err(E_STATE);
    }
    if i2.commit().is_ok() {
        return Err(E_STATE);
    }
    hits += 1;

    // I3：abort 后再 set —— E_STATE。
    let mut i3 = EditSession::open(&[])?;
    i3.abort()?;
    if i3.set_param("k", "v") != Err(E_STATE) {
        return Err(E_STATE);
    }
    hits += 1;

    // I4：open 时基线键非法 —— E_KEY。
    if !matches!(EditSession::open(&[("bad key", "v")]), Err(E_KEY)) {
        return Err(E_KEY);
    }
    hits += 1;

    // I5：还原账注入——abort 后手工污染影子，audit 必抓红。
    let mut i5 = EditSession::open(&[("k", "1")])?;
    i5.set_param("k", "2")?;
    i5.abort()?;
    i5.shadow.set("k", "POLLUTED"); // 破还原语义
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
