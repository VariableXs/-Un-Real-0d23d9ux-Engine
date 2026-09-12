//! 零基础拖拽修改（#365~#380，AI-05 域三）。
//!
//! 流程图的增删改全部落到确定性代码生成：拖方块=重排代码行，拖菱形=插 if，
//! 拖六边形=包循环。改完先出 diff 预览（#377），确认后才写回（三端走 C08 写回通道）。
//! 零 AI：不使用任何模型，全部为规则化变换。

use crate::checks::CheckSet;
use crate::learn::DiffRow;

// ────────────────── 拖拽视觉反馈（六阶段，规格表格逐条落地） ──────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragPhase {
    /// 开始拖拽
    Start,
    /// 拖拽中（经过可放置区）
    Dragging,
    /// 经过不可放置区
    Invalid,
    /// 释放成功
    Success,
    /// 释放失败
    Fail,
    /// 代码同步
    Sync,
}

/// 每个阶段的视觉参数（像素风音效由资产包提供，此处只给事件名）。
#[derive(Debug, Clone, PartialEq)]
pub struct DragFeedback {
    pub opacity: f64,
    pub scale: f64,
    pub border: &'static str,
    pub cursor: &'static str,
    pub sound: &'static str,
    pub anim_ms: f64,
}

pub const SYNC_HIGHLIGHT_MS: f64 = 2000.0;

pub fn feedback(phase: DragPhase) -> DragFeedback {
    match phase {
        DragPhase::Start => DragFeedback {
            opacity: 0.7,
            scale: 1.05,
            border: "shadow-deep",
            cursor: "grab",
            sound: "place",
            anim_ms: 0.0,
        },
        DragPhase::Dragging => DragFeedback {
            opacity: 0.9,
            scale: 1.05,
            border: "#34C759",
            cursor: "grabbing",
            sound: "",
            anim_ms: 0.0,
        },
        DragPhase::Invalid => DragFeedback {
            opacity: 0.9,
            scale: 1.05,
            border: "#FF3B30",
            cursor: "not-allowed",
            sound: "",
            anim_ms: 0.0,
        },
        DragPhase::Success => DragFeedback {
            opacity: 1.0,
            scale: 1.1,
            border: "#34C759",
            cursor: "default",
            sound: "place",
            anim_ms: 200.0,
        },
        DragPhase::Fail => DragFeedback {
            opacity: 1.0,
            scale: 1.0,
            border: "#FF3B30",
            cursor: "default",
            sound: "error",
            anim_ms: 300.0,
        },
        DragPhase::Sync => DragFeedback {
            opacity: 1.0,
            scale: 1.0,
            border: "#0A84FF",
            cursor: "default",
            sound: "",
            anim_ms: SYNC_HIGHLIGHT_MS,
        },
    }
}

// ───────────────────────── 流程图模型 ─────────────────────────

/// 方块=顺序语句，菱形=判断，六边形=循环。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Diamond,
    Hexagon,
}

/// 一个可被拖拽的图元。
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub id: usize,
    pub shape: Shape,
    pub label: String,
    pub code: String,
    /// #378 安全锁
    pub locked: bool,
    /// #368 出边（连线改分支）
    pub edges: Vec<usize>,
}

impl Block {
    pub fn rect(id: usize, label: &str, code: &str) -> Self {
        Block { id, shape: Shape::Rect, label: label.into(), code: code.into(), locked: false, edges: Vec::new() }
    }
}

// ───────────────────────── F365~F380 图操作 ─────────────────────────

#[derive(Debug, Clone)]
pub struct FlowGraph {
    pub blocks: Vec<Block>,
    next_id: usize,
    /// #376 撤销/重做：状态栈 + 游标
    history: Vec<Vec<Block>>,
    cursor: usize,
    /// #379 改前快照（大改前自动保存）
    pub snapshots: Vec<Vec<Block>>,
}

impl FlowGraph {
    pub fn new() -> Self {
        FlowGraph {
            blocks: Vec::new(),
            next_id: 1,
            history: vec![Vec::new()],
            cursor: 0,
            snapshots: Vec::new(),
        }
    }

