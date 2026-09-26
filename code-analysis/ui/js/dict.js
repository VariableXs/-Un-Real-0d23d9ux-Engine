/* =============================================================================
 * v3 · 词典引擎（开放字典 + 语言识别 + 语法词条 + 重复内容检测）
 *   专属文件夹：ui/dicts/*.json —— 丢进去就载入（壳A /api/dicts 列表），
 *   也支持运行时导入 .json。归一化接受任何形状：
 *     ① 平铺映射 { "token": "通行证" }
 *     ② 标准段   { "terms": {...}, "syntax": { "rust": { "fn": "函数" } } }
 *     ③ 数组条目 [ { "term": "...", "plain": "..." } ]
 *   合并纪律：先到先得（同名词条后包不覆盖，计入 skipped）——去重由构造保证。
 *   纯数据、无网络依赖；全部确定性算法（与 ca-core「零 AI」纪律一致）。
 * ========================================================================== */
(function (root) {
  "use strict";
  var CA = (root.CA = root.CA || {});

  /* ── 语言识别（扩展名表 + 关键词特征） ─────────────────────────────────── */
  var EXT_LANG = {
    ts: "TypeScript", tsx: "TypeScript", js: "JavaScript", jsx: "JavaScript", mjs: "JavaScript",
    py: "Python", pyw: "Python", rs: "Rust", go: "Go", java: "Java", cs: "C#",
    cpp: "C++", cc: "C++", cxx: "C++", hpp: "C++", hh: "C++", c: "C", h: "C",
    rb: "Ruby", php: "PHP", swift: "Swift", kt: "Kotlin", kts: "Kotlin",
    m: "Objective-C", mm: "Objective-C", scala: "Scala", sh: "Shell", bash: "Shell",
    ps1: "PowerShell", sql: "SQL", html: "HTML", css: "CSS", vue: "Vue",
    dart: "Dart", lua: "Lua", pl: "Perl", r: "R", jl: "Julia", hs: "Haskell"
  };
  /* 关键词特征：按命中数打分（无扩展名/纯文本时兜底） */
  var KEY_SIGNS = [
    ["Rust", /\bfn\s+\w+\s*\(/], ["Rust", /\blet\s+(mut\s+)?\w+\s*=/], ["Rust", /\bimpl\s+\w+/],
    ["Python", /\bdef\s+\w+\s*\(.*\)\s*:/], ["Python", /^\s*from\s+\w+\s+import\b/m],
    ["Go", /\bfunc\s+(\w+|\(\w+\s+\*?\w+\))\s*\(/], ["Go", /^package\s+\w+/m],
    ["Java", /\bpublic\s+(class|interface)\s+\w+/], ["Java", /\bSystem\.out\.print/],
    ["C#", /\busing\s+System\b/], ["C#", /\bnamespace\s+\w+/],
    ["TypeScript", /\binterface\s+\w+\s*\{/], ["TypeScript", /:\s*(string|number|boolean)\b/],
    ["JavaScript", /=>\s*\{?/], ["JavaScript", /\bconsole\.log\(/],
    ["Ruby", /\bdef\s+\w+\b/], ["Ruby", /\bputs\s+/],
    ["PHP", /<\?php/], ["Swift", /\bfunc\s+\w+\s*\(/], ["Swift", /\blet\s+\w+\s*(:|\s=)/],
    ["Kotlin", /\bfun\s+\w+\s*\(/], ["Kotlin", /\bval\s+\w+\s*=/],
    ["Shell", /^#!\/(bin|usr)\/(ba)?sh/m], ["PowerShell", /\$\w+\s*=\s*/],
    ["SQL", /\bSELECT\s+.+\bFROM\b/i], ["HTML", /<!doctype html>/i]
  ];

  function detectLang(fileName, sample) {
    var m = String(fileName || "").toLowerCase().match(/\.([a-z0-9]+)$/);
    if (m && EXT_LANG[m[1]]) return EXT_LANG[m[1]];
    var text = String(sample || "");
    if (text) {
      var score = {};
      for (var i = 0; i < KEY_SIGNS.length; i++) {
        var sign = KEY_SIGNS[i];
        if (sign[1] instanceof RegExp) {
          if (sign[1].test(text)) score[sign[0]] = (score[sign[0]] || 0) + 1;
        }
      }
      var best = "", bestN = 0;
      Object.keys(score).forEach(function (k) {
        if (score[k] > bestN) { best = k; bestN = score[k]; }
      });
      if (best) return best;
    }
    return "未知";
  }

  /* ── 归一化：任何形状 → { terms, syntax, count, skipped, syntaxCount } ── */
  function collectTerms(obj, out) {
    if (!obj || typeof obj !== "object") return;
    Object.keys(obj).forEach(function (k) {
      var v = obj[k];
      if (typeof v !== "string") return;
      var key = String(k).toLowerCase();
      if (out.terms[key] == null) { out.terms[key] = v; out.count++; }
      else out.skipped++;
    });
  }
  function collectSyntax(syn, out) {
    if (!syn || typeof syn !== "object") return;
    Object.keys(syn).forEach(function (lang) {
      var map = syn[lang];
      if (!map || typeof map !== "object") return;
      var l = String(lang).toLowerCase();
      if (!out.syntax[l]) out.syntax[l] = {};
      Object.keys(map).forEach(function (kw) {
        if (typeof map[kw] === "string" && out.syntax[l][kw] == null) {
          out.syntax[l][kw] = map[kw];
          out.syntaxCount++;
        }
      });
    });
  }
  function normalize(json, name) {
    var out = { name: name || "dict", terms: {}, syntax: {}, count: 0, skipped: 0, syntaxCount: 0 };
    if (!json || typeof json !== "object") return out;
    if (Array.isArray(json)) {
      json.forEach(function (it) {
        if (!it || typeof it !== "object") return;
        var t = it.term || it.name || it.k || it.word || it.key;
        var d = it.plain || it.explain || it.desc || it.meaning || it.definition || it.v;
        if (t && d && typeof d === "string") {
          var key = String(t).toLowerCase();
          if (out.terms[key] == null) { out.terms[key] = d; out.count++; }
          else out.skipped++;
        }
      });
      return out;
    }
    var keys = Object.keys(json);
    var strVals = keys.filter(function (k) { return typeof json[k] === "string"; });
    if (keys.length && strVals.length === keys.length) {
      collectTerms(json, out);                        /* ① 平铺映射 */
    } else {
      if (json.terms && typeof json.terms === "object") collectTerms(json.terms, out);   /* ② 标准段 */
      else if (json.dict && typeof json.dict === "object") collectTerms(json.dict, out);
      else if (json.syntax) { /* 只有 syntax 段 */ }
      else keys.forEach(function (k) {                /* ③ 分域嵌套 { "领域": {...} } */
        if (json[k] && typeof json[k] === "object" && !Array.isArray(json[k])) collectTerms(json[k], out);
      });
      if (json.syntax) collectSyntax(json.syntax, out);
    }
    return out;
  }

  /* ── 词典库：合并 + 查询 + 语法替换 + 释义 ────────────────────────────── */
  var D = { packs: [], terms: {}, syntax: {}, userTerms: {} };

  function load(json, name) {
    var p = normalize(json, name);
    /* 合并：先到先得（去重由构造保证），语法段同理 */
    Object.keys(p.terms).forEach(function (k) {
      if (D.terms[k] == null) D.terms[k] = p.terms[k];
      else p.skipped++;
    });
    Object.keys(p.syntax).forEach(function (l) {
      if (!D.syntax[l]) D.syntax[l] = {};
      Object.keys(p.syntax[l]).forEach(function (kw) {
        if (D.syntax[l][kw] == null) D.syntax[l][kw] = p.syntax[l][kw];
      });
    });
    D.packs.push(p);
    return p;
  }
  function loadUser(obj) {
    var p = normalize(obj, "我的补充");
    D.userTerms = p.terms;
    return p;
  }
  function lookup(term) {
    if (!term) return null;
    var k = String(term).toLowerCase();
    if (D.terms[k] != null) return D.terms[k];
    if (D.userTerms[k] != null) return D.userTerms[k];
    return null;
  }
  /* 语法替换：把语言关键词后缀大白话注释（未知语言原样返回） */
  function applySyntax(text, lang) {
    if (!text) return text || "";
    var l = String(lang || "").toLowerCase().split(/\s+/)[0];
    var s = D.syntax[l];
    if (!s) return text;
    var out = String(text);
    Object.keys(s).forEach(function (kw) {
      var re = new RegExp("\\b" + kw.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "\\b", "g");
      out = out.replace(re, kw + "·" + s[kw]);
    });
    return out;
  }
  /* 拆词：camelCase / snake / kebab → 小写词元 */
  function words(name) {
    return String(name || "")
      .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
      .split(/[^A-Za-z0-9\u4e00-\u9fa5]+/)
      .filter(Boolean)
      .map(function (w) { return w.toLowerCase(); });
  }
  /* 节点释义：全名 + 词元逐个查典（查到才输出，绝不硬凑） */
  function explainNode(n) {
    if (!n || !n.name) return "";
    var seen = {}, entries = [];
    var full = lookup(n.name);
    if (full) entries.push([n.name, full]);
    words(n.name).forEach(function (w) {
      if (seen[w] || w.length < 3) return;
      seen[w] = 1;
      var d = lookup(w);
      if (d && d !== full) entries.push([w, d]);
    });
    if (!entries.length) return "";
    var esc2 = function (s) { return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;"); };
    return '<div class="dict-sec">📘 ' + entries.map(function (e) {
      return "<b>" + esc2(e[0]) + "</b> —— " + esc2(e[1]);
    }).join("<br/>") + "</div>";
  }
  /* 重复内容检测：同层级同名节点分组（跨级别不算重复） */
  function dedupeNodes(ir) {
    var nodes = (ir && ir.nodes) || [];
    var map = {};
    nodes.forEach(function (n) {
      if (!n || n.name == null) return;
      var k = (n.kind == null ? "?" : n.kind) + "|" + n.name;
      if (!map[k]) map[k] = { name: n.name, kind: n.kind, count: 0, ids: [] };
      map[k].count++;
      if (map[k].ids.length < 8) map[k].ids.push(n.id);
    });
    var KIND = CA.KIND_NAME || ["project", "module", "subsystem", "file", "class", "func", "line"];
    return Object.keys(map)
      .map(function (k) { return map[k]; })
      .filter(function (g) { return g.count > 1; })
      .sort(function (a, b) { return b.count - a.count; })
      .slice(0, 20)
      .map(function (g) {
        g.levelName = KIND[g.kind] != null ? KIND[g.kind] : String(g.kind);
        return g;
      });
  }
  function stats() {
    return {
      packs: D.packs.map(function (p) {
        return { name: p.name, terms: p.count, skipped: p.skipped, syntax: p.syntaxCount };
      }),
      terms: Object.keys(D.terms).length,
      user: Object.keys(D.userTerms).length
    };
  }

  CA.Dict = {
    load: load,
    loadUser: loadUser,
    lookup: lookup,
    applySyntax: applySyntax,
    explainNode: explainNode,
    detectLang: detectLang,
    dedupeNodes: dedupeNodes,
    stats: stats,
    words: words,
    EXT_LANG: EXT_LANG
  };
})(typeof globalThis !== "undefined" ? globalThis : this);
