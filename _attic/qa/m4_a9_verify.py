# -*- coding: utf-8 -*-
# M4-A9: theme switch - dark baseline -> DB write light -> taskbar reload ->
# light screenshot -> restore dark. Validates DB->frontend->CSS full chain.
import ctypes, json, sqlite3, time, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from m4_a6_probe import CDP, JS_STATE, move_cursor, u32, grab

DB = os.path.join(os.environ["APPDATA"], "com.variable.app", "db", "variable.db")

def db_theme(v=None):
    conn = sqlite3.connect(DB, timeout=3)
    try:
        if v is None:
            row = conn.execute("SELECT value FROM settings WHERE key='theme'").fetchone()
            return row[0] if row else None
        conn.execute(
            "INSERT INTO settings(key,value,version,updated_at) VALUES('theme',?1,1,strftime('%s','now')*1000) "
            "ON CONFLICT(key) DO UPDATE SET value=excluded.value, version=version+1, updated_at=excluded.updated_at",
            (v,))
        conn.commit()
    finally:
        conn.close()

JS_THEME = """(() => {
  const cs = getComputedStyle(document.documentElement);
  const tb = document.querySelector('.taskbar');
  const tbs = tb ? getComputedStyle(tb) : null;
  return JSON.stringify({
    theme: document.documentElement.dataset.theme,
    bodyBg: cs.backgroundColor,
    tbBg: tbs ? tbs.backgroundColor : null,
    tbColor: tbs ? tbs.color : null
  });
})()"""

def reload_taskbar(cdp):
    cdp.mid += 1
    cdp.send({"id": cdp.mid, "method": "Page.reload"})
    time.sleep(3.5)
    # consume reload re-negotiation; JS_STATE still works after reload

def main():
    rep = {}
    cdp = CDP()
    sw = u32.GetSystemMetrics(0); sh = u32.GetSystemMetrics(1)

    # ensure taskbar visible & cursor away for clean shots
    move_cursor(sw // 2, sh // 3)
    time.sleep(0.5)

    rep["db_theme_before"] = db_theme()
    rep["ui_before"] = json.loads(cdp.evaluate(JS_THEME))
    grab("_attic/qa/m4_a9_1_dark.png")

    # switch to light via DB (the real persistence layer)
    db_theme("light")
    rep["db_theme_written"] = db_theme()
    reload_taskbar(cdp)
    move_cursor(sw // 2, sh // 3)
    time.sleep(1.0)
    rep["ui_after_light"] = json.loads(cdp.evaluate(JS_THEME))
    grab("_attic/qa/m4_a9_2_light.png")

    # light theme visual: summon taskbar over it too
    move_cursor(sw // 2, sh - 3)
    time.sleep(1.2)
    rep["light_summoned"] = json.loads(cdp.evaluate(JS_STATE))["rootClass"]
    grab("_attic/qa/m4_a9_3_light_summoned.png")
    move_cursor(sw // 2, sh // 2)

    # restore dark
    db_theme("dark")
    reload_taskbar(cdp)
    time.sleep(1.0)
    rep["ui_restored"] = json.loads(cdp.evaluate(JS_THEME))
    print(json.dumps(rep, ensure_ascii=False, indent=1))

if __name__ == "__main__":
    main()
