// Подставить УСТАНОВЛЕННЫЙ chromium всем, кто зовёт `chromium.launch()` без пути.
//
// В части окружений (в том числе здесь) установленный браузер не совпадает с тем,
// который ждёт закреплённая версия playwright: он лежит по фиксированному пути, а
// скачивать второй запрещено. Свои проверки берут его через `launchBrowser()`, но
// накопленных в репозитории десятки, и переписывать каждую ради прогона —
// не то изменение, которого они просят.
//
// Подключается только прогоном регрессии:
//   NODE_OPTIONS='--import ./scripts/lib/pin-browser.mjs'
// Ничего не делает, если пути нет: на машине с обычной установкой playwright
// сам знает, что запускать.
import { existsSync } from 'node:fs';

const pinned = process.env.PW_CHROMIUM ?? '/opt/pw-browsers/chromium';
if (existsSync(pinned)) {
  const pw = await import('playwright');
  for (const kind of ['chromium']) {
    const type = pw[kind];
    const launch = type.launch.bind(type);
    type.launch = (opts = {}) => launch({ executablePath: pinned, ...opts });
  }
}
