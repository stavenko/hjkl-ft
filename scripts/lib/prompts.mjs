// ПРОМПТЫ ДЛЯ ЗАМЕРОВ БЕРУТСЯ ИЗ КОДА, а не переписываются руками.
//
// Рукописные копии дважды разошлись с приложением, и оба раза расхождение было
// невидимым — обе стороны «работали», просто по-разному. Сначала в коде оказалось
// три поля опознания против одного в копии. Потом выяснилось, что ai-worker вклеивает
// в промпт СХЕМУ, которой в копии не было вовсе, и именно её служебные поля
// пропускали выдуманные слова как еду: «Бубурек копчёный — a type of smoked bread».
//
// Файл `scripts/prompts.json` пишет тест `write_prompts_json` — он строит промпты и
// схемы теми же функциями, что и приложение. Обновить: cd frontend && cargo test
// write_prompts_json.
import { readFileSync } from "node:fs";

const PATH = new URL("../prompts.json", import.meta.url);

let cache = null;

function load() {
  if (cache) return cache;
  try {
    cache = JSON.parse(readFileSync(PATH, "utf8"));
  } catch (e) {
    throw new Error(
      `нет scripts/prompts.json (${e.message}). Собрать: cd frontend && cargo test write_prompts_json`,
    );
  }
  return cache;
}

/// Промпт и схема узла с подставленным продуктом и опознанием.
///
/// `group` — «flags» или «nutrients», `node` — имя узла в выгрузке.
export function promptFor(group, node, food, identity = "") {
  const all = load();
  const entry = all[group]?.[node];
  if (!entry) {
    const have = Object.keys(all[group] ?? {}).join(", ");
    throw new Error(`нет узла ${group}.${node}; есть: ${have}`);
  }
  const prompt = entry.prompt
    .replaceAll("{{FOOD}}", food)
    .replaceAll("{{IDENTITY}}", identity);
  return { prompt, schema: entry.schema };
}

/// Список узлов в выгрузке — чтобы замер мог проверить, что берёт существующее.
export function nodes(group) {
  return Object.keys(load()[group] ?? {});
}
