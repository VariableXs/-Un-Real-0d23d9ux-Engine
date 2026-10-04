// VarixAutoPilot · 前端逻辑
//
// 只做四件事：轮询快照渲染、编辑模板、触发执行、把后端来的三要素错误如实呈现。
//
// ★ 边界原则 ★
// 前端**不自己判断"成功了吗"**——那必须由后端用客观证据
// （编辑器字符数 / 消息是否入流）判定。前端只负责展示后端的结论。
// 前端若自己猜，就是给自己造幻觉。
//
// ★ 双模式 ★
// Tauri 环境：走 invoke 调 Rust 后端。
// 浏览器环境（Tauri 全局不存在）：走内置 mock，用于**独立视觉预览与回归**——
//   没有 mock 的话，每次改样式都得先 cargo build 才能看效果，太慢。
// mock 数据是**真实结构**（7 个对话 / 模型 / 目录 / 发送态），
// 所以它既能验布局，也能在没有 WorkBuddy 时演示。

const $ = (id) => document.getElementById(id);

// ── 后端调用：Tauri 或 mock ───────────────────────────────
const IS_TAURI = typeof window.__TAURI_INTERNALS__ !== 'undefined';

const MOCK = {
  version: '5.6.2',
  conversation_title: '自动领任务直至验收并归档',
  current_model: 'Space-Bunny',
  sending: false,
  send_label: '',
  editor_chars: 0,
  editor_visible: true,
  session_count: 276,
  convs: [
    { index: 0, conv_id: 'a09df76e', title: '# 下一轮施工提示词（自动生成，勿手改）', rel_time: '4分钟前', model: '', selected: false, cwd: 'C:\\Users\\varia\\WorkBuddy\\2026-10-04-17-48-14', cwd_confidence: '精确（conversation-id 命中）' },
    { index: 1, conv_id: '6a70cd4d', title: '破解软件咨询', rel_time: '7小时前', model: '', selected: false, cwd: 'C:\\Users\\varia\\WorkBuddy\\2026-10-03-15-11-44', cwd_confidence: '精确（conversation-id 命中）' },
    { index: 2, conv_id: '44dc2b80', title: '重构原 MD 并输出改进版文件', rel_time: '14小时前', model: '', selected: false, cwd: 'C:\\Users\\varia\\WorkBuddy\\2026-10-03-22-15-25', cwd_confidence: '精确（conversation-id 命中）' },
    { index: 3, conv_id: 'e042f2d0', title: '自动领任务直至验收并归档', rel_time: '', model: 'Space-Bunny', selected: true, cwd: 'D:\\2\\14\\-Un-Real-0d23d9ux-Engine-main', cwd_confidence: '精确（conversation-id 命中）' },
    { index: 4, conv_id: '3b7c9a12', title: '检查 varix 库测试结果', rel_time: '14分钟前', model: '', selected: false, cwd: 'D:\\2\\14\\-Un-Real-0d23d9ux-Engine-main', cwd_confidence: '精确（conversation-id 命中）' },
    { index: 5, conv_id: 'a1b2c3d4', title: '核查后台命令输出', rel_time: '', model: '', selected: false, cwd: 'D:\\2\\14\\-Un-Real-0d23d9ux-Engine-main\\VarixTaskOps', cwd_confidence: '精确（conversation-id 命中）' },
    { index: 6, conv_id: 'ffeeddcc', title: '自动化任务领取与验收', rel_time: '1小时前', model: '', selected: false, cwd: '', cwd_confidence: '未查到该 session' },
  ],
  cwd_histogram: [],
};

/** 前端本地模板渲染（仅 mock 模式用；真实模式由 Rust 渲染）。 */
function localRender(tpl, vars) {
  const now = new Date();
  const p = (n) => String(n).padStart(2, '0');
  const sys = { 时间: `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())} ${p(now.getHours())}:${p(now.getMinutes())}:${p(now.getSeconds())}` };
  const missing = [];
  const text = String(tpl).replace(/\{\{\s*([^{}]+?)\s*\}\}/g, (_, k) => {
    const key = k.trim();
    if (key in sys) return sys[key];
    const v = vars[key];
    if (v === undefined || v === null || v === '') {
      if (!missing.includes(key)) missing.push(key);
      return `{{${key}}}`;
    }
    return String(v);
  });
  return { text, missing, chars: [...text].length };
}

