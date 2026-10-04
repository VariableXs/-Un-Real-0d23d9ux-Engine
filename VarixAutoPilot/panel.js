// VarixAutoPilot · 面板前端
//
// 只做三件事：轮询快照渲染、编辑并保存模板、触发执行。
// 所有判定都靠后端返回的客观证据（编辑器字符数 / 消息是否入流），
// 面板不自己猜"成功了吗"——猜就是给自己造幻觉。

const $ = (id) => document.getElementById(id);
let S = { snapshot: null, settings: null, lastLog: 0, busy: false };

/** 短路径：把长目录压成 …\父\子 便于扫读（完整值在 title 里）。 */
function shortPath(p) {
  if (!p) return '';
  if (p.length <= 46) return p;
  const parts = p.split(/[\\/]/).filter(Boolean);
  if (parts.length <= 2) return p;
  return '…\\' + parts.slice(-2).join('\\');
}

// ---------------------------------------------------------------- 快照渲染
async function tick() {
  if (S.busy) return;
  try {
    const r = await (await fetch('/api/snapshot')).json();
    if (r.ok) {
      S.snapshot = r.snapshot;
      render(r.snapshot);
      setConn('ok', '已连接');
    } else {
      setConn('err', r.err || '未知错误');
    }
  } catch (e) {
    setConn('err', '面板与后端失联：' + e.message);
  }
  setTimeout(tick, (S.settings && S.settings.autoRefreshMs) || 1500);
}

function setConn(kind, msg) {
  $('s-dot').className = 'dot ' + kind;
  $('s-msg').textContent = msg;
}

function render(s) {
  $('s-conv').textContent = s.convs.length;
  $('s-model').textContent = s.currentModel || '未知';
  $('s-cwd').textContent = shortPath(
    (s.convs.find((c) => c.selected) || {}).cwd || ''
  ) || '未知';
  $('s-ver').textContent = s.version || '?';

  const sendEl = $('s-send');
  if (s.sending) {
    sendEl.innerHTML = '<span class="badge y">生成中</span>';
  } else {
    sendEl.innerHTML = '<span class="badge n">空闲</span>';
  }

  $('conv-count').textContent = s.convs.length + ' 个';
  const box = $('convs');
  box.innerHTML = '';
  for (const c of s.convs) {
    const d = document.createElement('div');
    d.className = 'conv' + (c.selected ? ' sel' : '');

    const t = document.createElement('div');
    t.className = 't';
    t.textContent = c.title;
    if (c.selected) {
      const tg = document.createElement('span');
      tg.className = 'tag sel';
      tg.textContent = '当前';
      t.appendChild(tg);
    }
    d.appendChild(t);

    const rt = document.createElement('div');
    rt.className = 'rt';
    rt.textContent = c.relTime || '';
    d.appendChild(rt);

    const m = document.createElement('div');
    m.className = 'meta';
    // 模型：只有当前对话读得到（顶栏选择器），其余如实标"需点开"
    const modelTxt = c.model
      ? '<span class="k">模型 </span><span class="v">' + esc(c.model) + '</span>'
      : '<span class="k">模型 </span><span class="k">侧栏不显示（点开该对话可读）</span>';
    const cwdTxt = c.cwd
      ? '<span class="k">目录 </span><span class="v path" title="' + esc(c.cwd) + '">' + esc(shortPath(c.cwd)) + '</span>'
      : '<span class="k">目录 </span><span class="tag bad">' + esc(c.cwdConfidence) + '</span>';
    m.innerHTML = modelTxt + '<br>' + cwdTxt;
    d.appendChild(m);

    box.appendChild(d);
  }
}

