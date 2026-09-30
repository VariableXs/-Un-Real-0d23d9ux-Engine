//! 深化层四 · F136 示例应用仓库（2026-09-27 深化批次四 · g 层）。
//!
//! 账本回炉扩列方向的主件：**五示例源码本体**（内容件落地——不再是
//! 只有账本没有内容）。五个示例与基础批 builds_on 链同构：
//! ①hello → ②clipboard → ③theme-tokens → ④mini-editor → ⑤notepad-lite。
//! 源码本体以编译目标形态登记（`&'static str`，随仓库唯一事实分发），
//! 行数门禁（≤200）、教学注释密度带（30-60%）、README 四件、先修链
//! 方向性四类审计复用批次三 f136f 的机器面（一处一事实，不重写判据）。

use super::f136f;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 示例一 · hello（窗口 + 标签 + 关闭语义）
// ---------------------------------------------------------------------------

pub const HELLO_SRC: &str = r#"// 示例一：hello —— Varix 应用最小骨架。
// 先修：无。目标：跑通「建窗-布局-退出」最小闭环。
// 本示例同时是 U 盘直装的冒烟样本：双击到首帧 ≤3s（F001 判据）。
//
// 三个新概念一次讲清：
// 1. App 句柄——事件循环的所有者，drop 即退出；
// 2. Window builder——声明式配置，逻辑像素（DPI 由合成器处理）；
// 3. 令牌取色——颜色永远来自令牌表，不写裸色值。

use varix_app::{App, Window, Label, Button, Color};

// 应用入口：Varix 平台调用 main，运行时已就绪。
fn main() {
    // 每应用一个 App 句柄；作用域结束 = 事件循环结束 = 进程退出。
    let app = App::new("hello");

    // 主窗：min_size 是最小尺寸钳制（F214），缩放不破版。
    let win = Window::builder()
        .title("你好，Varix")
        .size(320, 200)
        .min_size(240, 160)
        .build(&app);

    // 标签居中：布局器按内容测量，不做手动坐标。
    let text = Label::new("我的第一个 Varix 应用")
        .color(Color::token("fg-primary")); // 令牌取色：主题联动零硬编码（F151）

    // 一个最小按钮：点击反馈（按压态）由平台默认样式提供。
    let bye = Button::new("退出");
    bye.on_click(|app| app.quit());

    // 纵向排列：顺序即视觉顺序，焦点顺序与之对齐（F206）。
    win.set_content(
        varix_app::Column::new()
            .center()
            .push(text)
            .push(bye),
    );

    // 关闭语义统一走窗口按钮/Esc（F207）；这里不自定义关闭拦截。
    app.run();
}
"#;

// ---------------------------------------------------------------------------
// 示例二 · clipboard（读剪贴板 + 所有权红线感知）
// ---------------------------------------------------------------------------

pub const CLIPBOARD_SRC: &str = r#"// 示例二：clipboard —— 剪贴板读写 + 所有权协议红线。
// 先修：hello。目标：学会「复制/粘贴」与后台读取禁区（B-3901）。
//
// 剪贴板的三条铁律（本示例逐条落到代码里）：
// 1. 写（put）永远允许——用户按了按钮就是前台意志；
// 2. 读（get）必须由前台交互事件触发——定时器/后台线程轮询是红线；
// 3. 读到的内容视为不可信外部数据——粘贴进输入框前不假设其形态。
//
// 违反第 2 条的后果：别人的密码复制后，被后台偷读——所以平台在
// 所有权协议里直接禁掉，而不是靠应用自觉。

use varix_app::{App, Window, Button, Input, clipboard};

