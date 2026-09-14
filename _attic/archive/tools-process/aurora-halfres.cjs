/* Half-res aurora pipeline: aurora+clouds+meteors render into a half-res
 * target, then composite over the full-res sky+stars with one quad.
 * Also: 3-octave fbm inside the aurora pass, and AURCOMP_FS shader. */
const fs = require("fs");
const F = "src/features/background/starfield/shaders.ts";
let s = fs.readFileSync(F, "utf8");
let ok = 0;
const fail = [];
function rep(from, to, tag) {
  if (!s.includes(from)) { fail.push(tag); return; }
  s = s.replace(from, to);
  ok++;
}

// 1. fbm3o definition right after the aurora shader's noise include.
rep(
  "${NOISE_GLSL}\n\n// Photoreal aurora ladder",
"${NOISE_GLSL}\n" +
"\n" +
"// 3-octave fbm — the aurora pass covers the whole sky, so every octave\n" +
"// counts. The soft plasmatic look loses nothing at three octaves.\n" +
"float fbm3o(vec2 p) {\n" +
"  float v = 0.0;\n" +
"  float amp = 0.55;\n" +
"  for (int i = 0; i < 3; i++) {\n" +
"    v += amp * vnoise(p);\n" +
"    p = p * 2.13 + vec2(11.3, 7.9);\n" +
"    amp *= 0.5;\n" +
"  }\n" +
"  return v;\n" +
"}\n" +
"\n" +
"// Photoreal aurora ladder",
  "FBM3O"
);

// 2. Swap the heavy fbm calls inside AURORA_FS (between its marker and STAR_VS).
{
  const a = s.indexOf("export const AURORA_FS");
  const b = s.indexOf("export const STAR_VS");
  let aur = s.slice(a, b);
  const before = (aur.match(/fbm\(/g) || []).length;
  aur = aur.replace(/fbm\(/g, "fbm3o(");
  s = s.slice(0, a) + aur + s.slice(b);
  ok++;
  console.log("aurora fbm swapped:", before);
}

// 3. AURCOMP_FS shader (scene + half-res aurora composite).
const compAnchor = "export const STAR_VS";
const aurcomp = `// Aurora composite: full-res sky+stars under the half-res aurora layer
// (premultiplied over). Linear upscaling doubles as free soft blur — the
// aurora is plasmatic light, so the half-res render is visually lossless.
export const AURCOMP_FS = \`#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform sampler2D uScene;
uniform sampler2D uAur;
void main() {
  vec3 c = texture(uScene, vUv).rgb;
  vec4 a = texture(uAur, vUv);
  outColor = vec4(a.rgb + c * (1.0 - a.a), 1.0);
}\`;

`;
if (!s.includes(compAnchor)) { fail.push("AURCOMP anchor"); }
else { s = s.replace(compAnchor, aurcomp + compAnchor); ok++; }

fs.writeFileSync(F, s);
console.log("shaders OK:", ok, "FAIL:", JSON.stringify(fail));