    fn alloc_id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn push(&mut self, b: Block) -> usize {
        let id = b.id;
        self.blocks.push(b);
        if id >= self.next_id {
            self.next_id = id + 1;
        }
        id
    }

    pub fn get(&self, id: usize) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    pub fn index_of(&self, id: usize) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }

    /// 提交一步（供撤销/重做取用）：截断 redo 分支后压栈。
    pub fn commit(&mut self) {
        self.history.truncate(self.cursor + 1);
        self.history.push(self.blocks.clone());
        self.cursor += 1;
    }

    /// #376 撤销
    pub fn undo(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.blocks = self.history[self.cursor].clone();
        true
    }

    /// #376 重做
    pub fn redo(&mut self) -> bool {
        if self.cursor + 1 >= self.history.len() {
            return false;
        }
        self.cursor += 1;
        self.blocks = self.history[self.cursor].clone();
        true
    }

    /// #379 改前快照：大改前自动保存（阈值=一次改动超过 3 个块）
    pub fn snapshot_if_big(&mut self, affected: usize) -> bool {
        if affected >= 3 {
            self.snapshots.push(self.blocks.clone());
            true
        } else {
            false
        }
    }

    /// #365 拖拽改顺序：把 id 挪到 to 下标处（锁定块拒绝）。
    pub fn reorder(&mut self, id: usize, to: usize) -> bool {
        let from = match self.index_of(id) {
            Some(i) => i,
            None => return false,
        };
        if self.blocks[from].locked {
            return false;
        }
        let b = self.blocks.remove(from);
        let at = to.min(self.blocks.len());
        self.blocks.insert(at, b);
        self.commit();
        true
    }

    /// #366 拖拽加判断：在 at 处插菱形，生成 if 语句。
    pub fn insert_branch(&mut self, cond: &str, at: usize) -> usize {
        let id = self.alloc_id();
        let b = Block {
            id,
            shape: Shape::Diamond,
            label: format!("判断 {cond}"),
            code: format!("if ({cond}) {{"),
            locked: false,
            edges: Vec::new(),
        };
        let at = at.min(self.blocks.len());
        self.blocks.insert(at, b);
        self.commit();
        id
    }

    /// #367 拖拽加循环：六边形包住 ids，生成 for/while。
    pub fn wrap_loop(&mut self, ids: &[usize], count: usize) -> Option<usize> {
        if ids.is_empty() {
            return None;
        }
        let first = self.blocks.iter().position(|b| ids.contains(&b.id))?;
        let last = self.blocks.iter().rposition(|b| ids.contains(&b.id))?;
        if self.blocks[first..=last].iter().any(|b| b.locked) {
            return None;
        }
        let head = self.alloc_id();
        self.blocks.insert(
            first,
            Block {
                id: head,
                shape: Shape::Hexagon,
                label: format!("循环 {count} 次"),
                code: format!("for (let i = 0; i < {count}; i++) {{"),
                locked: false,
                edges: Vec::new(),
            },
        );
        let tail_id = self.alloc_id();
        let tail_at = (last + 2).min(self.blocks.len());
        self.blocks.insert(
            tail_at,
            Block {
                id: tail_id,
                shape: Shape::Rect,
                label: "循环结束".into(),
                code: "}".into(),
                locked: false,
                edges: Vec::new(),
            },
        );
        self.commit();
        Some(head)
    }

    /// #368 连线改分支：把 from 的出边目标由 old_to 换成 new_to。
    pub fn reconnect(&mut self, from: usize, old_to: usize, new_to: usize) -> bool {
        let b = match self.blocks.iter_mut().find(|b| b.id == from) {
            Some(b) => b,
            None => return false,
        };
        if b.locked {
            return false;
        }
        let mut changed = false;
        for e in b.edges.iter_mut() {
            if *e == old_to {
                *e = new_to;
                changed = true;
            }
        }
        if !changed {
            b.edges.push(new_to);
        }
        if changed || true {
            // 提交一次（changed 已在上面判定，保持语义清晰）
        }
        let ok = changed || true;
        if ok {
            // commit 需要 &mut self，edges 借用已结束
        }
        self.commit();
        changed || true
    }

    /// #369 删除节点 + 修复上下文（去掉悬空的闭合括号）。
    pub fn delete(&mut self, id: usize) -> bool {
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => return false,
        };
        if self.blocks[idx].locked {
            return false;
        }
        let had_open = self.blocks[idx].code.trim_end().ends_with('{');
        self.blocks.remove(idx);
        if had_open {
            // 找到紧随其后的第一个 "}" 一并移除，避免留下悬空闭合
            if let Some(p) = self.blocks.iter().position(|b| b.code.trim() == "}") {
                self.blocks.remove(p);
            }
        }
        // 修复：其它块不再指向已删除节点
        for b in self.blocks.iter_mut() {
            b.edges.retain(|e| *e != id);
        }
        self.commit();
        true
    }

    /// #370 复制粘贴：复制代码段并重命名变量（vars 中的标识符加后缀）。
    pub fn duplicate(&mut self, id: usize, vars: &[&str]) -> Option<usize> {
        let src = self.get(id)?.clone();
        let idx = self.index_of(id)?;
        let new_id = self.alloc_id();
        let mut copy = src.clone();
        copy.id = new_id;
        copy.label = format!("{} 副本", src.label);
        copy.code = rename_vars(&src.code, vars, "_2");
        copy.edges = Vec::new();
        self.blocks.insert(idx + 1, copy);
        self.commit();
        Some(new_id)
    }

    /// #378 安全锁：锁定后不可拖拽/不可编辑，代码不可改。
    pub fn lock(&mut self, id: usize) -> bool {
        if let Some(b) = self.blocks.iter_mut().find(|b| b.id == id) {
            b.locked = true;
            true
        } else {
            false
        }
    }

    pub fn unlock(&mut self, id: usize) -> bool {
        if let Some(b) = self.blocks.iter_mut().find(|b| b.id == id) {
            b.locked = false;
            true
        } else {
            false
        }
    }

    /// #373 一键加空值检查：在 id 前插绿色菱形 + 红色空值处理路径。
    pub fn add_null_check(&mut self, id: usize, var: &str) -> Option<usize> {
        let idx = self.index_of(id)?;
        let nid = self.alloc_id();
        self.blocks.insert(
            idx,
            Block {
                id: nid,
                shape: Shape::Diamond,
                label: format!("{var} 是否为空"),
                code: format!("if ({var} == null) {{"),
                locked: false,
                edges: Vec::new(),
            },
        );
        let hid = self.alloc_id();
        self.blocks.insert(
            idx + 1,
            Block {
                id: hid,
                shape: Shape::Rect,
                label: "空值处理".into(),
                code: format!("return Err(\"{var} 为空\");"),
                locked: false,
                edges: Vec::new(),
            },
        );
        let close_id = self.alloc_id();
        self.blocks.insert(
            idx + 2,
            Block {
                id: close_id,
                shape: Shape::Rect,
                label: "结束判断".into(),
                code: "}".into(),
                locked: false,
                edges: Vec::new(),
            },
        );
        self.commit();
        Some(nid)
    }

    /// #374 一键加日志：节点后插 📝 日志输出。
    pub fn add_log(&mut self, id: usize) -> Option<usize> {
        let idx = self.index_of(id)?;
        let label = self.blocks[idx].label.clone();
        let lid = self.alloc_id();
        self.blocks.insert(
            idx + 1,
            Block {
                id: lid,
                shape: Shape::Rect,
                label: format!("📝 记录日志"),
                code: format!("log(\"step: {label}\");"),
                locked: false,
                edges: Vec::new(),
            },
        );
        self.commit();
        Some(lid)
    }

    /// #375 一键加错误处理：选中区域被 try-catch 包裹。
    pub fn wrap_try(&mut self, ids: &[usize]) -> Option<usize> {
        if ids.is_empty() {
            return None;
        }
        let first = self.blocks.iter().position(|b| ids.contains(&b.id))?;
        let last = self.blocks.iter().rposition(|b| ids.contains(&b.id))?;
        let head = self.alloc_id();
        self.blocks.insert(
            first,
            Block {
                id: head,
                shape: Shape::Rect,
                label: "try".into(),
                code: "try {".into(),
                locked: false,
                edges: Vec::new(),
            },
        );
        let catch_id = self.alloc_id();
        let at = (last + 2).min(self.blocks.len());
        self.blocks.insert(
            at,
            Block {
                id: catch_id,
                shape: Shape::Rect,
                label: "catch".into(),
                code: "} catch (e) {".into(),
                locked: false,
                edges: Vec::new(),
            },
        );
        let close_at = (at + 1).min(self.blocks.len());
        let close_id = self.alloc_id();
        self.blocks.insert(
            close_at,
            Block {
                id: close_id,
                shape: Shape::Rect,
                label: "catch 结束".into(),
                code: "}".into(),
                locked: false,
                edges: Vec::new(),
            },
        );
        self.commit();
        Some(head)
    }

    /// 生成代码（顺序=块的顺序，即拖拽后的真实代码顺序）。
    pub fn to_code(&self) -> Vec<String> {
        self.blocks.iter().map(|b| b.code.clone()).collect()
    }
}

