/* Revert the aurora look-changes: remove sweep ray, dome breathe, hue drift.
 * Keeps the anti-plastic clarity work (full-res aurora, texture striations,
 * reduced bloom). */
const fs = require("fs");
const f = "src/features/background/starfield/shaders.ts";
let s = fs.readFileSync(f, "utf8");
let ok = 0;
const fail = [];
function rep(from, to, tag) {
  if (!s.includes(from)) { fail.push(tag); return; }
  s = s.replace(from, to);
  ok++;
}

// 1. Remove the sweeping beam block.
rep(
  "  // Sweeping beam drifting inside the dome — a slow searchlight of light.\n" +
    "  float sweepRay = pow(max(0.0, 1.0 - abs(uv.x - (0.46 + 0.17 * sin(t * 0.07)))), 6.0);\n" +
    "  acc.rgb *= 1.0 + 0.20 * sweepRay * step(0.5, uAurora);\n",
  "",
  "SWEEP"
);

// 2. Revert dome breathe.
rep(
  "vec3 domeShape = vec3(0.36, 0.66 * (1.0 + 0.03 * sin(t * 0.05)), 1.70);",
  "vec3 domeShape = vec3(0.36, 0.66, 1.70);",
  "BREATHE"
);

// 3. Revert hue drift.
rep(
  ", 0.50 * surge, 0.18 + 0.10 * sin(t * 0.02), 0.45,",
  ", 0.50 * surge, 0.18, 0.45,",
  "HUE"
);

fs.writeFileSync(f, s);
console.log("OK:", ok, "FAIL:", JSON.stringify(fail));
