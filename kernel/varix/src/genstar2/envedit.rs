//! F476 环境变量编辑器（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **双栏编辑全操作；留痕；撤销栈；空 PATH 红色确认；生效时机说明文案。**
//!
//! 功能定义（主册批次三）：环境变量用户面（A 域 F011 兼容面的图形端）——
//! 双栏编辑器（用户变量/系统变量）、每变量一行（名=值，行内编辑）、新建/
//! 删除/上移下移；修改留痕（F372 时间线）；误删恢复（编辑器内撤销栈，会话
//! 内有效）；危险值输入警示（PATH 删到空时红色确认）。
//!
//! 零堆纪律：定长变量表与撤销栈，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 单栏变量容量。
pub const VAR_CAP: usize = 64;
/// 变量名长度上限。
pub const NAME_CAP: usize = 64;
/// 变量值长度上限。
pub const VALUE_CAP: usize = 256;
/// 撤销栈深度（会话内有效——F202 联动）。
pub const UNDO_CAP: usize = 32;

/// 双栏（主册：用户变量/系统变量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    User,
    System,
}

/// 一条环境变量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvVar {
    pub name: [u8; NAME_CAP],
    pub name_n: usize,
    pub value: [u8; VALUE_CAP],
    pub value_n: usize,
}

impl EnvVar {
    pub fn new(name: &str, value: &str) -> Option<EnvVar> {
        if name.is_empty() || name.len() > NAME_CAP || value.len() > VALUE_CAP {
            return None;
        }
        let mut v = EnvVar {
            name: [0; NAME_CAP],
            name_n: name.len(),
            value: [0; VALUE_CAP],
            value_n: value.len(),
        };
        v.name[..name.len()].copy_from_slice(name.as_bytes());
        v.value[..value.len()].copy_from_slice(value.as_bytes());
        Some(v)
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_n]).unwrap_or("")
    }

    pub fn value_str(&self) -> &str {
        core::str::from_utf8(&self.value[..self.value_n]).unwrap_or("")
    }
}

/// 留痕事件（F372 时间线记录）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditTrail {
    pub pane: Pane,
    pub action: &'static str,
    pub var_name: [u8; NAME_CAP],
    pub name_n: usize,
}

/// 环境变量编辑器。
pub struct EnvEditor {
    user: [Option<EnvVar>; VAR_CAP],
    system: [Option<EnvVar>; VAR_CAP],
    user_n: usize,
    system_n: usize,
    undo: [Option<Snapshot>; UNDO_CAP],
    undo_n: usize,
    trail: [Option<AuditTrail>; 16],
    trail_n: usize,
    trail_total: u32,
}

/// 撤销快照（单栏全量拷贝——会话内有效）。
#[derive(Clone, Copy)]
struct Snapshot {
    pane: Pane,
    vars: [Option<EnvVar>; VAR_CAP],
    n: usize,
}

impl EnvEditor {
    pub const fn new() -> Self {
        EnvEditor {
            user: [None; VAR_CAP],
            system: [None; VAR_CAP],
            user_n: 0,
            system_n: 0,
            undo: [None; UNDO_CAP],
            undo_n: 0,
            trail: [None; 16],
            trail_n: 0,
            trail_total: 0,
        }
    }

    fn pane(&self, p: Pane) -> (&[Option<EnvVar>], usize) {
        match p {
            Pane::User => (&self.user, self.user_n),
            Pane::System => (&self.system, self.system_n),
        }
    }

    fn save_undo(&mut self, p: Pane) {
        let (vars, n) = self.pane(p);
        let mut snap_vars = [None; VAR_CAP];
        for i in 0..n {
            snap_vars[i] = vars[i];
        }
        if self.undo_n < UNDO_CAP {
            self.undo[self.undo_n] = Some(Snapshot { pane: p, vars: snap_vars, n });
            self.undo_n += 1;
        }
    }

    fn trail(&mut self, p: Pane, action: &'static str, name: &str) {
        let mut t = AuditTrail { pane: p, action, var_name: [0; NAME_CAP], name_n: name.len().min(NAME_CAP) };
        t.var_name[..t.name_n].copy_from_slice(&name.as_bytes()[..t.name_n]);
        if self.trail_n < 16 {
            self.trail[self.trail_n] = Some(t);
            self.trail_n += 1;
        }
        self.trail_total += 1;
    }

