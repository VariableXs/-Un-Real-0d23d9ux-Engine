//! 调试与测试（#337~#346，AI-05 域一）。
//!
//! 全部为确定性算法：插桩记录、类型边界枚举、确定性变异、core dump 文本解析、
//! 条件表达式求值——不依赖任何真实调试器，也不调用任何网络模型（零 AI）。
//! 三端等价：本域只产出纯数据（时间线/用例列表/场景布局/录像帧），
//! 由壳A/壳B/壳C 各自渲染，故 Windows / Variable / VARIX 行为一致。

use crate::checks::CheckSet;
use std::collections::HashMap;

// ───────────────────────── F337 变量值时间旅行 ─────────────────────────

/// 一次赋值的插桩记录：变量在 ts 时刻被写成 value。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueEvent {
    pub var: String,
    pub ts: u64,
    pub value: String,
}

/// 时间轴（底部滑块的数据源）：拖动即回看任意时刻的变量值。
#[derive(Debug, Default)]
pub struct ValueTimeline {
    pub events: Vec<ValueEvent>,
}

impl ValueTimeline {
    pub fn new() -> Self {
        ValueTimeline::default()
    }

    /// 插桩：记录一次赋值（按调用顺序入队，ts 单调非减）。
    pub fn record(&mut self, var: &str, ts: u64, value: &str) {
        self.events.push(ValueEvent {
            var: var.into(),
            ts,
            value: value.into(),
        });
    }

    /// 变量在 ts 时刻的值 = 不晚于 ts 的最后一次赋值。
    pub fn at(&self, var: &str, ts: u64) -> Option<&str> {
        self.events
            .iter()
            .filter(|e| e.var == var && e.ts <= ts)
            .max_by_key(|e| e.ts)
            .map(|e| e.value.as_str())
    }

    /// 变量全部历史（按时间升序），节点旁显示历史值用。
    pub fn history(&self, var: &str) -> Vec<(u64, String)> {
        let mut v: Vec<(u64, String)> = self
            .events
            .iter()
            .filter(|e| e.var == var)
            .map(|e| (e.ts, e.value.clone()))
            .collect();
        v.sort_by_key(|(t, _)| *t);
        v
    }

    /// 时间轴范围（最早 / 最晚事件）。
    pub fn range(&self) -> Option<(u64, u64)> {
        let lo = self.events.iter().map(|e| e.ts).min()?;
        let hi = self.events.iter().map(|e| e.ts).max()?;
        Some((lo, hi))
    }

    /// 拖动滑块：此刻全部变量的快照（用于画面实时刷新）。
    pub fn slider(&self, ts: u64) -> HashMap<String, String> {
        let mut names: Vec<&str> = Vec::new();
        for e in &self.events {
            if !names.contains(&e.var.as_str()) {
                names.push(e.var.as_str());
            }
        }
        let mut m = HashMap::new();
        for n in names {
            if let Some(v) = self.at(n, ts) {
                m.insert(n.to_string(), v.to_string());
            }
        }
        m
    }
}

// ───────────────────────── F338 边界值测试生成 ─────────────────────────

/// 参与边界枚举的基础类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimTy {
    Int,
    Uint,
    Float,
    Bool,
    Str,
    Array,
    Ptr,
}

/// 由类型名推断基础类型（未知按 Int 处理，保证确定性不报错）。
pub fn prim_of(type_name: &str) -> PrimTy {
    let t = type_name.trim().to_lowercase();
    match t.as_str() {
        "u8" | "u16" | "u32" | "u64" | "usize" | "uint" | "unsigned" => PrimTy::Uint,
        "f32" | "f64" | "float" | "double" => PrimTy::Float,
        "bool" | "boolean" => PrimTy::Bool,
        "str" | "string" | "char*" | "text" => PrimTy::Str,
        "vec" | "list" | "array" | "slice" | "[]" => PrimTy::Array,
        "ptr" | "*" | "ref" | "&" | "pointer" => PrimTy::Ptr,
        _ => PrimTy::Int,
    }
}