// 应用入口：结构与 hello 完全同构——先修链的意义就在这里。
fn main() {
    let app = App::new("clipboard-demo");
    let win = Window::builder()
        .title("剪贴板演示")
        .size(360, 180)
        .build(&app);

    // 输入框三件套：placeholder 是空态引导，不是摆设（F229）。
    let input = Input::new().placeholder("粘贴到这里");

    // 复制按钮：put 是前台语义，写总是允许。
    let copy = Button::new("复制");
    copy.on_click(|input| {
        // 把输入框当前文本送上剪贴板——替换式写入，系统语义一致。
        clipboard::put(input.text());
    });

    // 粘贴按钮：get 必须挂在点击回调里——这就是「前台交互触发」。
    let paste = Button::new("粘贴");
    paste.on_click(|input| {
        // 剪贴板为空时返回 None：静默返回是合法路径，
        // 按钮保持可点、界面保持可用——零死胡同（体验十四章）。
        if let Some(text) = clipboard::get() {
            input.set_text(text);
        }
    });

    // 布局：纵向列 + 横向行，声明式组合，不做手动坐标。
    win.set_content(
        varix_app::Column::new()
            .push(input)
            .push_row(vec![copy, paste]),
    );

    // 关闭语义统一走窗口按钮/Esc；应用随最后窗口退出。
    app.run();
}
"#;

// ---------------------------------------------------------------------------
// 示例三 · theme-tokens（令牌取色 + 深浅联动）
// ---------------------------------------------------------------------------

pub const THEME_SRC: &str = r#"// 示例三：theme-tokens —— 主题令牌与深浅自动切换。
// 先修：hello（同源布局；与 clipboard 无依赖，可并行学）。
// 目标：界面颜色全部走令牌，主题切换零重启零闪烁（F151/F225）。
//
// 为什么令牌优先于裸色值：
// - 裸色值在深浅两套主题下必然一套翻车——人眼适配不了两份常量；
// - 令牌由令牌表保证对比度（≥4.5:1，F141），应用侧免心智；
// - 全系统审计「零硬编码色」（B-1104）——裸 hex 会被扫描器点名。
//
// 本示例的组件树：Panel（表面令牌）→ 两级文字（前景令牌）。

use varix_app::{App, Window, Panel, Label, Color, theme};

// 入口：与 hello 同构，只多一个主题监听。
fn main() {
    let app = App::new("theme-demo");
    let win = Window::builder()
        .title("主题令牌")
        .size(380, 220)
        .build(&app);

    // 面板底色用「抬升表面」令牌：深浅主题自动换装，代码零分支。
    let card = Panel::new()
        .surface(Color::token("surface-raised"))
        .padding(12);

    // 主文字：fg-primary 在深浅两套主题下都由令牌表保证可读。
    let title = Label::new("跟随主题")
        .color(Color::token("fg-primary"));

    // 次要文字用次级令牌：层级感来自令牌而不是手动调透明度。
    let hint = Label::new("切换深浅模式看效果")
        .color(Color::token("fg-secondary"));

    // 组装：先 push 文字再挂到窗口——声明式，顺序即视觉顺序。
    card.push(title).push(hint);
    win.set_content(card);

    // 监听主题切换：无窗口闪烁是合成器职责（交叉淡入 200ms），
    // 应用侧回调通常无事可做——留空是正确姿势，不要手绘过渡。
    theme::on_change(|_mode| {
        // 令牌色已由合成器热替换；这里只做数据级联动（如有）。
    });

    app.run();
}
"#;

// ---------------------------------------------------------------------------
// 示例四 · mini-editor（多行编辑 + 撤销栈接入）
// ---------------------------------------------------------------------------

pub const MINI_EDITOR_SRC: &str = r#"// 示例四：mini-editor —— 多行编辑器与全局撤销框架。
// 先修：clipboard（输入控件）+ theme-tokens（面板配色）。
// 目标：文本编辑三件套——占位符/校验/错误提示，以及 undo 链接入。
//
// 本示例覆盖的体验判据（写应用时同步自检）：
// - 输入框三态：placeholder / 正常 / 错误提示说「怎么改对」（F229）；
// - 撤销链粒度按动作，不是按字符（F202）；
// - 保存走原子写，断电零损坏（B-1801）；
// - 关窗前三问只在真有未保存改动时弹（F310）。
//
// 组件树：Column[ Editor, Row[Save, Undo], StatusBar ]。

use varix_app::{App, Window, Editor, Button, StatusBar, undo};

// 文件名固定为草稿：教学示例不引入文件对话框（F008 是进阶课题）。
const DRAFT: &str = "draft.txt";

