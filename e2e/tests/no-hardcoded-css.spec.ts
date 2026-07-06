import { test, expect } from '@playwright/test';
import * as fs from 'node:fs';
import * as path from 'node:path';

const FRONTEND_SRC = path.resolve(__dirname, '../../frontend/src');

function collectRsFiles(dir: string): string[] {
  const results: string[] = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...collectRsFiles(full));
    } else if (entry.isFile() && entry.name.endsWith('.rs')) {
      results.push(full);
    }
  }
  return results;
}

const HEX_COLOR_RE = /#[0-9a-fA-F]{3,8}\b/g;
const NAMED_COLOR_RE = /\b(background|color|border(?:-color)?)\s*:\s*(white|black|grey|gray|red|green|blue|yellow|orange|purple|pink)\b/gi;
const RGB_HSL_RE = /\b(rgb|hsl)a?\s*\(/gi;
const STYLE_ATTR_RE = /style\s*=\s*"([^"]*)"/gi;

function isSvgLine(line: string): boolean {
  return /(?:fill|stroke)\s*=\s*"currentColor"|xmlns=|viewBox=|<svg\b/.test(line);
}

interface Violation {
  file: string;
  line: number;
  text: string;
  match: string;
}

function stripBoxShadows(value: string): string {
  return value.replace(/box-shadow\s*:[^;]*/gi, '');
}

function findColorViolations(value: string): string[] {
  const cleaned = stripBoxShadows(value);
  const found: string[] = [];
  let m;

  HEX_COLOR_RE.lastIndex = 0;
  while ((m = HEX_COLOR_RE.exec(cleaned)) !== null) found.push(m[0]);

  NAMED_COLOR_RE.lastIndex = 0;
  while ((m = NAMED_COLOR_RE.exec(cleaned)) !== null) found.push(m[0]);

  RGB_HSL_RE.lastIndex = 0;
  while ((m = RGB_HSL_RE.exec(cleaned)) !== null) found.push(m[0]);

  return found;
}

function scanFile(filePath: string, relBase: string): Violation[] {
  const content = fs.readFileSync(filePath, 'utf-8');
  const lines = content.split('\n');
  const rel = path.relative(relBase, filePath);
  const violations: Violation[] = [];
  let inStyleBlock = false;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (isSvgLine(line)) continue;

    if (line.includes('<style>') || line.includes('<style>"')) inStyleBlock = true;

    if (inStyleBlock) {
      const colors = findColorViolations(line);
      for (const c of colors) {
        violations.push({ file: rel, line: i + 1, text: line.trim(), match: c });
      }
    }

    STYLE_ATTR_RE.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = STYLE_ATTR_RE.exec(line)) !== null) {
      const colors = findColorViolations(m[1]);
      for (const c of colors) {
        violations.push({ file: rel, line: i + 1, text: line.trim(), match: c });
      }
    }

    if (line.includes('</style>') || line.includes('"</style>')) inStyleBlock = false;
  }

  return violations;
}

function formatReport(violations: Violation[]): string {
  return violations
    .map((v) => `  ${v.file}:${v.line}  color=${v.match}\n    ${v.text}`)
    .join('\n');
}

test.describe('No hardcoded CSS colors in .rs frontend source', () => {
  test('inline styles and <style> blocks must not contain hardcoded colors', () => {
    const rsFiles = collectRsFiles(FRONTEND_SRC);
    expect(rsFiles.length).toBeGreaterThan(0);

    const violations: Violation[] = [];
    for (const f of rsFiles) {
      violations.push(...scanFile(f, FRONTEND_SRC));
    }

    if (violations.length > 0) {
      console.log(`\nFound ${violations.length} hardcoded color(s):\n${formatReport(violations)}\n`);
    }

    expect(violations, `${violations.length} hardcoded color(s) found`).toHaveLength(0);
  });
});