/// 边界值用例（输入 → 预期），确定性枚举，不做随机采样。
pub fn boundary_cases(ty: PrimTy) -> Vec<(String, String)> {
    match ty {
        PrimTy::Int => vec![
            ("0".into(), "正常".into()),
            ("1".into(), "正常".into()),
            ("-1".into(), "边界：负数".into()),
            ("2147483647".into(), "边界：上溢临界".into()),
            ("-2147483648".into(), "边界：下溢临界".into()),
        ],
        PrimTy::Uint => vec![
            ("0".into(), "边界：下溢临界".into()),
            ("1".into(), "正常".into()),
            ("4294967295".into(), "边界：上溢临界".into()),
        ],
        PrimTy::Float => vec![
            ("0.0".into(), "正常".into()),
            ("-0.0".into(), "边界：负零".into()),
            ("1e308".into(), "边界：接近上溢".into()),
            ("nan".into(), "边界：NaN 传播".into()),
        ],
        PrimTy::Bool => vec![("true".into(), "走真分支".into()), ("false".into(), "走假分支".into())],
        PrimTy::Str => vec![
            ("\"\"".into(), "边界：空串".into()),
            ("\"a\"".into(), "正常".into()),
            ("\"\\u{4e2d}\\u{6587}\"".into(), "边界：多字节".into()),
            ("null".into(), "边界：空引用".into()),
        ],
        PrimTy::Array => vec![
            ("[]".into(), "边界：空集合".into()),
            ("[1]".into(), "边界：单元素".into()),
            ("[1,2,3]".into(), "正常".into()),
        ],
        PrimTy::Ptr => vec![
            ("null".into(), "边界：空指针".into()),
            ("&x".into(), "正常".into()),
            ("dangling".into(), "边界：悬垂".into()),
        ],
    }
}

/// 按参数签名 `("count", "i32")` 批量生成用例表。
pub fn boundary_table(params: &[(&str, &str)]) -> Vec<(String, String, Vec<(String, String)>)> {
    params
        .iter()
        .map(|(name, ty)| (name.to_string(), ty.to_string(), boundary_cases(prim_of(ty))))
        .collect()
}

// ───────────────────────── F339 变异测试 ─────────────────────────

/// 一个变异体：原始片段 → 变异片段，由确定性算子生成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mutant {
    pub id: usize,
    pub op: &'static str,
    pub original: String,
    pub mutated: String,
}

/// 变异算子的确定性替换表（源码文本级，零随机）。
const MUTATION_OPS: [(&str, &str, &str); 8] = [
    ("+", "-", "arith"),
    ("-", "+", "arith"),
    ("*", "/", "arith"),
    (">", ">=", "relational"),
    ("<", "<=", "relational"),
    ("==", "!=", "equality"),
    ("true", "false", "boolean"),
    ("&&", "||", "logical"),
];

/// 对一段源码生成全部适用变异体（每种算子至多一处，确定性不重复）。
pub fn mutate(src: &str) -> Vec<Mutant> {
    let mut out = Vec::new();
    let mut id = 0usize;
    for (from, to, op) in MUTATION_OPS {
        if let Some(pos) = src.find(from) {
            let mutated = format!("{}{}{}", &src[..pos], to, &src[pos + from.len()..]);
            out.push(Mutant {
                id,
                op,
                original: src.to_string(),
                mutated,
            });
            id += 1;
        }
    }
    out
}

/// 单个变异体的判定结果：绿✓=被测试捕获，红✗=存活。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationResult {
    pub id: usize,
    pub killed: bool,
}

/// 变异得分 = 被杀 / 总数（百分比，整数）。
pub fn mutation_score(results: &[MutationResult]) -> u32 {
    if results.is_empty() {
        return 100;
    }
    let killed = results.iter().filter(|r| r.killed).count();
    ((killed * 100) / results.len()) as u32
}

/// 汇总：总数 / 被杀 / 存活。
pub fn mutation_summary(results: &[MutationResult]) -> (usize, usize, usize) {
    let total = results.len();
    let killed = results.iter().filter(|r| r.killed).count();
    (total, killed, total - killed)
}

// ───────────────────────── F340 崩溃 3D 重建 ─────────────────────────

/// 一个堆栈帧：3D 场景里的一层楼。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    pub func: String,
    pub file: String,
    pub line: u32,
    /// 局部变量 = 房间（名 → 值）
    pub vars: Vec<(String, String)>,
    /// 指针 = 走廊（名 → 目标）
    pub ptrs: Vec<(String, String)>,
}

