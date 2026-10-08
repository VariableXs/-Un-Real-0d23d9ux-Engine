//! 多源码：本地项目 / 克隆仓库 / 分支切换 / 多项目切换 / 跨项目操作（#441~#445）—— AI-06 域四。
//!
//! 零 AI：仓库地址解析、过渡动画编排、跨项目检索全为确定性算法。
//! 三端等价：本地路径与 Git 地址在内核态经 VFS/网络能力走同一份加载计划（部署总纲第三章）。

/// 项目生长动画时长（ms）。
pub const GROW_MS: u32 = 600;
/// 旧项目退出动画时长（ms）。
pub const SHRINK_MS: u32 = 300;
/// 分支切换的画布过渡时长（ms）。
pub const BRANCH_TRANSITION_MS: u32 = 400;

/// 源码来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// 本地目录（/open）
    Local,
    /// Git 仓库（/clone）
    Git,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Local => "local",
            SourceKind::Git => "git",
        }
    }
}

/// 一个已登记的源码项目。
#[derive(Debug, Clone)]
pub struct ProjectSource {
    pub id: usize,
    pub name: String,
    pub path: String,
    /// 克隆地址（本地项目为 None）。
    pub url: Option<String>,
    pub kind: SourceKind,
    /// 当前分支（本地项目为 "-"）。
    pub branch: String,
    pub branches: Vec<String>,
    /// 文件名清单（跨项目检索/对比/复制的数据源）。
    pub files: Vec<String>,
}

/// 多项目工作区。
#[derive(Debug, Default)]
pub struct Workspace {
    pub projects: Vec<ProjectSource>,
    /// 当前激活项目 id。
    pub active: Option<usize>,
    seq: usize,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: usize) -> Option<&ProjectSource> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut ProjectSource> {
        self.projects.iter_mut().find(|p| p.id == id)
    }

    pub fn by_name(&self, name: &str) -> Option<&ProjectSource> {
        self.projects.iter().find(|p| p.name == name)
    }

    pub fn active_project(&self) -> Option<&ProjectSource> {
        self.active.and_then(|id| self.get(id))
    }

    pub fn names(&self) -> Vec<&str> {
        self.projects.iter().map(|p| p.name.as_str()).collect()
    }

    fn next_id(&mut self) -> usize {
        self.seq += 1;
        self.seq
    }

    fn push(&mut self, p: ProjectSource) -> usize {
        let id = p.id;
        self.projects.push(p);
        id
    }
}

// ------------------------------------------------------------------ F441 打开本地项目

/// 加载计划：文件选择器 → 加载 → 生长动画。
#[derive(Debug, Clone, PartialEq)]
pub struct LoadPlan {
    pub name: String,
    pub path: String,
    pub kind: SourceKind,
    /// 是否走了文件选择器（命令行带路径时跳过选择器直接加载）。
    pub via_picker: bool,
    pub grow_ms: u32,
    /// 生长关键帧 (毫秒, 阶段名)。
    pub stages: Vec<(u32, &'static str)>,
}

fn grow_stages() -> Vec<(u32, &'static str)> {
    vec![(0, "光点炸开"), (200, "逐级下钻"), (400, "稳定"), (GROW_MS, "完成")]
}

/// F441 `/open <路径>`：登记并生成加载计划。
/// 命令行已带路径 ⇒ `via_picker = false`（总纲第六章第 7 条：跳过选择器直接加载）。
pub fn open_local(ws: &mut Workspace, path: &str, via_picker: bool) -> Option<LoadPlan> {
    let path = path.trim().trim_end_matches(['/', '\\']);
    if path.is_empty() {
        return None;
    }
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if name.is_empty() {
        return None;
    }
    if let Some(p) = ws.by_name(name) {
        // 已打开过：直接激活，不重复登记
        let id = p.id;
        let kind = p.kind;
        ws.active = Some(id);
        return Some(LoadPlan {
            name: name.to_string(),
            path: path.to_string(),
            kind,
            via_picker: false,
            grow_ms: GROW_MS,
            stages: grow_stages(),
        });
    }
    let id = ws.next_id();
    let plan = LoadPlan {
        name: name.to_string(),
        path: path.to_string(),
        kind: SourceKind::Local,
        via_picker,
        grow_ms: GROW_MS,
        stages: grow_stages(),
    };
    ws.push(ProjectSource {
        id,
        name: name.to_string(),
        path: path.to_string(),
        url: None,
        kind: SourceKind::Local,
        branch: "-".to_string(),
        branches: Vec::new(),
        files: Vec::new(),
    });
    ws.active = Some(id);
    Some(plan)
}

// ------------------------------------------------------------------ F442 克隆 Git 仓库

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloneError {
    /// 地址格式非法
    BadUrl,
    /// 同名项目已存在
    AlreadyExists,
}

impl CloneError {
    pub fn as_str(self) -> &'static str {
        match self {
            CloneError::BadUrl => "bad-url",
            CloneError::AlreadyExists => "already-exists",
        }
    }
}