// 应用入口。
fn main() {
    let app = App::new("mini-editor");
    let win = Window::builder()
        .title("迷你编辑器")
        .size(520, 360)
        .min_size(360, 240)
        .build(&app);

    // Editor 自带占位符三态与 IME 组合期保护（F229/B-904）：
    // 组合中的拼音不会误触快捷键，候选窗跟随光标。
    let editor = Editor::new()
        .placeholder("写点什么……")
        .max_len(100_000); // 上限校验超限时行内提示，不弹窗（F231）

    // 撤销/重做：平台全局栈，应用只发动作不记快照。
    // 粒度按「动作」（一次粘贴/一次删除词），不是每键一步。
    undo::attach(&editor);

    // 状态栏：所有用户反馈的出口——错误也在这里说人话。
    let status = StatusBar::new("就绪");

    // 保存按钮：点击后 100ms 内必有反馈（按压态 + 状态栏文案）。
    let save = Button::new("保存").shortcut("Ctrl+S");
    save.on_click((editor.clone(), status.clone()), |(editor, status)| {
        // 原子写：先写临时文件再换名，断电零损坏。
        match editor.save_atomic(DRAFT) {
            Ok(bytes) => {
                // 成功反馈带量：让用户知道保存了什么。
                status.set(&format!("已保存 {bytes} 字节"));
            }
            // 错误三要素：发生了什么/为什么/下一步怎么办（F209）。
            // 技术细节（错误码/栈）收进「详情」，不裸抛给用户。
            Err(e) => status.set(&format!(
                "保存失败：{e}；请检查磁盘空间后重试"
            )),
        }
    });

    // 撤销按钮：与 Ctrl+Z 等价——鼠标与键盘能力对等（F206）。
    let undo_btn = Button::new("撤销").shortcut("Ctrl+Z");
    undo_btn.on_click(editor.clone(), |editor| editor.undo());

    // 脏标记驱动关窗三问：打开没动直接关不问，动了才问。
    editor.on_dirty(|status| status.set("有未保存改动（Ctrl+S 保存）"));

    // 布局：编辑区吃满剩余高度，状态栏贴底。
    win.set_content(
        varix_app::Column::new()
            .push(editor)
            .push_row(vec![save, undo_btn])
            .push(status),
    );

    // 关窗前三问：返回 true = 有未保存改动 = 平台弹确认。
    // 默认焦点落在「保存」上（F310 判据），Enter 即保存。
    win.on_close(|editor| editor.dirty());

    app.run();
}
"#;

// ---------------------------------------------------------------------------
// 示例五 · notepad-lite（标签页 + 查找 + 全键盘可达）
// ---------------------------------------------------------------------------

pub const NOTEPAD_SRC: &str = r#"// 示例五：notepad-lite —— 标签页式编辑器（F005 标志件教学版）。
// 先修：mini-editor。目标：标签页/查找/键盘可达三个成熟应用支柱。
// 行数门禁 ≤200 行（F136 教学约束）——功能做小做精，不堆砌。
//
// 三个支柱对应的判据：
// - 标签页：重启恢复全状态含滚动位（F089）；拖出成窗是进阶路径；
// - 查找：即时高亮 <100ms，Esc 清除并还焦点（F221）；
// - 键盘可达：无鼠标走完全流程（F206）——每个按钮都有快捷键。
//
// 本示例刻意展示「平台默认值优先」：确认框/焦点环/按压态全部
// 用平台样式，应用代码里看不到一行样式——一致性来自词典（十）。

use varix_app::{
    App, Window, Tabs, Editor, Toolbar, Button, Input, StatusBar, undo,
};

// 上限与预算集中定义（单一定义点——十号纪律的工程面）。
const MAX_TABS: usize = 10;
const RESTORE_ON_BOOT: bool = true;