/// 崩溃现场：帧序列 + 崩溃点索引。
#[derive(Debug, Clone, Default)]
pub struct CrashScene {
    pub frames: Vec<StackFrame>,
    pub crash_index: usize,
}

/// 解析 core dump 文本（形如 `#0 foo at main.rs:12` + 缩进的 `x = 1` / `p -> 0x10`）。
pub fn parse_core_dump(dump: &str) -> CrashScene {
    let mut scene = CrashScene::default();
    for raw in dump.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if line.trim_start().starts_with('#') {
            // #0 foo at main.rs:12
            let body = line.trim_start().trim_start_matches('#');
            let num: String = body.chars().take_while(|c| c.is_ascii_digit()).collect();
            let rest = body[num.len()..].trim();
            let idx = num.parse::<usize>().unwrap_or(scene.frames.len());
            let (func, file, ln) = split_frame(rest);
            if idx == scene.frames.len() {
                scene.frames.push(StackFrame {
                    func,
                    file,
                    line: ln,
                    vars: Vec::new(),
                    ptrs: Vec::new(),
                });
            }
        } else if let Some(frame) = scene.frames.last_mut() {
            let t = line.trim();
            if let Some(p) = t.find("->") {
                let k = t[..p].trim().to_string();
                let v = t[p + 2..].trim().to_string();
                frame.ptrs.push((k, v));
            } else if let Some(p) = t.find('=') {
                let k = t[..p].trim().to_string();
                let v = t[p + 1..].trim().to_string();
                frame.vars.push((k, v));
            }
        }
    }
    // 崩溃点 = 最内层帧（#0），即栈顶
    scene.crash_index = 0;
    scene
}

fn split_frame(rest: &str) -> (String, String, u32) {
    let (func, loc) = match rest.find(" at ") {
        Some(p) => (rest[..p].trim().to_string(), rest[p + 4..].trim().to_string()),
        None => (rest.to_string(), String::new()),
    };
    let (file, ln) = match loc.rfind(':') {
        Some(p) => (
            loc[..p].to_string(),
            loc[p + 1..].parse::<u32>().unwrap_or(0),
        ),
        None => (loc, 0),
    };
    (func, file, ln)
}

/// 3D 布局：帧=楼层（y 自下而上），返回 (层号, 高度, 函数名)。
pub fn scene_layout(scene: &CrashScene) -> Vec<(usize, f64, String)> {
    const FLOOR_H: f64 = 40.0;
    scene
        .frames
        .iter()
        .enumerate()
        .map(|(i, f)| (i, (scene.frames.len() - 1 - i) as f64 * FLOOR_H, f.func.clone()))
        .collect()
}

/// 崩溃点（红色爆炸处）。
pub fn crash_point(scene: &CrashScene) -> Option<&StackFrame> {
    scene.frames.get(scene.crash_index)
}

// ───────────────────────── F341 逻辑断点 ─────────────────────────

/// 条件断点：节点旁红色圆点，悬停可编辑条件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakpoint {
    pub id: usize,
    pub node: usize,
    pub cond: String,
    pub hits: u32,
}

/// 断点会话（自带去重：同节点同条件只登记一次）。
#[derive(Debug, Default)]
pub struct BreakpointSet {
    pub bps: Vec<Breakpoint>,
}

impl BreakpointSet {
    pub fn new() -> Self {
        BreakpointSet::default()
    }

    /// 登记断点；已存在则原样返回其 id（不重复计数）。
    pub fn add(&mut self, node: usize, cond: &str) -> usize {
        if let Some(b) = self.bps.iter().find(|b| b.node == node && b.cond == cond) {
            return b.id;
        }
        let id = self.bps.len();
        self.bps.push(Breakpoint {
            id,
            node,
            cond: cond.into(),
            hits: 0,
        });
        id
    }

    pub fn remove(&mut self, id: usize) -> bool {
        let n = self.bps.len();
        self.bps.retain(|b| b.id != id);
        self.bps.len() != n
    }

    /// 命中判定：条件成立则命中并累加计数。
    pub fn hit(&mut self, id: usize, ctx: &HashMap<String, i64>) -> bool {
        let Some(b) = self.bps.iter_mut().find(|b| b.id == id) else {
            return false;
        };
        let ok = eval_cond(&b.cond, ctx);
        if ok {
            b.hits += 1;
        }
        ok
    }

