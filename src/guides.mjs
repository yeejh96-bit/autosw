import fs from 'node:fs';
import path from 'node:path';
import { packageRoot } from './workspace.mjs';

export function guide(topic = '') {
  const base = path.join(packageRoot, 'skills', 'autosw-document');
  const files = [];
  function visit(dir) {
    for (const item of fs.readdirSync(dir, { withFileTypes: true })) {
      if (item.isDirectory()) visit(path.join(dir, item.name));
      else if (item.name.endsWith('.md')) files.push(path.relative(base, path.join(dir, item.name)).replaceAll('\\', '/'));
    }
  }
  visit(base);
  const selected = topic || 'SKILL.md';
  if (!files.includes(selected)) return { available: files.sort(), error: '목록의 정확한 문서 경로를 topic으로 지정하세요.' };
  return { topic: selected, content: fs.readFileSync(path.join(base, selected), 'utf8'), available: files.sort() };
}