impl Default for FlowGraph {
    fn default() -> Self {
        FlowGraph::new()
    }
}

/// 标识符边界安全的重命名：只替换完整标识符，绝不替换子串。
pub fn rename_vars(code: &str, vars: &[&str], suffix: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let hit = vars.iter().any(|v| *v == word.as_str());
            out.push_str(&word);
            if hit {
                out.push_str(suffix);
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

// ───────────────────────── F371 填空编程 ─────────────────────────

/// 新建函数表单 → 生成函数骨架。
pub fn fill_in_form(name: &str, in_ty: &str, steps: &[&str], out_ty: &str, throws: &str) -> Vec<String> {
    let mut out = vec![format!("fn {name}(input: {in_ty}) -> {out_ty} {{")];
    for (i, s) in steps.iter().enumerate() {
        out.push(format!("  // 步骤{}：{}", i + 1, s));
        out.push(format!("  let _step{i} = todo!();"));
    }
    if !throws.trim().is_empty() {
        out.push(format!("  // 异常：{throws}"));
        out.push(format!("  return Err(\"{throws}\");"));
    } else {
        out.push("  return Ok(());".into());
    }
    out.push("}".into());
    out
}

// ───────────────────────── F372 模板拖入 ─────────────────────────

/// 模板库：拖入后展开为完整流程图 + 代码。
pub const TEMPLATES: &[(&str, &[&str])] = &[
    ("登录流程", &["检查参数", "查询用户", "校验密码", "签发 token"]),
    ("下单流程", &["校验库存", "创建订单", "扣减库存", "发起支付"]),
    ("重试流程", &["执行请求", "判断是否失败", "等待退避", "再次请求"]),
    ("导入流程", &["读取文件", "解析字段", "逐条校验", "批量入库"]),
];

/// 展开模板为块序列（id 从 1 连续分配，便于直接塞进图）。
pub fn expand_template(name: &str) -> Vec<Block> {
    let steps = TEMPLATES.iter().find(|(n, _)| *n == name).map(|(_, s)| *s);
    match steps {
        Some(s) => s
            .iter()
            .enumerate()
            .map(|(i, step)| Block::rect(i + 1, step, &format!("{step}();")))
            .collect(),
        None => Vec::new(),
    }
}

// ───────────────────────── F377 修改预览 ─────────────────────────

/// 改完流程图 → 右侧实时代码 diff（复用 #349 的 LCS 行级 diff）。
pub fn preview(before: &[String], after: &[String]) -> Vec<DiffRow> {
    let l: Vec<&str> = before.iter().map(|s| s.as_str()).collect();
    let r: Vec<&str> = after.iter().map(|s| s.as_str()).collect();
    crate::learn::diff_lines(&l, &r)
}

// ───────────────────────── F380 冲突检测 ─────────────────────────

/// 一次冲突：节点引用了未定义的变量，或改动破坏了既有引用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub node: usize,
    pub reason: String,
}

/// 扫描代码行：出现在赋值左侧的算已定义；其余被引用的标识符若从未定义则报冲突。
pub fn detect_conflicts(lines: &[String], ignore: &[&str]) -> Vec<Conflict> {
    let mut defined: Vec<String> = Vec::new();
    for l in lines {
        if let Some(p) = l.find(" = ") {
            let lhs = l[..p].trim();
            let name = lhs
                .trim_start_matches("let ")
                .trim_start_matches("const ")
                .trim();
            if !name.is_empty() && !defined.iter().any(|d| d == name) {
                defined.push(name.to_string());
            }
        }
    }
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        for w in ident_words(l) {
            if ignore.contains(&w.as_str()) {
                continue;
            }
            // 数字/浮点字面量不是标识符，跳过。
            if w.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
                continue;
            }
            if !defined.iter().any(|d| *d == w) && !is_keyword(&w) && !l.contains(&format!("{w} = ")) {
                out.push(Conflict {
                    node: i,
                    reason: format!("引用了未定义的 {w}"),
                });
                break;
            }
        }
    }
    out
}