    pub fn hits(&self, id: usize) -> u32 {
        self.bps.iter().find(|b| b.id == id).map(|b| b.hits).unwrap_or(0)
    }

    pub fn on_node(&self, node: usize) -> Vec<&Breakpoint> {
        self.bps.iter().filter(|b| b.node == node).collect()
    }
}

/// 条件求值：支持 `x > 5` / `x>=3` / `flag` / `true`。确定性，缺变量按 0。
pub fn eval_cond(cond: &str, ctx: &HashMap<String, i64>) -> bool {
    let c = cond.trim();
    if c.is_empty() {
        return true; // 无条件断点 = 每次都命中
    }
    if c == "true" {
        return true;
    }
    if c == "false" {
        return false;
    }
    for op in [">=", "<=", "==", "!=", ">", "<"] {
        if let Some(p) = c.find(op) {
            let lhs = c[..p].trim();
            let rhs = c[p + op.len()..].trim();
            let lv = ctx.get(lhs).copied().unwrap_or(0);
            let rv = rhs.parse::<i64>().unwrap_or(0);
            return match op {
                ">" => lv > rv,
                "<" => lv < rv,
                ">=" => lv >= rv,
                "<=" => lv <= rv,
                "==" => lv == rv,
                _ => lv != rv,
            };
        }
    }
    ctx.get(c).copied().unwrap_or(0) != 0
}

// ───────────────────────── F342 执行录像 ─────────────────────────

/// 录像的一帧：某时刻执行到某节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordFrame {
    pub ts: u64,
    pub node: usize,
    pub note: String,
}

/// 录制器。
#[derive(Debug, Default)]
pub struct Recorder {
    pub frames: Vec<RecordFrame>,
}

impl Recorder {
    pub fn new() -> Self {
        Recorder::default()
    }

    pub fn record(&mut self, ts: u64, node: usize, note: &str) {
        self.frames.push(RecordFrame {
            ts,
            node,
            note: note.into(),
        });
    }

    pub fn duration(&self) -> u64 {
        match (self.frames.first().map(|f| f.ts), self.frames.last().map(|f| f.ts)) {
            (Some(a), Some(b)) => b - a,
            _ => 0,
        }
    }
}

/// 播放状态（▶⏸⏹ + 进度滑块 + 速度 0.5x/1x/2x）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayState {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug)]
pub struct Player<'a> {
    pub rec: &'a Recorder,
    pub pos_ms: f64,
    pub speed: f64,
    pub state: PlayState,
}

impl<'a> Player<'a> {
    pub fn new(rec: &'a Recorder) -> Self {
        Player {
            rec,
            pos_ms: 0.0,
            speed: 1.0,
            state: PlayState::Stopped,
        }
    }

    pub fn play(&mut self) {
        self.state = PlayState::Playing;
    }
    pub fn pause(&mut self) {
        self.state = PlayState::Paused;
    }
    pub fn stop(&mut self) {
        self.state = PlayState::Stopped;
        self.pos_ms = 0.0;
    }

    /// 速度档位白名单，防止非法倍速。
    pub fn set_speed(&mut self, s: f64) -> bool {
        if [0.5f64, 1.0, 2.0].contains(&s) {
            self.speed = s;
            true
        } else {
            false
        }
    }

    /// 推进 dt_ms 真实时间；返回当前帧索引（受倍速影响）。
    pub fn step(&mut self, dt_ms: f64) -> Option<usize> {
        if self.state != PlayState::Playing {
            return self.frame_at_pos();
        }
        self.pos_ms += dt_ms * self.speed;
        let dur = self.rec.duration() as f64;
        if self.pos_ms >= dur {
            self.pos_ms = dur;
            self.state = PlayState::Paused;
        }
        self.frame_at_pos()
    }

    fn frame_at_pos(&self) -> Option<usize> {
        let base = self.rec.frames.first()?.ts as f64;
        let t = base + self.pos_ms;
        self.rec
            .frames
            .iter()
            .rposition(|f| (f.ts as f64) <= t)
            .or(Some(0))
    }
}

// ───────────────────────── F343 数据流动画 ─────────────────────────

/// 数据球：从输入节点滚入函数节点，出来时变色。
#[derive(Debug, Clone, PartialEq)]
pub struct DataParticle {
    pub id: usize,
    pub from: usize,
    pub to: usize,
    pub progress: f64,
    pub color: &'static str,
}