/// 从 Git 地址推导仓库名：`https://h/a/b.git` → `b`；`git@h:a/b.git` → `b`。
pub fn repo_name(url: &str) -> Option<String> {
    let u = url.trim();
    if u.is_empty() {
        return None;
    }
    // 先剥协议前缀得到「路径部分」，再取最后一段作为仓库名。
    let tail = if let Some(rest) = u.strip_prefix("git@") {
        rest.split(':').nth(1)?
    } else if let Some(rest) = u.strip_prefix("ssh://") {
        rest
    } else if u.starts_with("http://") || u.starts_with("https://") {
        u
    } else {
        return None;
    };
    let last = tail.trim_end_matches('/').rsplit('/').next()?;
    let name = last.trim_end_matches('/').trim_end_matches(".git");
    if name.is_empty() || name.contains(' ') {
        return None;
    }
    Some(name.to_string())
}

/// F442 `/clone <url>`：登记仓库并生成「下载 → 解析 → 生长」计划。
pub fn clone_repo(ws: &mut Workspace, url: &str) -> Result<LoadPlan, CloneError> {
    let name = repo_name(url).ok_or(CloneError::BadUrl)?;
    if ws.by_name(&name).is_some() {
        return Err(CloneError::AlreadyExists);
    }
    let id = ws.next_id();
    let mut stages = vec![(0u32, "下载"), (300u32, "解析")];
    stages.extend(grow_stages().into_iter().map(|(t, s)| (t + 300, s)));
    let plan = LoadPlan {
        name: name.clone(),
        path: format!("./{name}"),
        kind: SourceKind::Git,
        via_picker: false,
        grow_ms: GROW_MS,
        stages,
    };
    ws.push(ProjectSource {
        id,
        name,
        path: plan.path.clone(),
        url: Some(url.trim().to_string()),
        kind: SourceKind::Git,
        branch: "main".to_string(),
        branches: vec!["main".to_string()],
        files: Vec::new(),
    });
    ws.active = Some(id);
    Ok(plan)
}

// ------------------------------------------------------------------ F443/F444 过渡

/// 画布过渡动画。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// 旧项目：缩小淡出
    ShrinkFade { ms: u32 },
    /// 新项目：从中心生长
    GrowFromCenter { ms: u32 },
    /// 分支切换：画布过渡
    BranchSwitch { ms: u32 },
}

impl Transition {
    pub fn ms(self) -> u32 {
        match self {
            Transition::ShrinkFade { ms } | Transition::GrowFromCenter { ms } | Transition::BranchSwitch { ms } => ms,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Transition::ShrinkFade { .. } => "shrink-fade",
            Transition::GrowFromCenter { .. } => "grow-from-center",
            Transition::BranchSwitch { .. } => "branch-switch",
        }
    }
}

/// F443 `/branch <名称>`：切换分支（导航面板 Git 区选择 → 画布过渡）。
pub fn switch_branch(ws: &mut Workspace, id: usize, branch: &str) -> Option<Transition> {
    let p = ws.get_mut(id)?;
    if !p.branches.iter().any(|b| b == branch) {
        return None;
    }
    if p.branch == branch {
        return None;
    }
    p.branch = branch.to_string();
    Some(Transition::BranchSwitch { ms: BRANCH_TRANSITION_MS })
}

/// 登记分支（克隆/扫描所得）。
pub fn add_branch(ws: &mut Workspace, id: usize, branch: &str) -> bool {
    match ws.get_mut(id) {
        Some(p) => {
            if p.branches.iter().any(|b| b == branch) {
                return false;
            }
            p.branches.push(branch.to_string());
            true
        }
        None => false,
    }
}

/// F444 `/project switch <名称>`：旧项目缩小淡出 → 新项目从中心生长。
pub fn switch_project(ws: &mut Workspace, name: &str) -> Option<(Transition, Transition)> {
    let p = ws.by_name(name)?;
    let id = p.id;
    let was_active = ws.active;
    if was_active == Some(id) {
        return None;
    }
    ws.active = Some(id);
    Some((
        Transition::ShrinkFade { ms: SHRINK_MS },
        Transition::GrowFromCenter { ms: GROW_MS },
    ))
}