fn ident_words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in line.chars() {
        if c.is_alphanumeric() || c == '_' {
            cur.push(c);
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn is_keyword(w: &str) -> bool {
    matches!(
        w,
        "let" | "const" | "fn" | "if" | "else" | "for" | "while" | "return" | "try"
            | "catch" | "Err" | "Ok" | "null" | "true" | "false" | "i" | "e" | "input"
    )
}

// ───────────────────────── 域自检 ─────────────────────────

/// #365~#380 自检（16 项）。
pub fn run_drag_checks() -> CheckSet {
    let mut s = CheckSet::new("drag");

    // F365 拖拽改顺序
    let mut g = FlowGraph::new();
    g.push(Block::rect(1, "A", "a();"));
    g.push(Block::rect(2, "B", "b();"));
    g.reorder(1, 1);
    let order1 = g.blocks.iter().map(|b| b.id).collect::<Vec<_>>();
    let code1 = g.to_code();
    s.add(
        "F365 拖拽改顺序",
        order1 == vec![2, 1] && code1 == vec!["b();".to_string(), "a();".to_string()],
        "A 拖到 B 下方→代码行重排",
    );

    // F366 拖拽加判断
    let bid = g.insert_branch("x > 0", 0);
    let head_code = g.to_code()[0].clone();
    s.add(
        "F366 拖拽加判断",
        g.get(bid).map(|b| b.shape) == Some(Shape::Diamond) && head_code == "if (x > 0) {",
        "插入 if 语句",
    );

    // F367 拖拽加循环
    let mut g2 = FlowGraph::new();
    g2.push(Block::rect(1, "A", "a();"));
    g2.push(Block::rect(2, "B", "b();"));
    let loop_head = g2.wrap_loop(&[1, 2], 3);
    let c2 = g2.to_code();
    s.add(
        "F367 拖拽加循环",
        loop_head.is_some() && c2[0] == "for (let i = 0; i < 3; i++) {" && c2.len() == 4 && c2[3] == "}",
        "六边形包住→for/while",
    );

    // F368 连线改分支
    let mut g3 = FlowGraph::new();
    g3.push(Block { id: 1, shape: Shape::Diamond, label: "d".into(), code: "if (a) {".into(), locked: false, edges: vec![2] });
    g3.push(Block::rect(2, "B", "b();"));
    g3.push(Block::rect(3, "C", "c();"));
    let rec = g3.reconnect(1, 2, 3);
    let edges_after = g3.get(1).map(|b| b.edges.clone()).unwrap_or_default();
    s.add(
        "F368 连线改分支",
        rec && edges_after == vec![3] && g3.get(1).map(|b| b.edges.contains(&2)) == Some(false),
        "箭头吸附到新目标",
    );

    // F369 删除节点
    let mut g4 = FlowGraph::new();
    g4.push(Block { id: 1, shape: Shape::Diamond, label: "d".into(), code: "if (a) {".into(), locked: false, edges: vec![2] });
    g4.push(Block::rect(2, "B", "b();"));
    g4.push(Block::rect(3, "end", "}"));
    let del = g4.delete(1);
    let left = g4.to_code();
    s.add(
        "F369 删除节点",
        del && left == vec!["b();".to_string()] && g4.get(1).is_none(),
        "删除代码+修复上下文",
    );

    // F370 复制粘贴
    let mut g5 = FlowGraph::new();
    g5.push(Block::rect(1, "A", "let x = x + 1;"));
    let new_id = g5.duplicate(1, &["x"]);
    let copied = g5.get(new_id.unwrap()).map(|b| b.code.clone()).unwrap_or_default();
    s.add(
        "F370 复制粘贴",
        new_id.is_some() && copied == "let x_2 = x_2 + 1;" && g5.blocks.len() == 2,
        "复制代码段+重命名变量",
    );

    // F371 填空编程
    let form = fill_in_form("login", "Req", &["查库", "校验"], "Token", "密码错误");
    s.add(
        "F371 填空编程",
        form[0] == "fn login(input: Req) -> Token {" && form.iter().any(|l| l.contains("步骤1：查库")) && form.iter().any(|l| l.contains("密码错误")),
        "表单→函数骨架",
    );

    // F372 模板拖入
    let tpl = expand_template("登录流程");
    s.add(
        "F372 模板拖入",
        tpl.len() == 4 && tpl[0].code == "检查参数();" && expand_template("不存在").is_empty(),
        "模板展开为完整流程图",
    );

    // F373 一键加检查
    let mut g6 = FlowGraph::new();
    g6.push(Block::rect(1, "use", "use(user);"));
    let nc = g6.add_null_check(1, "user");
    let codes6: Vec<String> = g6.to_code();
    s.add(
        "F373 一键加检查",
        nc.is_some() && codes6[0] == "if (user == null) {" && codes6[1].contains("user 为空") && codes6[2] == "}",
        "插入 if-null",
    );

    // F374 一键加日志
    let mut g7 = FlowGraph::new();
    g7.push(Block::rect(1, "A", "a();"));
    let lid = g7.add_log(1);
    s.add(
        "F374 一键加日志",
        lid.is_some() && g7.to_code()[1] == "log(\"step: A\");",
        "插入 print/log",
    );

    // F375 一键加错误处理
    let mut g8 = FlowGraph::new();
    g8.push(Block::rect(1, "A", "a();"));
    let th = g8.wrap_try(&[1]);
    let c8 = g8.to_code();
    s.add(
        "F375 一键加错误处理",
        th.is_some() && c8[0] == "try {" && c8[2] == "} catch (e) {" && c8[3] == "}",
        "包裹 try-catch",
    );

    // F376 撤销/重做
    let mut g9 = FlowGraph::new();
    g9.push(Block::rect(1, "A", "a();"));
    g9.commit();
    g9.push(Block::rect(2, "B", "b();"));
    g9.commit();
    let before_undo = g9.blocks.len();
    let undone = g9.undo();
    let after_undo = g9.blocks.len();
    let redone = g9.redo();
    s.add(
        "F376 撤销/重做",
        before_undo == 2 && undone && after_undo == 1 && redone && g9.blocks.len() == 2 && !g9.redo(),
        "Ctrl+Z/Y 代码同步撤销",
    );

    // F377 修改预览
    let before = vec!["a();".to_string(), "b();".to_string()];
    let after = vec!["a();".to_string(), "c();".to_string()];
    let rows = preview(&before, &after);
    let adds = rows.iter().filter(|r| matches!(r.kind, crate::learn::DiffKind::Add)).count();
    s.add(
        "F377 修改预览",
        adds == 1 && rows.iter().any(|r| matches!(r.kind, crate::learn::DiffKind::Del)),
        "实时代码 diff 预览",
    );

    // F378 安全锁
    let mut g10 = FlowGraph::new();
    g10.push(Block::rect(1, "A", "a();"));
    g10.push(Block::rect(2, "B", "b();"));
    g10.lock(1);
    let blocked = g10.reorder(1, 1);
    let order_locked = g10.blocks.iter().map(|b| b.id).collect::<Vec<_>>();
    let blocked_del = g10.delete(1);
    g10.unlock(1);
    let ok_after = g10.reorder(1, 1);
    s.add(
        "F378 安全锁",
        !blocked && order_locked == vec![1, 2] && !blocked_del && ok_after && g10.blocks[1].id == 1,
        "锁定后禁止拖拽/删除",
    );

    // F379 改前快照
    let mut g11 = FlowGraph::new();
    for i in 1..=4 {
        g11.push(Block::rect(i, &format!("N{i}"), &format!("n{i}();")));
    }
    let small = g11.snapshot_if_big(2);
    let big = g11.snapshot_if_big(4);
    s.add(
        "F379 改前快照",
        !small && big && g11.snapshots.len() == 1 && g11.snapshots[0].len() == 4,
        "大改前自动保存 AST 快照",
    );

    // F380 冲突检测
    let lines = vec!["let a = 1;".to_string(), "use(b);".to_string()];
    let conflicts = detect_conflicts(&lines, &["use"]);
    s.add(
        "F380 冲突检测",
        conflicts.len() == 1 && conflicts[0].node == 1 && conflicts[0].reason.contains("b"),
        "未定义引用→标红+警告气泡",
    );

    // 附则：六阶段拖拽视觉反馈
    let f_start = feedback(DragPhase::Start);
    let f_fail = feedback(DragPhase::Fail);
    let f_sync = feedback(DragPhase::Sync);
    s.add(
        "drag 拖拽视觉反馈六阶段",
        f_start.opacity == 0.7 && f_start.scale == 1.05 && f_fail.anim_ms == 300.0 && f_sync.anim_ms == SYNC_HIGHLIGHT_MS && feedback(DragPhase::Invalid).border == "#FF3B30",
        "开始/拖拽/禁止/成功/失败/同步",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f365_reorder_keeps_all_blocks() {
        let mut g = FlowGraph::new();
        g.push(Block::rect(1, "A", "a();"));
        g.push(Block::rect(2, "B", "b();"));
        g.push(Block::rect(3, "C", "c();"));
        assert!(g.reorder(3, 0));
        let ids: Vec<usize> = g.blocks.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![3, 1, 2]);
        assert!(!g.reorder(99, 0));
    }

    #[test]
    fn f370_rename_only_whole_identifiers() {
        assert_eq!(rename_vars("x + xy + x", &["x"], "_2"), "x_2 + xy + x_2");
        assert_eq!(rename_vars("abc", &["ab"], "_2"), "abc");
    }

    #[test]
    fn f376_undo_redo_branch_truncate() {
        let mut g = FlowGraph::new();
        g.push(Block::rect(1, "A", "a();"));
        g.commit();
        g.undo();
        g.push(Block::rect(2, "B", "b();"));
        g.commit();
        assert!(!g.redo());
        assert_eq!(g.blocks.len(), 1);
    }

    #[test]
    fn f369_delete_fixes_dangling_brace() {
        let mut g = FlowGraph::new();
        g.push(Block { id: 1, shape: Shape::Diamond, label: "d".into(), code: "if (a) {".into(), locked: false, edges: vec![] });
        g.push(Block::rect(2, "B", "b();"));
        g.push(Block::rect(3, "e", "}"));
        assert!(g.delete(1));
        assert_eq!(g.to_code(), vec!["b();".to_string()]);
    }

    #[test]
    fn f380_no_conflict_when_defined() {
        let lines = vec!["let b = 2;".to_string(), "use(b);".to_string()];
        assert!(detect_conflicts(&lines, &["use"]).is_empty());
    }
}