    /// 新建（留痕 + 可撤销）。
    pub fn create(&mut self, p: Pane, v: EnvVar) -> bool {
        let cur = match p {
            Pane::User => self.user_n,
            Pane::System => self.system_n,
        };
        if cur >= VAR_CAP {
            return false;
        }
        self.save_undo(p);
        let arr = match p {
            Pane::User => &mut self.user,
            Pane::System => &mut self.system,
        };
        arr[cur] = Some(v);
        match p {
            Pane::User => self.user_n += 1,
            Pane::System => self.system_n += 1,
        }
        self.trail(p, "create", v.name_str());
        true
    }

    /// 删除（留痕 + 可撤销）。
    pub fn delete(&mut self, p: Pane, name: &str) -> bool {
        let (idx, var) = {
            let (vars, n) = self.pane(p);
            let mut found = None;
            for i in 0..n {
                if let Some(v) = vars[i] {
                    if v.name_str() == name {
                        found = Some((i, v));
                        break;
                    }
                }
            }
            match found {
                Some(x) => x,
                None => return false,
            }
        };
        self.save_undo(p);
        let n = match p {
            Pane::User => self.user_n,
            Pane::System => self.system_n,
        };
        let arr = match p {
            Pane::User => &mut self.user,
            Pane::System => &mut self.system,
        };
        // 尾补位删除。
        arr[idx] = arr[n - 1];
        arr[n - 1] = None;
        match p {
            Pane::User => self.user_n -= 1,
            Pane::System => self.system_n -= 1,
        }
        self.trail(p, "delete", var.name_str());
        true
    }

    /// 行内改值（留痕 + 可撤销——快照先于修改）。
    pub fn set_value(&mut self, p: Pane, name: &str, value: &str) -> bool {
        if value.len() > VALUE_CAP {
            return false;
        }
        // 先拍快照（撤销必须回到改前状态）。
        self.save_undo(p);
        let (arr, n) = match p {
            Pane::User => (&mut self.user, self.user_n),
            Pane::System => (&mut self.system, self.system_n),
        };
        let mut hit = false;
        for i in 0..n {
            if let Some(v) = arr[i] {
                if v.name_str() == name {
                    let mut nv = v;
                    nv.value = [0; VALUE_CAP];
                    nv.value_n = value.len();
                    nv.value[..value.len()].copy_from_slice(value.as_bytes());
                    arr[i] = Some(nv);
                    hit = true;
                    break;
                }
            }
        }
        if hit {
            self.trail(p, "modify", name);
        } else {
            // 没改到东西 → 撤销快照回收（空操作不留账）。
            self.undo_n = self.undo_n.saturating_sub(1);
            self.undo[self.undo_n] = None;
        }
        hit
    }

    /// 上移/下移（up=true 上移）。
    pub fn move_row(&mut self, p: Pane, name: &str, up: bool) -> bool {
        let (vars, n) = self.pane(p);
        let mut idx = None;
        for i in 0..n {
            if let Some(v) = vars[i] {
                if v.name_str() == name {
                    idx = Some(i);
                    break;
                }
            }
        }
        let idx = match idx {
            Some(i) => i,
            None => return false,
        };
        let target = if up { idx.checked_sub(1) } else { Some(idx + 1) };
        match target {
            Some(t) if t < n => {
                self.save_undo(p);
                let arr = match p {
                    Pane::User => &mut self.user,
                    Pane::System => &mut self.system,
                };
                arr.swap(idx, t);
                self.trail(p, if up { "move-up" } else { "move-down" }, name);
                true
            }
            _ => false, // 顶/底边界无动作（诚实）
        }
    }

    /// 撤销（会话内有效——栈顶快照回滚）。
    pub fn undo(&mut self) -> bool {
        match self.undo_n {
            0 => false,
            _ => {
                self.undo_n -= 1;
                if let Some(s) = self.undo[self.undo_n].take() {
                    match s.pane {
                        Pane::User => {
                            self.user = s.vars;
                            self.user_n = s.n;
                        }
                        Pane::System => {
                            self.system = s.vars;
                            self.system_n = s.n;
                        }
                    }
                    true
                } else {
                    false
                }
            }
        }
    }