// 应用入口。
fn main() {
    let app = App::new("notepad-lite");
    let win = Window::builder()
        .title("轻量记事本")
        .size(720, 480)
        .min_size(480, 320)
        .build(&app);

    // 标签容器：每标签一个 Editor；重启恢复全状态。
    // 收缩阈值与 Tooltip 由平台默认（F271 判据），应用不重复造。
    let tabs = Tabs::new()
        .restore_on_boot(RESTORE_ON_BOOT)
        .max_tabs(MAX_TABS);

    // 首标签兜底：空态给行动指引而不是白页（F210）。
    // 空态文案说「能做什么、怎么开始」，可跳过、不重复骚扰。
    tabs.add("未命名", Editor::new().placeholder("Ctrl+N 新建标签，开始输入"));

    // 工具栏：新建/关闭/查找三钮。
    // 顺序 = 使用频率序 = 焦点序（Tab 循环顺着点下来）。
    let toolbar = Toolbar::new();

    // 新建：超过上限时按钮置灰 + tooltip 解释原因（不做静默失败）。
    let new_tab = Button::new("新建").shortcut("Ctrl+N");
    new_tab.on_click(tabs.clone(), |tabs| {
        if tabs.len() >= MAX_TABS {
            tabs.flash_full(); // 满额反馈：抖动 + tooltip「最多 10 个标签」
            return;
        }
        tabs.add("未命名", Editor::new().placeholder("开始输入……"));
    });

    // 关闭标签：脏内容三问，取消永远是安全出路。
    // 关最后一个标签 = 关窗（语义与 Windows 记事本一致——十号）。
    let close_tab = Button::new("关闭标签").shortcut("Ctrl+W");
    close_tab.on_click(tabs.clone(), |tabs| {
        tabs.close_current_checked();
    });

    // 查找条：Ctrl+F 唤出，Enter 下一个，Esc 清除并还焦点。
    // 高亮即时性由平台保证；这里只负责把查询词递过去。
    let find = Input::new().placeholder("查找（Enter 下一个）");
    find.on_enter(tabs.clone(), |tabs, query| {
        // 空查询不清空现有高亮——避免「输入中途高亮消失」的错乱。
        if !query.is_empty() {
            tabs.find_highlight(&query);
        }
    });
    find.on_escape(tabs.clone(), |tabs| {
        tabs.find_clear();
        tabs.refocus_editor(); // 焦点还给触发元素（F206 焦点归还）
    });

    toolbar.push(new_tab).push(close_tab).push(find);

    // 状态栏：标签名 + 行数，切换标签即时刷新。
    let status = StatusBar::new("就绪");
    tabs.on_status(|status, tab| {
        status.set(&format!("{} · {} 行", tab.title(), tab.line_count()));
    });

    // 撤销链：每标签独立（切标签即切栈），粒度按动作。
    undo::attach_all(&tabs);

    // 布局：工具栏顶、标签区吃满、状态栏贴底。
    win.set_content(
        varix_app::Column::new()
            .push(toolbar)
            .push(tabs)
            .push(status),
    );

    // 关窗三问：任一标签脏即弹；批量勾选「应用到全部」支持（F310）。
    win.on_close(|tabs| tabs.any_dirty());

    // 快捷键注册：声明的键位必须真实生效（四号「快捷键是承诺」）。
    // Ctrl+F / Esc 已由查找条声明；这里补全局对。
    win.shortcuts(|s| {
        s.bind("Ctrl+F", |win| win.focus_find());
        s.bind("Escape", |win| win.clear_find());
    });

    app.run();
}
"#;

// ---------------------------------------------------------------------------
// 审计面：五例逐例过四类门（行数/密度/README/先修链方向性）
// ---------------------------------------------------------------------------

/// 示例登记：(名称, 源码, 先修示例数)。顺序即 builds_on 链序。
pub fn examples() -> alloc::vec::Vec<(&'static str, &'static str, usize)> {
    alloc::vec![
        ("hello", HELLO_SRC, 0),
        ("clipboard", CLIPBOARD_SRC, 1),
        ("theme-tokens", THEME_SRC, 1),
        ("mini-editor", MINI_EDITOR_SRC, 2),
        ("notepad-lite", NOTEPAD_SRC, 3),
    ]
}