const api = IS_TAURI
  ? {
      probe: () => invoke('probe'),
      preview: (tpl, vars) => invoke('preview', { tpl, vars }),
      send: (tpl, vars, dryRun, openNew) =>
        invoke('send', { tpl, vars, dryRun, openNew }),
    }
  : {
      probe: async () => MOCK,
      preview: async (tpl, vars) => localRender(tpl, vars),
      send: async (tpl, vars, dryRun) => {
        const r = localRender(tpl, vars);
        if (r.missing.length) {
          return {
            ok: false, dry_run: dryRun, chars: 0, evidence: '',
            err: {
              what: `有 ${r.missing.length} 个占位符没填，拒绝发送`,
              why: r.missing.map((k) => `{{${k}}}`).join('、'),
              next: '在「变量」区填好，或把模板里那些占位符删掉',
            },
          };
        }
        await new Promise((r) => setTimeout(r, 600));
        return {
          ok: true, dry_run: dryRun, chars: r.chars,
          evidence: dryRun ? '内容已就位，未发送' : '输入框已清空（剩余 5 字符）',
        };
      },
    };

// Tauri 的 invoke 是动态加载的（Tauri 2 不再把它打进全局），
// 所以上面用条件表达式里的 import 无法在顶层 await 之前完成——
// 这里改为在模块顶层用动态 import 赋值。
if (IS_TAURI) {
  const core = await import('/__TAURI__/core.js');
  api.probe = () => core.invoke('probe');
  api.preview = (tpl, vars) => core.invoke('preview', { tpl, vars });
  api.send = (tpl, vars, dryRun, openNew) =>
    core.invoke('send', { tpl, vars, dryRun, openNew });
}



const DEFAULT_TPL = `# {{任务}} · 下一轮施工指令

> 由 VarixAutoPilot 于 {{时间}} 填入。
> 改写这段话：直接在输入框里改完再发——本工具只负责送到，不替你决定。

## 目标

在 **Agent 模式** 下施工 **{{任务}}**。

## 必读上下文

{{上下文}}

## 纪律（凌驾任务目标）

1. 源码只增不减不移动——推前必数文件数与行数，数不对不推
2. 过程产物全归 \`_attic/\`，原位清零，不入库
3. 只推纯功能代码——测试脚本与截图一律不推
4. 只 add 显式路径，绝不 \`git add .\`
5. 异常零静默——失败必带「发生了什么/为什么/下一步怎么办」
6. 完成度铁律——不半途交付、不留占位符、不写 TODO
7. 修测试不迁就实现，修实现不迁就测试；但要先归因是哪一边错

## 现在开始

读上面点名的文件，按锚点原文逐条落实判据。做完交活、回写任务板、归档过程产物。`;

const S = { snap: null, busy: false, timer: 0 };