// ------------------------------------------------------------------ F445 跨项目操作

/// 跨项目来源配色：每个项目一色，结果按来源标注。
pub const SOURCE_COLORS: &[&str] = &[
    "#007AFF", "#34C759", "#FF3B30", "#FFAB00", "#AF52DE", "#5AC8FA", "#8E8E93", "#FF2D55",
];

/// 项目 → 颜色（按登记顺序稳定分配）。
pub fn source_color(ws: &Workspace, id: usize) -> &'static str {
    let idx = ws.projects.iter().position(|p| p.id == id).unwrap_or(0);
    SOURCE_COLORS[idx % SOURCE_COLORS.len()]
}

/// 跨项目搜索命中。
#[derive(Debug, Clone, PartialEq)]
pub struct CrossHit {
    pub project: String,
    pub file: String,
    pub color: &'static str,
}

/// F445 跨项目搜索：结果分颜色标注来源。
pub fn cross_search(ws: &Workspace, q: &str) -> Vec<CrossHit> {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for p in &ws.projects {
        for f in &p.files {
            if f.to_lowercase().contains(&q) {
                out.push(CrossHit {
                    project: p.name.clone(),
                    file: f.clone(),
                    color: source_color(ws, p.id),
                });
            }
        }
    }
    out
}

/// F445 跨项目对比：只在 A / 只在 B / 双方共有。
#[derive(Debug, Clone, Default)]
pub struct CompareResult {
    pub only_a: Vec<String>,
    pub only_b: Vec<String>,
    pub common: Vec<String>,
}

pub fn cross_compare(ws: &Workspace, a: usize, b: usize) -> Option<CompareResult> {
    let pa = ws.get(a)?;
    let pb = ws.get(b)?;
    let mut r = CompareResult::default();
    for f in &pa.files {
        if pb.files.contains(f) {
            r.common.push(f.clone());
        } else {
            r.only_a.push(f.clone());
        }
    }
    for f in &pb.files {
        if !pa.files.contains(f) && !r.only_a.contains(f) {
            r.only_b.push(f.clone());
        }
    }
    Some(r)
}

/// F445 跨项目复制：把 A 的文件登记到 B（去重）。
pub fn cross_copy(ws: &mut Workspace, from: usize, file: &str, to: usize) -> bool {
    let has = match ws.get(from) {
        Some(p) => p.files.iter().any(|f| f == file),
        None => false,
    };
    if !has || from == to {
        return false;
    }
    match ws.get_mut(to) {
        Some(p) => {
            if p.files.iter().any(|f| f == file) {
                return false;
            }
            p.files.push(file.to_string());
            true
        }
        None => false,
    }
}

/// 给项目登记文件清单（扫描结果落库，供跨项目检索/对比使用）。
pub fn set_files(ws: &mut Workspace, id: usize, files: &[&str]) -> bool {
    match ws.get_mut(id) {
        Some(p) => {
            p.files = files.iter().map(|f| f.to_string()).collect();
            true
        }
        None => false,
    }
}

// ------------------------------------------------------------------ 自检

