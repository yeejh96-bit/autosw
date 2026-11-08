import crypto from 'node:crypto';
import { readZipStrict, patchZip, parseOfficeXml, isHp, childrenHp, ancestorHp, paragraphText, xmlEscape, checkXmlText } from './office-zip.mjs';

export const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
// BOM도 원문의 일부로 보존하여 수정 영역 밖 바이트가 달라지지 않게 한다.
const utf8 = bytes => new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
function integer(value, positive = false) {
  if (!/^\d+$/.test(value ?? '') || !Number.isSafeInteger(Number(value)) || Number(value) < (positive ? 1 : 0)) throw new Error('표 주소·크기·병합 정보가 잘못됐습니다.');
  return Number(value);
}
function plainParagraph(p) {
  // 들여쓰기·스타일·글자 모양은 유지. 혼합 서식과 제어 문자는 앱 편집에 맡긴다.
  if (ancestorHp(p, 'ctrl') || ancestorHp(p, 'header') || ancestorHp(p, 'footer') || ancestorHp(p, 'drawText')) return '머리말·꼬리말·제어 요소의 문단';
  for (let node = p.parent; node; node = node.parent) {
    if (isHp(node, 'tc') && ['1', 'true'].includes(node.attrs.protect)) return '보호된 셀';
  }
  if (p.children.some(c => !isHp(c, 'run') && !isHp(c, 'linesegarray'))) return '지원하지 않는 문단 요소';
  const runs = childrenHp(p, 'run');
  if (!runs.length || runs.some(r => r.children.length > 1 || (r.children.length && !isHp(r.children[0], 't')) || r.parts.some(x => typeof x === 'string' && x.trim()))) return '그림·표·제어 요소 또는 잘못된 run';
  const style = JSON.stringify(runs[0].attrs);
  if (runs.some(r => JSON.stringify(r.attrs) !== style)) return '혼합 글자 서식';
  if (runs.some(r => r.children[0]?.children.some(c => !isHp(c, 'lineBreak')))) return '텍스트 안의 탭·제어 요소';
  return null;
}
export function loadHwpx(bytes) {
  const archive = readZipStrict(bytes);
  if (utf8(archive.read('mimetype') ?? Buffer.alloc(0)).trim() !== 'application/hwp+zip') throw new Error('HWPX mimetype이 없습니다.');
  if (archive.entries.some(e => /encrypt|signature|scripts\/|docoptions\/(?:password|drm)/i.test(e.name))) throw new Error('암호화·서명·스크립트 문서는 지원하지 않습니다.');
  const names = archive.entries.map(e => e.name).filter(n => /^Contents\/section\d+\.xml$/.test(n)).sort((a, b) => Number(a.match(/\d+/)[0]) - Number(b.match(/\d+/)[0]));
  if (!names.length) throw new Error('HWPX 본문이 없습니다.');
  const sections = [], internals = new Map(), targets = new Map();
  for (const name of names) {
    const data = archive.read(name);
    if (data.length > 32 * 1024 * 1024) throw new Error('본문 XML 크기 한도를 넘었습니다.');
    const parsed = parseOfficeXml(utf8(data));
    const ps = parsed.nodes.filter(n => isHp(n, 'p')), ts = parsed.nodes.filter(n => isHp(n, 'tbl'));
    const pKeys = new Map(ps.map((p, i) => [p, `${name}/p${i}`]));
    const tKeys = new Map(ts.map((t, i) => [t, `${name}/tbl${i}`]));
    const cKeys = new Map();
    const tables = ts.map(t => {
      const key = tKeys.get(t), rows = integer(t.attrs.rowCnt, true), cols = integer(t.attrs.colCnt, true);
      if (rows * cols > 1000000) throw new Error('표 크기 한도를 넘었습니다.');
      const occupied = new Set(), cells = [];
      for (const tr of childrenHp(t, 'tr')) for (const c of childrenHp(tr, 'tc')) {
        const addr = childrenHp(c, 'cellAddr'), span = childrenHp(c, 'cellSpan');
        if (addr.length !== 1 || span.length !== 1) throw new Error('셀 주소 또는 병합 정보가 없습니다.');
        const row = integer(addr[0].attrs.rowAddr), col = integer(addr[0].attrs.colAddr), rowSpan = integer(span[0].attrs.rowSpan, true), colSpan = integer(span[0].attrs.colSpan, true);
        if (row + rowSpan > rows || col + colSpan > cols) throw new Error('셀 병합이 표 밖으로 나갑니다.');
        for (let r = row; r < row + rowSpan; r++) for (let cc = col; cc < col + colSpan; cc++) {
          const at = `${r},${cc}`; if (occupied.has(at)) throw new Error('겹치는 표 셀입니다.'); occupied.add(at);
        }
        const cellKey = `${key}/r${row}c${col}`;
        cKeys.set(c, cellKey);
        const sublists = childrenHp(c, 'subList'), paragraphs = sublists.flatMap(s => childrenHp(s, 'p'));
        const childTables = ts.filter(child => ancestorHp(child, 'tc') === c).map(child => tKeys.get(child));
        cells.push({ key: cellKey, row, col, rowSpan, colSpan, paragraphs: paragraphs.map(p => pKeys.get(p)), childTables });
        let reason = sublists.length !== 1 || sublists[0].children.some(n => !isHp(n, 'p')) ? '복잡한 셀 본문' : childTables.length ? '중첩표를 포함한 부모 셀' : paragraphs.map(plainParagraph).find(Boolean) ?? null;
        if (!paragraphs.length) reason = '입력 문단 없음';
        targets.set(cellKey, { key: cellKey, kind: 'cell', section: name, node: c, paragraphs, before: paragraphs.map(paragraphText), writable: !reason, reason });
      }
      return { key, id: t.attrs.id ?? '', rows, cols, parentCell: null, cells };
    });
    tables.forEach((t, i) => { t.parentCell = cKeys.get(ancestorHp(ts[i], 'tc')) ?? null; });
    const paragraphs = ps.map(p => {
      const key = pKeys.get(p), reason = plainParagraph(p);
      targets.set(key, { key, kind: 'paragraph', section: name, node: p, paragraphs: [p], before: [paragraphText(p)], writable: !reason, reason });
      return { key, text: paragraphText(p), cell: cKeys.get(ancestorHp(p, 'tc')) ?? null };
    });
    sections.push({ name, paragraphs, tables }); internals.set(name, parsed);
  }
  return { bytes, archive, internals, targets, structure: { schemaVersion: 1, sourceSha256: sha256(bytes), sections } };
}
export function inspectHwpx(bytes) {
  const doc = loadHwpx(bytes);
  return { ...doc.structure, targets: [...doc.targets.values()].map(({ key, kind, before, writable, reason }) => ({ key, kind, before, writable, reason })), visualStatus: 'not-checked' };
}
function textElement(xml, t, value) {
  const prefix = t.name.includes(':') ? t.name.split(':')[0] + ':' : '';
  const content = value.split('\n').map(xmlEscape).join(`<${prefix}lineBreak/>`);
  if (t.selfClosing) return xml.slice(t.start, t.openEnd).replace(/\/\s*>$/, '>') + content + `</${t.name}>`;
  return xml.slice(t.start, t.openEnd) + content + xml.slice(t.closeStart, t.end);
}
function paragraphReplacement(xml, p, value) {
  const runs = childrenHp(p, 'run'), edits = [];
  runs.forEach((r, i) => {
    const t = r.children[0], newValue = i === 0 ? value : '';
    if (t) edits.push({ start: t.start, end: t.end, value: textElement(xml, t, newValue) });
    else {
      const prefix = r.name.includes(':') ? r.name.split(':')[0] + ':' : '';
      const content = `<${prefix}t>` + newValue.split('\n').map(xmlEscape).join(`<${prefix}lineBreak/>`) + `</${prefix}t>`;
      const replacement = r.selfClosing ? xml.slice(r.start, r.end).replace(/\/\s*>$/, '>') + content + `</${r.name}>` : xml.slice(r.start, r.openEnd) + content + xml.slice(r.closeStart, r.end);
      edits.push({ start: r.start, end: r.end, value: replacement });
    }
  });
  // 줄 배치 캐시는 수정한 문단에서만 제거. 실제 한글 재배치는 별도 검증한다.
  for (const n of childrenHp(p, 'linesegarray')) edits.push({ start: n.start, end: n.end, value: '' });
  let text = xml.slice(p.start, p.end);
  for (const e of edits.sort((a, b) => b.start - a.start)) text = text.slice(0, e.start - p.start) + e.value + text.slice(e.end - p.start);
  return text;
}
function exactKeys(obj, allowed, label) {
  if (!obj || typeof obj !== 'object' || Array.isArray(obj) || Object.keys(obj).some(k => !allowed.includes(k))) throw new Error(`${label}에 잘못된 항목이 있습니다.`);
}
export function prepareHwpx(bytes, request) {
  exactKeys(request, ['schemaVersion', 'sourceSha256', 'changes', 'requiredTargets', 'unmapped'], '입력');
  const doc = loadHwpx(bytes);
  if (request.schemaVersion !== 1 || request.sourceSha256 !== doc.structure.sourceSha256) throw new Error('원본 버전이 달라졌습니다. 구조와 미리보기를 다시 확인하세요.');
  if (!Array.isArray(request.changes) || !request.changes.length) throw new Error('입력할 변경 항목이 없습니다.');
  if (request.unmapped !== undefined && (!Array.isArray(request.unmapped) || request.unmapped.length)) throw new Error('입력 위치를 정하지 못한 항목이 남아 있습니다.');
  if (request.requiredTargets !== undefined && (!Array.isArray(request.requiredTargets) || request.requiredTargets.some(k => typeof k !== 'string' || !request.changes.some(c => c.target === k)))) throw new Error('필수 입력 위치의 조치가 누락됐습니다.');
  const edits = new Map(), touched = new Set(), preview = [];
  for (const change of request.changes) {
    exactKeys(change, ['target', 'label', 'source', 'action', 'expected', 'values'], '변경');
    const target = doc.targets.get(change.target);
    if (!target) throw new Error(`입력 위치가 없습니다: ${change.target}`);
    if (!target.writable && change.action !== 'preserve') throw new Error(`지원하지 않는 입력 위치: ${change.target} (${target.reason})`);
    if (typeof change.label !== 'string' || !change.label.trim() || typeof change.source !== 'string' || !change.source.trim()) throw new Error('항목명과 확인된 값의 출처가 필요합니다.');
    if (!['set', 'clear', 'preserve'].includes(change.action)) throw new Error('입력·비우기·보존 조치를 지정하세요.');
    if (JSON.stringify(change.expected) !== JSON.stringify(target.before)) throw new Error(`현재 값이 다릅니다: ${change.target}`);
    const values = change.action === 'preserve' ? target.before : change.values;
    if (change.action === 'preserve' && change.values !== undefined && JSON.stringify(change.values) !== JSON.stringify(target.before)) throw new Error('보존 조치의 값은 현재 값과 같아야 합니다.');
    if (!Array.isArray(values) || values.length !== target.paragraphs.length || values.some(v => typeof v !== 'string')) throw new Error('기존 문단 수와 입력값 배열을 맞추세요. 문단 안 줄바꿈은 사용할 수 있습니다.');
    if (change.action === 'clear' && values.some(Boolean)) throw new Error('비우기에는 빈 문자열만 지정하세요.');
    if (change.action === 'set' && values.every(v => v === '')) throw new Error('전체 비우기는 clear로 명시하세요.');
    if (change.action !== 'preserve') for (const value of values) { checkXmlText(value); if (/[\r\t]/.test(value) || value.length > 1000000) throw new Error('입력은 LF 줄바꿈을 사용하며 탭·과도하게 긴 값은 지원하지 않습니다.'); }
    target.paragraphs.forEach((p, i) => {
      const identity = `${target.section}:${p.start}`;
      if (touched.has(identity)) throw new Error('같은 문단·셀에 중복 입력이 있습니다.'); touched.add(identity);
      if (values[i] !== target.before[i]) {
        if (!edits.has(target.section)) edits.set(target.section, []);
        edits.get(target.section).push({ start: p.start, end: p.end, value: paragraphReplacement(doc.internals.get(target.section).xml, p, values[i]) });
      }
    });
    preview.push({ target: change.target, label: change.label, source: change.source, action: change.action, before: target.before, after: values });
  }
  const replacements = new Map();
  for (const [section, list] of edits) {
    let xml = doc.internals.get(section).xml;
    const sorted = list.sort((a, b) => b.start - a.start);
    for (let i = 0; i < sorted.length; i++) {
      const e = sorted[i]; if (i && e.end > sorted[i - 1].start) throw new Error('수정 영역이 겹칩니다.');
      xml = xml.slice(0, e.start) + e.value + xml.slice(e.end);
    }
    replacements.set(section, Buffer.from(xml, 'utf8'));
  }
  const output = patchZip(doc.archive, replacements), after = loadHwpx(output);
  for (const item of preview) if (JSON.stringify(after.targets.get(item.target)?.before) !== JSON.stringify(item.after)) throw new Error(`작성 후 값 대조 실패: ${item.label}`);
  for (const entry of doc.archive.entries) {
    const wanted = replacements.get(entry.name) ?? entry.data;
    if (!wanted.equals(after.archive.read(entry.name))) throw new Error('대상 밖 ZIP 내용 변경이 있습니다.');
  }
  const expectedStructure = structuredClone(doc.structure);
  expectedStructure.sourceSha256 = after.structure.sourceSha256;
  for (const section of expectedStructure.sections) for (const p of section.paragraphs) {
    const old = doc.targets.get(p.key), updated = after.targets.get(p.key);
    if (touched.has(`${section.name}:${old.node.start}`)) p.text = updated.before[0];
  }
  if (JSON.stringify(expectedStructure) !== JSON.stringify(after.structure)) throw new Error('문단·표 구조 또는 대상 밖 글자가 바뀌었습니다.');
  const unassigned = [...doc.targets.values()].filter(t => t.writable && (t.kind === 'cell' || (!ancestorHp(t.node, 'tc') && t.kind === 'paragraph')) && !t.paragraphs.every(p => touched.has(`${t.section}:${p.start}`))).map(t => ({ target: t.key, before: t.before, action: 'unchanged-not-requested' }));
  return { output, preview, unassigned, report: { schemaVersion: 1, sourceSha256: doc.structure.sourceSha256, outputSha256: sha256(output), changedSections: [...replacements.keys()], valueCheck: 'passed', preservationCheck: 'passed', visualStatus: 'not-checked', previewStatus: 'requires-hangul-save', changes: preview, unassigned } };
}

export function verifyHwpx(bytes, request, actual) {
  const expected = prepareHwpx(bytes, request);
  if (!actual.equals(expected.output)) throw new Error('산출물이 승인된 입력으로 만든 파일과 다릅니다. 한글 재저장본은 별도 화면·내용 검증이 필요합니다.');
  return expected.report;
}