// ─────────────────────────── 工具 ───────────────────────────
const esc = (s) =>
  String(s == null ? '' : s).replace(/[&<>"']/g, (x) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[x]));

/** 压短长路径：…\父\子，完整值挂 title。 */
function shortPath(p, max = 42) {
  if (!p) return '';
  if (p.length <= max) return p;
  const parts = p.split(/[\\/]/).filter(Boolean);
  return '…\\' + parts.slice(-2).join('\\');
}

/** 场景切到 body[data-scene]，驱动整套配色与光晕。 */
function setScene(name) {
  if (document.body.dataset.scene !== name) document.body.dataset.scene = name;
}

function conn(kind, msg) {
  $('s-dot').className = 'dot ' + kind;
  $('s-msg').textContent = msg;
}

// ─────────────────────────── 快照 ───────────────────────────
async function tick() {
  if (S.busy) return void (S.timer = setTimeout(tick, 1200));
  try {
    const s = await api.probe();
    S.snap = s;
    render(s);
    conn('ok', '已连接');
  } catch (e) {
    renderErr(e);
    conn('err', '采集失败');
  }
  S.timer = setTimeout(tick, 1500);
}

/** 把后端的三要素错误渲染出来（不裸奔异常码）。 */
function renderErr(e) {
  const p = e && typeof e === 'object' ? e : null;
  const what = p?.what || p?.message || String(e);
  $('hint').className = 'note';
  $('hint').innerHTML =
    `<span class="e-what">${esc(what)}</span>` +
    (p?.why ? `<span class="e-why">${esc(p.why)}</span>` : '') +
    (p?.next ? `<span class="e-next">${esc(p.next)}</span>` : '');
  setScene('alarm');
}

function render(s) {
  $('s-conv').textContent = s.convs.length;
  $('s-model').textContent = s.current_model || '未知';
  $('s-cwd').textContent = shortPath(s.convs.find((c) => c.selected)?.cwd || '') || '未知';
  $('s-ver').textContent = s.version || '?';
  $('s-send').innerHTML = s.sending
    ? '<span class="tag bad">生成中</span>'
    : '<span class="tag now">空闲</span>';

  // 场景：忙 → 琥珀；否则回到空闲
  if (s.sending) setScene('busy');
  else if (document.body.dataset.scene === 'busy') setScene('idle');

  $('conv-count').textContent = s.convs.length + ' 个';
  const box = $('convs');
  box.innerHTML = '';
  if (!s.convs.length) {
    box.innerHTML = '<div class="empty">没有读到对话。确认 WorkBuddy 已启动且开着对话窗口。</div>';
    return;
  }
  for (const c of s.convs) {
    const d = document.createElement('div');
    d.className = 'conv' + (c.selected ? ' sel' : '');

    const t = document.createElement('div');
    t.className = 't';
    t.textContent = c.title;
    if (c.selected) {
      const g = document.createElement('span');
      g.className = 'tag now';
      g.textContent = '当前';
      t.append(' ');
      t.append(g);
    }
    d.append(t);

    const rt = document.createElement('div');
    rt.className = 'rt';
    rt.textContent = c.rel_time || '';
    d.append(rt);

    const m = document.createElement('div');
    m.className = 'meta';
    const model = c.model
      ? `<b>模型</b>${esc(c.model)}`
      : `<b>模型</b><span class="tag">侧栏不显示 · 点开可读</span>`;
    const dir = c.cwd
      ? `<b>目录</b><span class="dir" title="${esc(c.cwd)}">${esc(shortPath(c.cwd))}</span>`
      : `<b>目录</b><span class="tag bad">${esc(c.cwd_confidence)}</span>`;
    m.innerHTML = `<span>${model}</span><span>${dir}</span>`;
    d.append(m);

    box.append(d);
  }
}

// ─────────────────────────── 模板 ───────────────────────────
const collect = () => ({
  tpl: $('tpl').value,
  vars: { '任务': $('v-task').value, '上下文': $('v-ctx').value },
  openNew: $('o-new').checked,
  dryRun: $('o-dry').checked,
});

async function doPreview() {
  try {
    const r = await api.preview(collect().tpl, collect().vars);
    $('preview').innerHTML = esc(r.text).replace(/\{\{\s*([^{}]+?)\s*\}\}/g,
      '<span class="ph">{{$1}}</span>');
    const h = $('hint');
    if (r.missing.length) {
      h.className = 'note';
      h.innerHTML =
        `<span class="e-what">还有 ${r.missing.length} 个占位符没填：` +
        r.missing.map((k) => `{{${esc(k)}}}`).join('、') + `</span>` +
        `<span class="e-next">未填的会原样保留（不会变成空段落），发送会被拦下。填好或删掉即可。</span>`;
    } else {
      h.className = 'note ok';
      h.textContent = `预览就绪 · ${r.chars} 字符 · ` +
        (collect().dryRun ? '干跑模式（只填不发）' : '将真发送');
    }
  } catch (e) {
    renderErr(e);
  }
}

// ─────────────────────────── 执行 ───────────────────────────
async function doSend(real) {
  if (S.busy) return;
  S.busy = true;
  $('b-fill').disabled = $('b-send').disabled = true;
  const h = $('hint');
  h.className = 'note';
  h.textContent = real ? '正在填入并发送…' : '正在填入（不发送）…';
  try {
    const c = collect();
    const r = await api.send(c.tpl, c.vars, !real, c.openNew);
    if (r.ok) {
      h.className = 'note ok';
      h.textContent = r.dry_run
        ? `已填入 ${r.chars} 字符，未发送。`
        : `已发送 · ${r.chars} 字符 · ${r.evidence}`;
      if (real) {
        setScene('sent');
        const f = $('flash');
        f.classList.remove('go');
        void f.offsetWidth;          // 强制重排以重启动画
        f.classList.add('go');
        setTimeout(() => setScene('idle'), 1400);
      }
    } else {
      h.className = 'note' + (r.err?.busy ? ' busy' : '');
      h.innerHTML = r.err
        ? `<span class="e-what">${esc(r.err.what)}</span>` +
          (r.err.why ? `<span class="e-why">${esc(r.err.why)}</span>` : '') +
          (r.err.next ? `<span class="e-next">${esc(r.err.next)}</span>` : '')
        : '执行失败（后端未给出原因）';
      if (r.err?.busy) setScene('busy');
    }
  } catch (e) {
    renderErr(e);
  } finally {
    S.busy = false;
    $('b-fill').disabled = $('b-send').disabled = false;
    tick();
  }
}

// ─────────────────────────── 启动 ───────────────────────────
(async function init() {
  $('tpl').value = DEFAULT_TPL;
  await doPreview();
  tick();
  // 模板改动 300ms 防抖后自动预览（不打断输入）
  let t = 0;
  $('tpl').addEventListener('input', () => {
    clearTimeout(t);
    t = setTimeout(doPreview, 300);
  });
})();

// 快捷键：Ctrl+Enter 真发，Ctrl+P 预览
document.addEventListener('keydown', (e) => {
  if (e.ctrlKey && e.key === 'Enter') { e.preventDefault(); doSend(true); }
  if (e.ctrlKey && e.key === 'p') { e.preventDefault(); doPreview(); }
});