/// AI-06 域四自检（#441~#445，5 项）。
pub fn run_multisrc_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("multisrc");

    // F441
    let mut ws = Workspace::new();
    let p1 = open_local(&mut ws, "D:/proj/demo", true).expect("打开本地项目");
    let dup = open_local(&mut ws, "D:/proj/demo", true).expect("重复打开");
    let bad = open_local(&mut ws, "   ", false);
    s.add(
        "F441 打开本地项目",
        p1.name == "demo"
            && p1.kind == SourceKind::Local
            && p1.via_picker
            && p1.grow_ms == 600
            && p1.stages.len() == 4
            && !dup.via_picker
            && ws.projects.len() == 1
            && ws.active == Some(1)
            && bad.is_none(),
        "文件选择器→加载→生长动画；命令行带路径时跳过选择器",
    );

    // F442
    let mut ws2 = Workspace::new();
    let c1 = clone_repo(&mut ws2, "https://github.com/acme/engine.git");
    let plan = c1.as_ref().expect("克隆成功");
    let stages_len = plan.stages.len();
    let err_dup = clone_repo(&mut ws2, "https://github.com/acme/engine.git").unwrap_err();
    let err_url = clone_repo(&mut ws2, "not-a-url").unwrap_err();
    s.add(
        "F442 克隆Git仓库",
        plan.name == "engine"
            && plan.kind == SourceKind::Git
            && stages_len == 6
            && ws2.get(1).map(|p| p.branch.as_str()) == Some("main")
            && err_dup == CloneError::AlreadyExists
            && err_url == CloneError::BadUrl
            && repo_name("git@github.com:acme/engine.git").as_deref() == Some("engine"),
        "输入URL→下载→解析→生长",
    );

    // F443
    let mut ws3 = Workspace::new();
    clone_repo(&mut ws3, "https://github.com/acme/engine.git").unwrap();
    add_branch(&mut ws3, 1, "dev");
    let t = switch_branch(&mut ws3, 1, "dev");
    let same = switch_branch(&mut ws3, 1, "dev");
    let missing = switch_branch(&mut ws3, 1, "nope");
    s.add(
        "F443 切换分支",
        t == Some(Transition::BranchSwitch { ms: 400 })
            && t.unwrap().ms() == 400
            && same.is_none()
            && missing.is_none()
            && ws3.get(1).map(|p| p.branch.as_str()) == Some("dev")
            && !add_branch(&mut ws3, 1, "dev"),
        "导航面板Git区→选择分支→画布过渡",
    );

    // F444
    let mut ws4 = Workspace::new();
    open_local(&mut ws4, "D:/a/alpha", false).unwrap();
    open_local(&mut ws4, "D:/b/beta", false).unwrap();
    let sw = switch_project(&mut ws4, "alpha");
    let (out_t, in_t) = sw.expect("切换成功");
    let active_name = ws4.active_project().map(|p| p.name.clone());
    let again = switch_project(&mut ws4, "beta");
    let none = switch_project(&mut ws4, "ghost");
    s.add(
        "F444 多项目切换",
        out_t == Transition::ShrinkFade { ms: 300 }
            && in_t == Transition::GrowFromCenter { ms: 600 }
            && out_t.ms() == 300
            && in_t.as_str() == "grow-from-center"
            && active_name.as_deref() == Some("alpha")
            && again.is_some()
            && none.is_none(),
        "旧项目缩小淡出→新项目从中心生长",
    );

    // F445
    let mut ws5 = Workspace::new();
    open_local(&mut ws5, "D:/a/alpha", false).unwrap();
    open_local(&mut ws5, "D:/b/beta", false).unwrap();
    set_files(&mut ws5, 1, &["src/main.rs", "src/net.rs", "README.md"]);
    set_files(&mut ws5, 2, &["src/main.rs", "src/ui.rs"]);
    let hits = cross_search(&ws5, "main");
    let cmp = cross_compare(&ws5, 1, 2).expect("对比");
    let copied = cross_copy(&mut ws5, 1, "src/net.rs", 2);
    let recopy = cross_copy(&mut ws5, 1, "src/net.rs", 2);
    let bad_copy = cross_copy(&mut ws5, 1, "nope.rs", 2);
    s.add(
        "F445 跨项目操作",
        hits.len() == 2
            && hits[0].color == "#007AFF"
            && hits[1].color == "#34C759"
            && cmp.common == vec!["src/main.rs".to_string()]
            && cmp.only_a == vec!["src/net.rs".to_string(), "README.md".to_string()]
            && cmp.only_b == vec!["src/ui.rs".to_string()]
            && copied
            && !recopy
            && !bad_copy,
        "搜索/对比/复制，结果分颜色标注来源",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f441_trailing_slash_normalized() {
        let mut ws = Workspace::new();
        let p = open_local(&mut ws, "D:/proj/demo/", false).unwrap();
        assert_eq!(p.name, "demo");
        assert_eq!(p.path, "D:/proj/demo");
    }

    #[test]
    fn f442_ssh_and_https_names() {
        assert_eq!(repo_name("https://h/a/b").as_deref(), Some("b"));
        assert_eq!(repo_name("ssh://git@h/a/b.git").as_deref(), Some("b"));
        assert!(repo_name("").is_none());
        assert!(repo_name("ftp://h/a").is_none());
    }

    #[test]
    fn f443_unknown_project() {
        let mut ws = Workspace::new();
        assert!(switch_branch(&mut ws, 9, "main").is_none());
        assert!(!add_branch(&mut ws, 9, "main"));
    }

    #[test]
    fn f444_switch_to_self_is_none() {
        let mut ws = Workspace::new();
        open_local(&mut ws, "D:/a/alpha", false).unwrap();
        assert!(switch_project(&mut ws, "alpha").is_none());
    }

    #[test]
    fn f445_empty_query_no_hits() {
        let mut ws = Workspace::new();
        open_local(&mut ws, "D:/a/alpha", false).unwrap();
        set_files(&mut ws, 1, &["a.rs"]);
        assert!(cross_search(&ws, "  ").is_empty());
        assert!(cross_compare(&ws, 1, 9).is_none());
    }
}