pub const PARTICLE_IN: &str = "#0A84FF";
pub const PARTICLE_OUT: &str = "#30D158";

/// 生成一颗数据球（进度 0）。
pub fn spawn_particle(id: usize, from: usize, to: usize) -> DataParticle {
    DataParticle {
        id,
        from,
        to,
        progress: 0.0,
        color: PARTICLE_IN,
    }
}

/// 推进 dt_ms；速度恒定（1000ms 走完全程）。到达后变色。
pub fn advance_particle(p: &mut DataParticle, dt_ms: f64) -> bool {
    p.progress = (p.progress + dt_ms / 1000.0).min(1.0);
    if p.progress >= 1.0 {
        p.color = PARTICLE_OUT;
        return true;
    }
    false
}

// ───────────────────────── F344 代码变漫画 ─────────────────────────

/// 一格漫画：每个 if 分支 = 一格。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComicPanel {
    pub index: usize,
    pub title: String,
    pub ok: bool,
}

/// 分支→分镜：`if` 的 then 为「成功✅」，else 为「失败❌」。
pub fn to_comic(src: &str) -> Vec<ComicPanel> {
    let mut out = Vec::new();
    for raw in src.lines() {
        let t = raw.trim();
        if t.starts_with("if ") || t.starts_with("if(") {
            let cond = t
                .trim_start_matches("if")
                .trim()
                .trim_start_matches('(')
                .trim_end_matches(") {")
                .trim_end_matches('{')
                .trim()
                .to_string();
            let i = out.len();
            out.push(ComicPanel {
                index: i,
                title: format!("成功✅ 如果{cond}"),
                ok: true,
            });
            out.push(ComicPanel {
                index: i + 1,
                title: format!("失败❌ 如果不{cond}"),
                ok: false,
            });
        }
    }
    out
}

// ───────────────────────── F345 逻辑变地铁图 ─────────────────────────

/// 地铁站：函数=站点，分支=换乘站。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Station {
    pub name: String,
    pub line: usize,
    pub transfer: bool,
}

/// 调用图 → 地铁图：主干线路取调用链，分支函数标为换乘站。
pub fn to_metro(
    lines: &[&[&str]],
    branches: &[&str],
) -> (Vec<Station>, Vec<(String, String, usize)>) {
    let mut stations = Vec::new();
    let mut edges = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        for w in line.windows(2) {
            edges.push((w[0].to_string(), w[1].to_string(), li));
        }
        for name in *line {
            if !stations.iter().any(|s: &Station| s.name == *name) {
                stations.push(Station {
                    name: name.to_string(),
                    line: li,
                    transfer: false,
                });
            }
        }
    }
    for b in branches {
        if let Some(s) = stations.iter_mut().find(|s| s.name == *b) {
            s.transfer = true;
        }
    }
    (stations, edges)
}

// ───────────────────────── F346 代码变拼图 ─────────────────────────

/// 拼图形状：输入/输出类型各映射为一个确定的凹凸齿数（0~7）。
pub fn puzzle_shape(in_ty: &str, out_ty: &str) -> (u8, u8) {
    fn gear(ty: &str) -> u8 {
        let mut h = 0u32;
        for b in ty.trim().to_lowercase().bytes() {
            h = h.wrapping_mul(31).wrapping_add(b as u32);
        }
        (h % 8) as u8
    }
    (gear(in_ty), gear(out_ty))
}

/// 只有类型匹配才能拼合：前一块的输出齿 == 后一块的输入齿。
pub fn puzzle_fits(a: (u8, u8), b: (u8, u8)) -> bool {
    a.1 == b.0
}

// ───────────────────────── 域自检 ─────────────────────────