const esc = (s) =>
  String(s == null ? '' : s).replace(/[&<>"']/g, (x) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[x])
  );

// ---------------------------------------------------------------- 设置
async function loadSettings() {
  const r = await (await fetch('/api/settings')).json();
  if (!r.ok) return setHint('读设置失败：' + r.err, true);
  S.settings = r.settings;
  $('tpl').value = r.settings.template || '';
  $('v-task').value = (r.settings.vars || {})['任务'] || '';
  $('v-ctx').value = (r.settings.vars || {})['上下文'] || '';
  $('o-new').checked = !!r.settings.openNew;
  $('o-dry').checked = !!r.settings.dryRun;
  await doPreview();
}

function collect() {
  return {
    template: $('tpl').value,
    vars: { '任务': $('v-task').value, '上下文': $('v-ctx').value },
    openNew: $('o-new').checked,
    dryRun: $('o-dry').checked,
  };
}

async function saveSettings() {
  const r = await (await fetch('/api/settings', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(collect()),
  })).json();
  S.settings = r.settings;
  setHint(r.ok ? '已保存到 settings.json（跨重启保留）' : '保存失败：' + r.err, !r.ok);
  await doPreview();
}

function setHint(msg, bad) {
  const h = $('hint');
  h.textContent = msg;
  h.style.color = bad ? 'var(--err)' : 'var(--fg3)';
}

// ---------------------------------------------------------------- 预览
async function doPreview() {
  const body = collect();
  const r = await (await fetch('/api/preview', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })).json();
  if (!r.ok) return setHint(r.err, true);
  const p = $('preview');
  p.innerHTML = highlight(r.text);
  if (r.missing.length) {
    setHint(
      '⚠ 还有 ' + r.missing.length + ' 个占位符没填：' +
        r.missing.map((k) => '{{' + k + '}}').join('、') +
        ' —— 未填的会原样保留（不会变成空段落），发送会被后端拦住。',
      true
    );
  } else {
    setHint('预览就绪，' + r.chars + ' 字符' + (body.dryRun ? '（当前是干跑模式：只填不发）' : '（当前会真发送）'), false);
  }
}

/** 把 {{键}} 高亮出来——没填的用醒目底色，一眼看见。 */
function highlight(t) {
  return esc(t).replace(/\{\{\s*([^{}]+?)\s*\}\}/g, '<span class="ph">{{$1}}</span>');
}

// ---------------------------------------------------------------- 执行
async function doSend(real) {
  if (S.busy) return setHint('上一轮还在跑，请等它结束', true);
  S.busy = true;
  $('b-send').disabled = true;
  $('b-sendreal').disabled = true;
  try {
    const body = { ...collect(), dryRun: real ? false : true };
    setHint(real ? '正在填入并发送…' : '正在填入（不发送）…', false);
    const r = await (await fetch('/api/send', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })).json();

    if (r.ok) {
      setHint(
        r.dryRun
          ? `已填入 ${r.chars} 字符，未发送。截图：${r.shot || '（无）'}`
          : `已发送（${r.chars} 字符）· ${r.evidence || '已确认'}`,
        false
      );
    } else {
      setHint('✗ ' + (r.err || '未知错误') + (r.hint ? '\n' + r.hint : ''), true);
    }
  } catch (e) {
    setHint('请求失败：' + e.message, true);
  } finally {
    S.busy = false;
    $('b-send').disabled = false;
    $('b-sendreal').disabled = false;
    pollLog();
    tick();
  }
}

// ---------------------------------------------------------------- 日志
async function pollLog() {
  const r = await (await fetch('/api/log')).json();
  if (!r.ok) return;
  const box = $('log');
  box.innerHTML = r.lines
    .map((l) => {
      const m = /\[(OK|ERR|WARN|INFO)\]/.exec(l);
      const cls = m ? m[1] : '';
      return '<div class="' + cls + '">' + esc(l) + '</div>';
    })
    .join('');
  const w = $('logwrap');
  w.scrollTop = w.scrollHeight;
}

// ---------------------------------------------------------------- 启动
(async function init() {
  await loadSettings();
  setInterval(pollLog, 2500);
  tick();
})();

// 快捷键：Ctrl+Enter 真发，Ctrl+S 保存
document.addEventListener('keydown', (e) => {
  if (e.ctrlKey && e.key === 'Enter') { e.preventDefault(); doSend(true); }
  if (e.ctrlKey && e.key === 's') { e.preventDefault(); saveSettings(); }
});