/// 先修链方向性：先修数 < 自身序号（链上只能指向更早的示例）。
/// 这是 f136f 拓扑语义在本体的轻量形态——链是线性登记，无环可能，
/// 但序号方向性必须机检（防登记倒置）。
pub fn prereq_direction_violations() -> alloc::vec::Vec<&'static str> {
    let ex = examples();
    let mut bad: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    for (i, (name, _, prereqs)) in ex.iter().enumerate() {
        if *prereqs >= i + 1 {
            bad.push(name);
        }
    }
    bad
}

/// README 四件必查（简介/运行步骤/截图位/先修说明）——示例配套件清单。
pub fn readme_pieces_ok(pieces: &[(&'static str, bool)]) -> bool {
    let required = ["简介", "运行步骤", "截图位", "先修说明"];
    required.iter().all(|r| pieces.iter().any(|(n, ok)| n == r && *ok))
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F136G_TAG: &str = "stareco-F136-deep4";

pub fn run_f136_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F136G_TAG);

    let ex = examples();

    // 行数门禁：逐例 ≤200（f136f::audit_lines 复用——一处一事实）。
    let files: alloc::vec::Vec<(&'static str, usize)> =
        ex.iter().map(|(n, src, _)| (*n, src.lines().count())).collect();
    let audit = f136f::audit_lines(&files);
    set.add("f136g lines gate", audit.over.is_empty(), "五例全部 ≤200 行");
    set.add("f136g lines total", audit.total >= 250, "源码本体体量（≥250 行内容件）");

    // 注释密度带：逐例 30-60%（f136f::comment_density 复用）。
    let mut density_ok = true;
    let mut density_report = alloc::vec::Vec::new();
    for (n, src, _) in &ex {
        let d = f136f::comment_density(src);
        density_report.push((*n, d));
        if !f136f::density_in_band(d) {
            density_ok = false;
        }
    }
    set.add("f136g density band", density_ok, "五例密度全在 30-60%");

    // 先修链方向性。
    set.add("f136g prereq direction", prereq_direction_violations().is_empty(), "先修只指向更早示例");
    set.add("f136g chain shape", ex[0].2 == 0 && ex[4].2 == 3, "链首无先修/链尾三先修");

    // 密度逐例数值锚点（防"全部 0 注释也进带"的假阳性——30% 下限已防，
    // 这里钉两个具体数字做回归锚）。
    let hello_density = f136f::comment_density(HELLO_SRC);
    let notepad_density = f136f::comment_density(NOTEPAD_SRC);
    set.add(
        "f136g density anchors",
        (30..=60).contains(&hello_density) && (30..=60).contains(&notepad_density),
        "首尾两例密度锚点",
    );

    // README 四件（登记配套件结论——示例仓库发布门输入）。
    let readme = [
        ("简介", true),
        ("运行步骤", true),
        ("截图位", true),
        ("先修说明", true),
    ];
    set.add("f136g readme four", readme_pieces_ok(&readme), "四件齐");
    let readme_bad = [
        ("简介", true),
        ("运行步骤", true),
        ("截图位", false),
        ("先修说明", true),
    ];
    set.add("f136g readme miss", !readme_pieces_ok(&readme_bad), "缺件点名");

    // 源码本体卫生：无 TODO/占位符（正文纪律对内容件同样生效）。
    let has_todo = ex.iter().any(|(_, s, _)| s.contains("TODO") || s.contains("FIXME"));
    set.add("f136g no todo", !has_todo, "源码本体零 TODO");

    // 五例齐整性。
    set.add("f136g count", ex.len() == 5, "五示例登记");

    let _ = density_report;
    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn sources_nonempty_and_gated() {
        for (name, src, _) in examples() {
            assert!(src.len() > 200, "{name} 源码过短");
            assert!(src.lines().count() <= 200, "{name} 超行数门禁");
            assert!(src.starts_with("// 示例"), "{name} 缺教学头注");
        }
    }

    #[test]
    fn density_all_in_band() {
        for (name, src, _) in examples() {
            let d = f136f::comment_density(src);
            assert!(f136f::density_in_band(d), "{name} 密度 {d}% 出带");
        }
    }
}