/// #337~#346 自检（10 项 + 附则）。
pub fn run_debug_checks() -> CheckSet {
    let mut s = CheckSet::new("debug");

    // F337 变量值时间旅行
    let mut tl = ValueTimeline::new();
    tl.record("x", 10, "1");
    tl.record("x", 20, "2");
    tl.record("y", 30, "9");
    let at15 = tl.at("x", 15).map(|v| v.to_string());
    let at25 = tl.at("x", 25).map(|v| v.to_string());
    let hist = tl.history("x").len();
    let snap20 = tl.slider(20).len();
    s.add(
        "F337 变量值时间旅行",
        at15.as_deref() == Some("1") && at25.as_deref() == Some("2") && hist == 2 && snap20 == 1 && tl.range() == Some((10, 30)),
        "时间轴滑块回看历史值",
    );

    // F338 边界值测试生成
    let ints = boundary_cases(PrimTy::Int);
    let strs = boundary_cases(prim_of("String"));
    s.add(
        "F338 边界值测试生成",
        ints.len() == 5 && ints.iter().any(|(i, _)| i == "2147483647") && strs.iter().any(|(i, _)| i == "null") && boundary_table(&[("n", "i32")]).len() == 1,
        "类型约束枚举用例表",
    );

    // F339 变异测试
    let ms = mutate("a > b");
    let has_ge = ms.iter().any(|m| m.mutated == "a >= b");
    let results = vec![
        MutationResult { id: 0, killed: true },
        MutationResult { id: 1, killed: false },
    ];
    let (total, killed, survived) = mutation_summary(&results);
    s.add(
        "F339 变异测试",
        !ms.is_empty() && has_ge && total == 2 && killed == 1 && survived == 1 && mutation_score(&results) == 50,
        "确定性变异+绿✓红✗判定",
    );

    // F340 崩溃3D重建
    let dump = "#0 foo at main.rs:12\n  x = 1\n  p -> 0x10\n#1 bar at lib.rs:3\n";
    let scene = parse_core_dump(dump);
    let layout = scene_layout(&scene);
    let top = crash_point(&scene).map(|f| f.func.clone());
    let rooms = scene.frames[0].vars.len();
    let corridors = scene.frames[0].ptrs.len();
    s.add(
        "F340 崩溃3D重建",
        scene.frames.len() == 2 && top.as_deref() == Some("foo") && rooms == 1 && corridors == 1 && layout[0].1 > layout[1].1,
        "帧=楼层/变量=房间/指针=走廊",
    );

    // F341 逻辑断点
    let mut bps = BreakpointSet::new();
    let id0 = bps.add(1, "x > 5");
    let dup = bps.add(1, "x > 5");
    let mut ctx = HashMap::new();
    ctx.insert("x".to_string(), 9i64);
    let hit_ok = bps.hit(id0, &ctx);
    let mut ctx2 = HashMap::new();
    ctx2.insert("x".to_string(), 1i64);
    let hit_no = bps.hit(id0, &ctx2);
    let hits = bps.hits(id0);
    s.add(
        "F341 逻辑断点",
        id0 == dup && bps.bps.len() == 1 && hit_ok && !hit_no && hits == 1 && eval_cond("", &ctx),
        "条件插桩+去重登记",
    );

    // F342 执行录像
    let mut rec = Recorder::new();
    rec.record(0, 1, "start");
    rec.record(500, 2, "mid");
    rec.record(1000, 3, "end");
    let mut pl = Player::new(&rec);
    pl.play();
    let spd_bad = pl.set_speed(3.0);
    let spd_ok = pl.set_speed(2.0);
    let f0 = pl.step(100.0); // pos=200ms → 帧0
    let f1 = pl.step(200.0); // pos=600ms → 帧1
    let f2 = pl.step(200.0); // pos=1000ms → 帧2（到末尾自动暂停）
    let state_at_end = pl.state;
    s.add(
        "F342 执行录像",
        rec.duration() == 1000 && !spd_bad && spd_ok && f0 == Some(0) && f1 == Some(1) && f2 == Some(2) && state_at_end == PlayState::Paused,
        "播放条+0.5x/1x/2x 倍速",
    );

    // F343 数据流动画
    let mut p = spawn_particle(0, 1, 2);
    let c_in = p.color;
    let halfway = p.progress;
    let done = advance_particle(&mut p, 1000.0);
    let c_out = p.color;
    s.add(
        "F343 数据流动画",
        halfway == 0.0 && done && c_in == PARTICLE_IN && c_out == PARTICLE_OUT && p.progress == 1.0,
        "数据球滚入→变色",
    );

    // F344 代码变漫画
    let comic = to_comic("if (a) {\n  f()\n}\n");
    let ok_panel = comic.first().map(|c| (c.ok, c.title.contains("成功✅")));
    let bad_panel = comic.get(1).map(|c| (c.ok, c.title.contains("失败❌")));
    s.add(
        "F344 代码变漫画",
        comic.len() == 2 && ok_panel == Some((true, true)) && bad_panel == Some((false, true)),
        "分支→分镜 成功✅/失败❌",
    );

    // F345 逻辑变地铁图
    let (stations, edges) = to_metro(&[&["main", "auth", "db"], &["main", "log"]], &["auth"]);
    let transfer = stations.iter().filter(|st| st.transfer).count();
    s.add(
        "F345 逻辑变地铁图",
        stations.len() == 4 && edges.len() == 3 && transfer == 1 && stations.iter().any(|st| st.name == "main"),
        "函数=站点/调用=线路/分支=换乘",
    );

    // F346 代码变拼图
    let a = puzzle_shape("int", "int");
    let b = puzzle_shape("int", "bool");
    let c = puzzle_shape("bool", "int");
    s.add(
        "F346 代码变拼图",
        a.1 == b.0 && puzzle_fits(a, b) && !puzzle_fits(a, c) && a == puzzle_shape("int", "int"),
        "类型决定凹凸，匹配才能拼合",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f337_timeline_last_write_wins() {
        let mut tl = ValueTimeline::new();
        tl.record("v", 1, "a");
        tl.record("v", 5, "b");
        tl.record("v", 9, "c");
        assert_eq!(tl.at("v", 0), None);
        assert_eq!(tl.at("v", 4), Some("a"));
        assert_eq!(tl.at("v", 9), Some("c"));
        assert_eq!(tl.at("v", 100), Some("c"));
    }

    #[test]
    fn f338_boundary_enum_is_deterministic() {
        let a = boundary_cases(PrimTy::Array);
        let b = boundary_cases(PrimTy::Array);
        assert_eq!(a, b);
        assert_eq!(a[0].0, "[]");
        assert_eq!(prim_of("usize"), PrimTy::Uint);
        assert_eq!(prim_of("f64"), PrimTy::Float);
    }

    #[test]
    fn f339_mutation_ops_cover_relational() {
        let m = mutate("a < b");
        assert!(m.iter().any(|x| x.mutated == "a <= b"));
        assert!(m.iter().all(|x| x.original == "a < b"));
        let ids: Vec<usize> = m.iter().map(|x| x.id).collect();
        let uniq: std::collections::HashSet<usize> = ids.iter().copied().collect();
        assert_eq!(ids.len(), uniq.len());
    }

    #[test]
    fn f340_frame_order_and_crash_top() {
        let scene = parse_core_dump("#0 a at x.rs:1\n#1 b at y.rs:2\n#2 c at z.rs:3\n");
        assert_eq!(scene.frames.len(), 3);
        assert_eq!(crash_point(&scene).unwrap().func, "a");
        let l = scene_layout(&scene);
        assert_eq!(l[0].0, 0);
        assert!(l[0].1 > l[2].1);
    }

    #[test]
    fn f341_breakpoint_dedup_and_remove() {
        let mut bps = BreakpointSet::new();
        let a = bps.add(7, "n == 0");
        let b = bps.add(7, "n == 0");
        assert_eq!(a, b);
        assert_eq!(bps.bps.len(), 1);
        assert!(bps.remove(a));
        assert!(!bps.remove(a));
        let mut ctx = HashMap::new();
        ctx.insert("n".to_string(), 0i64);
        assert!(eval_cond("n == 0", &ctx));
        assert!(!eval_cond("n != 0", &ctx));
    }

    #[test]
    fn f342_player_speed_and_stop() {
        let mut rec = Recorder::new();
        rec.record(0, 1, "a");
        rec.record(2000, 2, "b");
        let mut pl = Player::new(&rec);
        pl.play();
        pl.set_speed(2.0);
        pl.step(500.0);
        assert_eq!(pl.pos_ms, 1000.0);
        pl.stop();
        assert_eq!(pl.pos_ms, 0.0);
        assert_eq!(pl.state, PlayState::Stopped);
    }

    #[test]
    fn f346_puzzle_gear_stable() {
        let s1 = puzzle_shape("Vec<u8>", "Result");
        let s2 = puzzle_shape("Vec<u8>", "Result");
        assert_eq!(s1, s2);
        assert!(s1.0 < 8 && s1.1 < 8);
    }
}
