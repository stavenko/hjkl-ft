// One-off: dump chapters 1 & 2 of the story (RU) — section headings, body text,
// lists and tasks — into a single markdown file. Source of truth: story.yaml
// (structure) + i18n.rs (the RU strings).
//
//   node scripts/dump-story-text.mjs > docs/story-ch1-ch2.md

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const yaml = readFileSync(join(ROOT, "frontend/story/story.yaml"), "utf8");
const i18n = readFileSync(join(ROOT, "frontend/src/services/i18n.rs"), "utf8");

// ── RU string map: every `"key" => "value",` AFTER `fn ru(` ─────────────────
const ruStart = i18n.indexOf("fn ru(");
const ruBody = i18n.slice(ruStart);
const RU = {};
for (const m of ruBody.matchAll(/^\s*"([^"]+)"\s*=>\s*"((?:[^"\\]|\\.)*)",/gm)) {
  RU[m[1]] = m[2].replace(/\\"/g, '"').replace(/\\\\/g, "\\");
}
const tr = (k) =>
  (RU[k] ?? `«⟨нет строки: ${k}⟩»`).replace(/\s*\{dot\}/g, " 🔴").replace(/\s+([.,;:!?])/g, "$1");

// ── Task pool: id → RU title ─────────────────────────────────────────────────
const TASK = {};
{
  const pool = yaml.slice(yaml.indexOf("\ntasks:"), yaml.indexOf("\nchapters:"));
  let id = null;
  for (const line of pool.split("\n")) {
    const mId = /^\s*-\s*id:\s*(\S+)/.exec(line);
    if (mId) { id = mId[1]; continue; }
    const mT = /title:\s*\{.*ru:\s*"((?:[^"\\]|\\.)*)"\s*\}/.exec(line);
    if (mT && id) { TASK[id] = mT[1]; }
  }
}
// {n} placeholders → human-readable note for the doc.
const taskTitle = (id) => {
  let t = TASK[id] ?? id;
  if (id === "veg_streak") t = t.replace("{n}", "600 (мужчины) / 400 (женщины)");
  if (id === "protein_streak") t = t.replace("{n} г белка", "(1,2 × вес в кг) г белка");
  return t;
};

// ── Walk chapters ch1 & ch2, emit markdown ───────────────────────────────────
const lines = yaml.split("\n");
const out = ["# Истории: главы 1 и 2", ""];

const ruOf = (s) => {
  const m = /ru:\s*"((?:[^"\\]|\\.)*)"/.exec(s);
  return m ? m[1] : null;
};

let i = lines.findIndex((l) => /^chapters:/.test(l));
let inChapter = 0; // 0 none, 1 or 2
for (; i < lines.length; i++) {
  const line = lines[i];

  const chId = /^\s{2}-\s*id:\s*(ch\d+|ch1|ch2|ch3)\s*$/.exec(line);
  if (/^\s{2}-\s*id:\s*(ch\d+)\s*$/.test(line)) {
    const cid = /id:\s*(\S+)/.exec(line)[1];
    if (cid === "ch1") inChapter = 1;
    else if (cid === "ch2") inChapter = 2;
    else inChapter = 99; // ch3+ → stop after we finish
    if (inChapter === 99) break;
    // chapter title is on the next line
    const title = ruOf(lines[i + 1]);
    out.push(`## Глава ${inChapter}. ${title}`, "");
    continue;
  }
  if (!inChapter) continue;

  // section start: 6-space indent "- id:"
  if (/^\s{6}-\s*id:\s*\S+/.test(line)) {
    // title is within the next 2 lines
    let title = null, icon = "";
    for (let j = i + 1; j <= i + 3 && j < lines.length; j++) {
      const ic = /icon:\s*"([^"]*)"/.exec(lines[j]);
      if (ic) icon = ic[1];
      const t = /title:\s*\{.*ru:\s*"((?:[^"\\]|\\.)*)"/.exec(lines[j]);
      if (t) { title = t[1]; break; }
    }
    out.push(`### ${icon ? icon + " " : ""}${title}`, "");
    // collect this section's tasks list (the `tasks: [..]` line)
    continue;
  }

  // tasks: [a, b, c]
  const mTasks = /^\s{8}tasks:\s*\[([^\]]*)\]/.exec(line);
  if (mTasks) {
    const ids = mTasks[1].split(",").map((s) => s.trim()).filter(Boolean);
    // hold for emission after blocks — but simplest: stash on a pending var
    pendingTasks = ids;
    continue;
  }

  // blocks
  const text = /^\s{10}-\s*\{text_key:\s*(\S+?)\}/.exec(line);
  if (text) { out.push(tr(text[1]), ""); continue; }

  const heading = /^\s{10}-\s*\{heading:\s*(\S+?)\}/.exec(line);
  if (heading) { out.push(`#### ${tr(heading[1])}`, ""); continue; }

  const bullets = /^\s{10}-\s*\{bullets:\s*\[([^\]]*)\]\}/.exec(line);
  if (bullets) {
    for (const k of bullets[1].split(",").map((s) => s.trim()).filter(Boolean)) out.push(`- ${tr(k)}`);
    out.push("");
    continue;
  }

  const list = /^\s{10}-\s*\{list:\s*\[([^\]]*)\]\}/.exec(line);
  if (list) {
    let n = 1;
    for (const k of list[1].split(",").map((s) => s.trim()).filter(Boolean)) out.push(`${n++}. ${tr(k)}`);
    out.push("");
    continue;
  }

  const tasksBlock = /^\s{10}-\s*\{tasks:\s*true\}/.exec(line);
  if (tasksBlock) {
    out.push("**Задание:**");
    for (const id of pendingTasks) out.push(`- ${taskTitle(id)}`);
    out.push("");
    pendingTasks = [];
    continue;
  }
  // widgets and everything else: ignore
}

// sections that carry tasks but have no {tasks: true} block (none in ch1/2) —
// flush any leftover at section boundaries is unnecessary here.

var pendingTasks = [];
process.stdout.write(out.join("\n") + "\n");