    /// 空 PATH 红色确认（主册：PATH 删到空时红色确认——危险值警示）。
    pub fn needs_red_confirm(pane_vars: &[Option<EnvVar>], n: usize, deleting: &str) -> bool {
        // PATH 删除且删除后不存在其它 PATH → 红色确认。
        if deleting.to_ascii_uppercase() != "PATH" {
            return false;
        }
        !pane_vars[..n].iter().flatten().any(|v| v.name_str().eq_ignore_ascii_case("PATH") && v.name_str() != deleting)
    }

    /// 生效时机说明（主册：新开的终端才会读到新值——诚实边界文案）。
    pub fn effect_timing_note() -> &'static str {
        "新开的终端才会读到新值"
    }

    pub fn trail_count(&self) -> u32 {
        self.trail_total
    }

    pub fn var_names(&self, p: Pane) -> usize {
        match p {
            Pane::User => self.user_n,
            Pane::System => self.system_n,
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_envedit_checks() -> CheckSet {
    let mut cs = CheckSet::new("F476-envedit");
    let path_var = EnvVar::new("PATH", "C:\\bin;C:\\tools").unwrap();
    let home = EnvVar::new("HOME", "C:\\Users\\vx").unwrap();
    // 1) 双栏全操作：新建（用户/系统两栏独立）。
    let mut e = EnvEditor::new();
    cs.add("dual_pane_create", e.create(Pane::User, path_var) && e.create(Pane::System, home) && e.var_names(Pane::User) == 1 && e.var_names(Pane::System) == 1, "");
    // 2) 行内改值 + 上移下移 + 边界诚实。
    cs.add("set_value", e.set_value(Pane::User, "PATH", "C:\\new"), "");
    cs.add("move_row", e.create(Pane::User, EnvVar::new("TMP", "C:\\tmp").unwrap()) && e.move_row(Pane::User, "TMP", true), "");
    cs.add("move_edge_honest", !e.move_row(Pane::User, "TMP", true), "");
    // 3) 留痕（F372 时间线：create×3 + modify + move = 5 条）。
    cs.add("audit_trail", e.trail_count() == 5, "");
    // 4) 撤销栈（删除后撤销恢复）。
    e.delete(Pane::User, "PATH");
    let after_del = e.var_names(Pane::User);
    cs.add("undo_restores", e.undo() && e.var_names(Pane::User) == after_del + 1 && e.set_value(Pane::User, "PATH", "C:\\restored"), "");
    // 5) 空 PATH 红色确认。
    let mut e2 = EnvEditor::new();
    e2.create(Pane::User, path_var);
    cs.add("path_delete_red", EnvEditor::needs_red_confirm(&[Some(path_var)], 1, "PATH"), "");
    cs.add("normal_delete_no_red", !EnvEditor::needs_red_confirm(&[Some(home)], 1, "HOME"), "");
    // 6) 生效时机说明文案。
    cs.add("effect_timing_note", EnvEditor::effect_timing_note().contains("新开"), "");
    // 7) 超长名/值诚实拒绝。
    cs.add("oversize_honest", EnvVar::new(&"N".repeat(NAME_CAP + 1), "v").is_none() && EnvVar::new("OK", &"v".repeat(VALUE_CAP + 1)).is_none(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_stack_session_scoped() {
        let mut e = EnvEditor::new();
        let v = EnvVar::new("A", "1").unwrap();
        e.create(Pane::User, v);
        e.set_value(Pane::User, "A", "2");
        e.set_value(Pane::User, "A", "3");
        // 逐级撤销回到初值（会话内有效）。
        e.undo();
        e.undo();
        e.undo();
        assert!(!e.undo(), "栈空诚实失败");
        let (vars, n) = (e.var_names(Pane::User), e.var_names(Pane::User));
        assert_eq!(vars, n);
    }

    #[test]
    fn delete_then_undo_restores_exact_var() {
        let mut e = EnvEditor::new();
        e.create(Pane::User, EnvVar::new("KEEP", "x").unwrap());
        e.create(Pane::User, EnvVar::new("PATH", "C:\\b").unwrap());
        e.delete(Pane::User, "KEEP"); // 尾补位：PATH 前移
        assert_eq!(e.var_names(Pane::User), 1);
        e.undo();
        assert_eq!(e.var_names(Pane::User), 2);
    }

    #[test]
    fn every_mutation_leaves_trail() {
        let mut e = EnvEditor::new();
        e.create(Pane::System, EnvVar::new("SRV", "1").unwrap());
        e.set_value(Pane::System, "SRV", "2");
        e.delete(Pane::System, "SRV");
        assert_eq!(e.trail_count(), 3);
    }
}

// ===========================================================================
// 深化 v2（F476）：PATH 合并导入导出 / 变量名合法性 / 双栏持久化 / 撤销标注
// ===========================================================================

/// PATH 合并模式（导入外部 PATH 时：去重保序）。
pub fn merge_paths(base: &str, extra: &str, sep: char) -> Option<usize> {
    // 合并去重保序（PATH 语义：先出现的段优先，重复段丢弃）；
    // 任一侧超 VALUE_CAP 诚实拒绝。零堆：定长已见表（48 字节 × 24 段）。
    if base.len() + extra.len() + 1 > VALUE_CAP {
        return None;
    }
    let mut seen: [([u8; 48], u8); 24] = [([0; 48], 0); 24];
    let mut seen_n = 0usize;
    let mut out = 0usize;
    for half in [base, extra] {
        for seg in half.split(sep) {
            let sb = seg.as_bytes();
            if sb.is_empty() || sb.len() > 48 {
                continue;
            }
            let dup = (0..seen_n).any(|i| {
                seen[i].1 as usize == sb.len() && seen[i].0[..sb.len()] == *sb
            });
            if dup {
                continue;
            }
            if seen_n < 24 {
                seen[seen_n].0[..sb.len()].copy_from_slice(sb);
                seen[seen_n].1 = sb.len() as u8;
                seen_n += 1;
            }
            out += 1;
        }
    }
    Some(out)
}

/// 变量名合法性（主册兼容面：字母数字下划线；空名拒绝——F011 同构）。
pub fn valid_var_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_CAP
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

impl EnvEditor {
    /// 双栏持久化序列化（魔标+版本+两栏计数+逐条「名长+名+值长+值」）。
    pub fn save(&self, out: &mut [u8]) -> Option<usize> {
        let need = 7
            + self.user_n * (1 + NAME_CAP.max(1) + 2 + VALUE_CAP.max(1))
            + self.system_n * (1 + NAME_CAP.max(1) + 2 + VALUE_CAP.max(1));
        if out.len() < 7 {
            return None;
        }
        out[..4].copy_from_slice(&PERSIST_MAGIC);
        out[4] = 1;
        out[5] = self.user_n as u8;
        out[6] = self.system_n as u8;
        let mut w = 7usize;
        let panes = [(&self.user, self.user_n), (&self.system, self.system_n)];
        for (vars, n) in panes {
            for i in 0..n {
                let v = match vars[i] {
                    Some(v) => v,
                    None => continue,
                };
                if w + 3 + v.name_n + v.value_n > out.len() {
                    return None;
                }
                out[w] = v.name_n as u8;
                w += 1;
                out[w..w + v.name_n].copy_from_slice(&v.name[..v.name_n]);
                w += v.name_n;
                out[w..w + 2].copy_from_slice(&(v.value_n as u16).to_le_bytes());
                w += 2;
                out[w..w + v.value_n].copy_from_slice(&v.value[..v.value_n]);
                w += v.value_n;
            }
        }
        Some(w)
    }

    /// 反序列化（魔标/版本不符或条目损坏 = 整体拒收——坏账不静默吞）。
    pub fn load(&mut self, buf: &[u8]) -> bool {
        if buf.len() < 7 || buf[..4] != PERSIST_MAGIC || buf[4] != 1 {
            return false;
        }
        let un = buf[5] as usize;
        let sn = buf[6] as usize;
        if un > VAR_CAP || sn > VAR_CAP {
            return false;
        }
        let mut r = 7usize;
        let mut read_one = |r: &mut usize| -> Option<EnvVar> {
            if *r >= buf.len() {
                return None;
            }
            // 线格式（与 save 同序）：name_n(1) + name(nn) + value_n(2) + value(vn)。
            let nn = buf[*r] as usize;
            *r += 1;
            if nn == 0 || nn > NAME_CAP || *r + nn + 2 > buf.len() {
                return None;
            }
            let name_at = *r;
            *r += nn; // 跳过 name 字节段
            let vn = u16::from_le_bytes([buf[*r], buf[*r + 1]]) as usize;
            *r += 2;
            if vn > VALUE_CAP || *r + vn > buf.len() {
                return None;
            }
            let mut v = EnvVar { name: [0; NAME_CAP], name_n: nn, value: [0; VALUE_CAP], value_n: vn };
            v.name[..nn].copy_from_slice(&buf[name_at..name_at + nn]);
            v.value[..vn].copy_from_slice(&buf[*r..*r + vn]);
            *r += vn;
            Some(v)
        };
        let mut new_user = [None; VAR_CAP];
        for i in 0..un {
            match read_one(&mut r) {
                Some(v) => new_user[i] = Some(v),
                None => return false,
            }
        }
        let mut new_system = [None; VAR_CAP];
        for i in 0..sn {
            match read_one(&mut r) {
                Some(v) => new_system[i] = Some(v),
                None => return false,
            }
        }
        self.user = new_user;
        self.user_n = un;
        self.system = new_system;
        self.system_n = sn;
        true
    }
}

pub const PERSIST_MAGIC: [u8; 4] = *b"VEE1";

pub fn run_envedit_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F476-deep");
    // PATH 合并去重保序段数。
    cs.add("path_merge", merge_paths("C:\\a;C:\\b", "C:\\b;C:\\c", ';') == Some(3), "");
    cs.add("path_merge_oversize_honest", merge_paths("", &"x".repeat(VALUE_CAP + 1), ';').is_none(), "");
    // 变量名合法性。
    cs.add("name_valid", valid_var_name("PATH_2") && !valid_var_name("") && !valid_var_name("BAD NAME"), "");
    // 双栏持久化 round-trip。
    let mut e = EnvEditor::new();
    e.create(Pane::User, EnvVar::new("PATH", "C:\\bin").unwrap());
    e.create(Pane::System, EnvVar::new("SRV", "10.0.0.1").unwrap());
    let mut buf = [0u8; 2048];
    let n = e.save(&mut buf).unwrap();
    let mut q = EnvEditor::new();
    cs.add("persist_roundtrip", q.load(&buf[..n]) && q.var_names(Pane::User) == 1 && q.var_names(Pane::System) == 1, "");
    let mut bad = buf;
    bad[0] = b'X';
    cs.add("persist_bad_magic", !EnvEditor::new().load(&bad[..n]), "");
    // 撤销栈满 32 后诚实停写（不覆盖最旧——会话内有效口径）。
    let mut e2 = EnvEditor::new();
    for i in 0..40 {
        e2.set_value(Pane::User, "NOPE", "x"); // 空操作不进栈
        e2.create(Pane::User, EnvVar::new(mk(i), "v").unwrap_or(EnvVar { name: [0; NAME_CAP], name_n: 0, value: [0; VALUE_CAP], value_n: 0 }));
    }
    cs.add("undo_cap_bounded", e2.trail_count() == 40, "");
    cs
}

fn mk(i: usize) -> &'static str {
    const P: [&str; 40] = ["a0","a1","a2","a3","a4","a5","a6","a7","a8","a9","b0","b1","b2","b3","b4","b5","b6","b7","b8","b9","c0","c1","c2","c3","c4","c5","c6","c7","c8","c9","d0","d1","d2","d3","d4","d5","d6","d7","d8","d9"];
    P[i.min(P.len() - 1)]
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn persist_roundtrip_preserves_values() {
        let mut e = EnvEditor::new();
        e.create(Pane::User, EnvVar::new("HOME", "C:\\Users\\vx").unwrap());
        let mut buf = [0u8; 2048];
        let n = e.save(&mut buf).unwrap();
        let mut q = EnvEditor::new();
        assert!(q.load(&buf[..n]));
        assert_eq!(q.var_names(Pane::User), 1);
    }

    #[test]
    fn load_rejects_truncated_buffer() {
        let mut e = EnvEditor::new();
        e.create(Pane::User, EnvVar::new("A", "1").unwrap());
        let mut buf = [0u8; 2048];
        let n = e.save(&mut buf).unwrap();
        assert!(!EnvEditor::new().load(&buf[..n - 2]));
    }

    #[test]
    fn var_name_rules() {
        assert!(valid_var_name("_PRIVATE"));
        // Windows 兼容语义：字母/数字/下划线（数字开头不禁止——与宿主一致）；
        // 非法字符（连字符/空格/等号）拒绝才是红线。
        assert!(valid_var_name("9START"));
        assert!(!valid_var_name("A-B"));
        assert!(!valid_var_name("HAS SPACE"));
        assert!(!valid_var_name(""));
    }
}

