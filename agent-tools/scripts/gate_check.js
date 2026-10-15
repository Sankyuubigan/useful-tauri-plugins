// Проверка гейтов 8-11 по готовому файлу карты.
// Запуск: node gate_check.js <карта> <бюджет_в_токенах>
const fs = require("fs");
const [, , mapPath, budgetArg] = process.argv;
const budget = Number(budgetArg);
const text = fs.readFileSync(mapPath, "utf8").replace(/\r/g, "");

const HEADER = "## 📁 Структура проекта";
const FILE_HEAD = /^\S.*\.(?:rs|ts|tsx|js|jsx|cjs|mjs|py):$/;
const lines = text.split("\n");

// Граница трёх частей: шапка, инвентарь, блоки кода.
// Инвентарь заканчивается там, где начинается первый блок файла.
const firstBlock = lines.findIndex((l) => FILE_HEAD.test(l));
const headerEnd = lines.findIndex((l) => l.startsWith(HEADER));
const header = lines.slice(0, headerEnd + 1).join("\n");
const inventory = headerEnd >= 0 && firstBlock > headerEnd
  ? lines.slice(headerEnd, firstBlock).join("\n")
  : "";
const body = firstBlock >= 0 ? lines.slice(firstBlock).join("\n") : "";

// Блоки файлов: заголовок `путь:` и следующие строки кода.
const blocks = [];
let current = null;
for (const line of body.split("\n")) {
  const head = /^(\S.*\.(?:rs|ts|tsx|js|jsx|cjs|mjs|py)):$/.exec(line);
  if (head) {
    current = { path: head[1], text: "" };
    blocks.push(current);
    continue;
  }
  if (current) current.text += line + "\n";
}
const estimate = (s) => Math.max(0, Math.round(s.length / 4));
const total = estimate(text);

console.log(`бюджет: ${budget}, карта: ~${total} токенов`);
console.log(`\nГЕЙТ 8 — широта`);
console.log(`  разных файлов в карте: ${blocks.length} (нужно >= 10) — ${blocks.length >= 10 ? "ПРОЙДЕН" : "НЕ ПРОЙДЕН"}`);
for (const needle of ["orchestrator", "workflow_engine", "fact_extractor"]) {
  console.log(`  содержит «${needle}»: ${text.includes(needle) ? "да" : "НЕТ"}`);
}

console.log(`\nГЕЙТ 9 — верх карты без импортов и полей класса`);
const forbiddenSymbols = ["invoke", "openDialog", "store", "bus", "logFront", "trackError"];
const forbiddenFields = ["sessionId", "history", "uidList"];
const importLines = [];
for (const block of blocks.slice(0, 20)) {
  for (const line of block.text.split("\n")) {
    if (/^\s*\d+ \|\s*(import|export)\s/.test(line)) importLines.push(`${block.path}: ${line.trim()}`);
  }
}
console.log(`  строк импорта среди первых 20 файлов: ${importLines.length} — ${importLines.length === 0 ? "ПРОЙДЕН" : "НЕ ПРОЙДЕН"}`);
importLines.slice(0, 5).forEach((l) => console.log("    " + l));
let fieldHits = 0;
for (const block of blocks.slice(0, 20)) {
  for (const line of block.text.split("\n")) {
    const code = line.replace(/^\s*\d+ \|\s*/, "");
    if (forbiddenSymbols.some((s) => new RegExp(`\\b(function|const|class|let|var)\\s+${s}\\b`).test(code))) fieldHits += 1;
    if (forbiddenFields.some((f) => new RegExp(`^\\s*(private |public |readonly |static )*${f}\\??\\s*[:=]`).test(code))) fieldHits += 1;
  }
}
console.log(`  определений с именами импортов или полей: ${fieldHits} — ${fieldHits === 0 ? "ПРОЙДЕН" : "НЕ ПРОЙДЕН"}`);

console.log(`\nГЕЙТ 10 — потолок на файл`);
let worst = { path: "", share: 0 };
for (const block of blocks) {
  const share = (estimate(block.text) / total) * 100;
  if (share > worst.share) worst = { path: block.path, share };
}
console.log(`  максимальная доля: ${worst.share.toFixed(1)}% (${worst.path}) — ${worst.share <= 15 ? "ПРОЙДЕН" : "НЕ ПРОЙДЕН"}`);

console.log(`\nГЕЙТ 11 — инвентарь`);
if (!inventory) {
  console.log("  секция отсутствует — НЕ ПРОЙДЕН");
} else {
  const share = (estimate(inventory) / total) * 100;
  console.log(`  доля карты: ${share.toFixed(1)}% (цель 10-15%) — ${share <= 15 ? "верхняя граница соблюдена" : "НЕ ПРОЙДЕН"}`);
  const dirs = inventory.split("\n").filter((l) => / — \d+ ф\.,/.test(l));
  console.log(`  каталогов показано: ${dirs.length}`);
  dirs.forEach((l) => console.log("    " + l));
}