// Диагностика графа codemap по индексу king_orch_3.
// Входные данные — .codemap/index.json, тот же формат, что читает store.rs.
const fs = require("fs");
const index = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));

const definers = new Map();   // имя -> [файлы, где определено]
const refsIn = new Map();     // файл  -> (имя -> сколько раз упомянуто)
for (const tag of index.tags) {
  if (tag.kind === "Def") {
    let list = definers.get(tag.name);
    if (!list) { list = []; definers.set(tag.name, list); }
    list.push(tag.rel_path);
  } else {
    let map = refsIn.get(tag.rel_path);
    if (!map) { map = new Map(); refsIn.set(tag.rel_path, map); }
    map.set(tag.name, (map.get(tag.name) || 0) + 1);
  }
}

const importance = (name, definerCount) => {
  let mul = 1;
  const snake = name.includes("_"), kebab = name.includes("-");
  const camel = /[A-Z]/.test(name) && /[a-z]/.test(name);
  if ((snake || kebab || camel) && name.length >= 8) mul *= 10;
  if (name.startsWith("_")) mul *= 0.1;
  if (definerCount > 5) mul *= 0.1;
  return mul;
};

const inByFile = new Map();   // файл -> сумма веса входящих рёбер
const outByFile = new Map();  // файл -> сумма веса исходящих рёбер
const edgesTo = new Map();    // "дефинер|референсер" -> вес

for (const [name, targets] of definers) {
  const mul = importance(name, targets.length);
  for (const [referencer, refs] of refsIn) {
    const count = refs.get(name);
    if (!count) continue;
    const weight = mul * Math.sqrt(count);
    outByFile.set(referencer, (outByFile.get(referencer) || 0) + weight);
    for (const definer of targets) {
      const key = definer + "|" + referencer;
      edgesTo.set(key, (edgesTo.get(key) || 0) + weight);
    }
  }
}
for (const key of edgesTo.keys()) {
  const definer = key.slice(0, key.indexOf("|"));
  inByFile.set(definer, (inByFile.get(definer) || 0) + edgesTo.get(key));
}

const total = (map) => [...map.values()].reduce((a, b) => a + b, 0);
const share = (map, n) => [...map].sort((a, b) => b[1] - a[1]).slice(0, n)
  .map(([p, w]) => `${((100 * w) / total(map)).toFixed(2)}%  ${p}`).join("\n");

console.log("=== сумма ВХОДЯЩИХ рёбер (куда течёт ранг) ===");
console.log(share(inByFile, 12));
console.log("\n=== сумма ИСХОДЯЩИХ рёбер (от кого течёт ранг) ===");
console.log(share(outByFile, 12));
console.log("\n=== кто ссылается на searxng_search.ts ===");
for (const [key, w] of [...edgesTo].sort((a, b) => b[1] - a[1]).slice(0, 40)) {
  if (key.startsWith("src-tauri/mcp_servers/searxng_search.ts")) console.log(w.toFixed(1), key);
}
console.log("\n=== кто ссылается на orchestrator/mod.rs ===");
for (const [key, w] of [...edgesTo].sort((a, b) => b[1] - a[1]).slice(0, 200)) {
  if (key.startsWith("src-tauri/src/domain/orchestrator/mod.rs")) console.log(w.toFixed(1), key);
}