// =============================================
// office-zip.mjs — hwpx/docx(=zip) 를 md 로 바꾸는 공용 라이브러리
//
// 왜 있나: DOCX·HWPX PowerShell 변환기를 Node 로 옮겨
//   Windows용 PowerShell 변환기의 내부 교차 검증에도 쓴다. 두 .ps1 은 "문단 뽑는 함수"만 다르고 나머지가 같아서
//   같은 부분을 여기 한 벌만 두고, convert-docx.mjs·convert-hwpx.mjs는 얇은 진입점만 남겼다.
//
// 규칙: 새 의존성을 깔지 않는다. node: 내장 모듈만 쓴다.
//   그래서 zip 풀기도 XML 읽기도 여기서 직접 한다.
//   기존 .ps1 은 회사 윈도우 PC 가 그대로 쓴다 — 고치지 않는다. 동작만 똑같이 맞춘다.
// =============================================
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { writeAtomic } from './atomic-file.mjs';
export { parseOfficeXml, HP, isHp, childrenHp, ancestorHp, paragraphText, textOf, xmlEscape, checkXmlText } from './office-xml.mjs';

// 작성용 ZIP은 기존 변환보다 엄격하게 검사한다. 기존 readZip의 동작은 유지한다.
export function crc32(data) {
  let crc = 0xffffffff;
  for (const byte of data) { crc ^= byte; for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0); }
  return (crc ^ 0xffffffff) >>> 0;
}
export function readZipStrict(buf) {
  if (buf.length > 128 * 1024 * 1024 || buf.length < 22 || buf.readUInt32LE(0) !== 0x04034b50) throw new Error('지원하는 HWPX ZIP 형식이 아닙니다. HWP는 한글 변환이 필요합니다.');
  let eocd = -1;
  for (let i = buf.length - 22; i >= Math.max(0, buf.length - 65557); i--) {
    if (buf.readUInt32LE(i) === 0x06054b50 && i + 22 + buf.readUInt16LE(i + 20) === buf.length) { eocd = i; break; }
  }
  if (eocd < 0) throw new Error('ZIP 종료 정보가 없습니다.');
  const count = buf.readUInt16LE(eocd + 10), size = buf.readUInt32LE(eocd + 12), offset = buf.readUInt32LE(eocd + 16);
  if (buf.readUInt16LE(eocd + 4) || buf.readUInt16LE(eocd + 6) || buf.readUInt16LE(eocd + 8) !== count || count === 65535 || size === 0xffffffff || offset === 0xffffffff || offset + size !== eocd || count > 10000) throw new Error('분할 ZIP·ZIP64 또는 잘못된 ZIP입니다.');
  let p = offset, total = 0;
  const entries = [], names = new Set();
  const decode = b => new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(b);
  for (let i = 0; i < count; i++) {
    if (p + 46 > eocd || buf.readUInt32LE(p) !== 0x02014b50) throw new Error('잘못된 ZIP 중앙 정보입니다.');
    const flags = buf.readUInt16LE(p + 8), method = buf.readUInt16LE(p + 10), crc = buf.readUInt32LE(p + 16), packed = buf.readUInt32LE(p + 20), length = buf.readUInt32LE(p + 24);
    const nl = buf.readUInt16LE(p + 28), el = buf.readUInt16LE(p + 30), cl = buf.readUInt16LE(p + 32), local = buf.readUInt32LE(p + 42);
    const end = p + 46 + nl + el + cl;
    if (end > eocd || !nl || local + 30 > offset || packed === 0xffffffff || length === 0xffffffff || local === 0xffffffff || buf.readUInt16LE(p + 34)) throw new Error('ZIP 범위를 벗어났습니다.');
    const name = decode(buf.subarray(p + 46, p + 46 + nl));
    if (names.has(name) || name.includes('\\') || name.startsWith('/') || name.split('/').some(x => x === '..' || x === '.') || /[\x00-\x1f]/.test(name)) throw new Error('중복되거나 잘못된 ZIP 항목 이름입니다.');
    names.add(name);
    if (flags & ~0x80e || ![0, 8].includes(method) || (method === 0 && (flags & 6))) throw new Error('암호화 또는 지원하지 않는 ZIP 방식입니다.');
    if (buf.readUInt32LE(local) !== 0x04034b50 || buf.readUInt16LE(local + 6) !== flags || buf.readUInt16LE(local + 8) !== method) throw new Error('ZIP 로컬 정보가 다릅니다.');
    const lnl = buf.readUInt16LE(local + 26), lel = buf.readUInt16LE(local + 28), start = local + 30 + lnl + lel;
    if (start + packed > offset || decode(buf.subarray(local + 30, local + 30 + lnl)) !== name) throw new Error('ZIP 로컬 항목 범위가 다릅니다.');
    if (!(flags & 8) && (buf.readUInt32LE(local + 14) !== crc || buf.readUInt32LE(local + 18) !== packed || buf.readUInt32LE(local + 22) !== length)) throw new Error('ZIP 검사값이 다릅니다.');
    for (const extra of [buf.subarray(p + 46 + nl, p + 46 + nl + el), buf.subarray(local + 30 + lnl, start)]) {
      for (let x = 0; x < extra.length;) {
        if (x + 4 > extra.length || x + 4 + extra.readUInt16LE(x + 2) > extra.length || extra.readUInt16LE(x) === 1) throw new Error('잘못된 ZIP 확장 정보 또는 ZIP64입니다.');
        x += 4 + extra.readUInt16LE(x + 2);
      }
    }
    total += length;
    if (length > 64 * 1024 * 1024 || total > 256 * 1024 * 1024) throw new Error('ZIP 확장 크기 한도를 넘었습니다.');
    const raw = buf.subarray(start, start + packed);
    const data = method === 0 ? Buffer.from(raw) : zlib.inflateRawSync(raw, { maxOutputLength: Math.max(1, length) });
    if (data.length !== length || crc32(data) !== crc) throw new Error('ZIP 내용 검사값이 다릅니다.');
    entries.push({ name, data, flags, method, crc, packed, local, start, central: Buffer.from(buf.subarray(p, end)) });
    p = end;
  }
  if (p !== eocd) throw new Error('남는 ZIP 중앙 정보가 있습니다.');
  const ordered = [...entries].sort((a, b) => a.local - b.local);
  for (let i = 0; i < ordered.length; i++) {
    const e = ordered[i], end = ordered[i + 1]?.local ?? offset;
    if ((i === 0 && e.local !== 0) || e.start + e.packed > end) throw new Error('ZIP 항목이 겹칩니다.');
    const descriptor = end - e.start - e.packed;
    if (!(e.flags & 8) && descriptor !== 0) throw new Error('지원하지 않는 ZIP 항목 간 데이터입니다.');
    if (e.flags & 8) {
      if (![12, 16].includes(descriptor)) throw new Error('잘못된 ZIP 데이터 설명자입니다.');
      let d = e.start + e.packed;
      if (descriptor === 16) { if (buf.readUInt32LE(d) !== 0x08074b50) throw new Error('ZIP 설명자 서명이 다릅니다.'); d += 4; }
      if (buf.readUInt32LE(d) !== e.crc || buf.readUInt32LE(d + 4) !== e.packed || buf.readUInt32LE(d + 8) !== e.data.length) throw new Error('ZIP 설명자 값이 다릅니다.');
    }
    e.record = Buffer.from(buf.subarray(e.local, end));
    e.header = Buffer.from(buf.subarray(e.local, e.start));
  }
  return { entries, ordered, end: Buffer.from(buf.subarray(eocd)), read: name => entries.find(e => e.name === name)?.data ?? null };
}
export function patchZip(archive, replacements) {
  for (const name of replacements.keys()) if (!archive.entries.some(e => e.name === name)) throw new Error('없는 ZIP 항목은 추가하지 않습니다.');
  const chunks = [], records = new Map(); let offset = 0;
  for (const e of archive.ordered) {
    const central = Buffer.from(e.central); central.writeUInt32LE(offset, 42);
    let record = e.record;
    if (replacements.has(e.name)) {
      const data = replacements.get(e.name), raw = e.method === 0 ? data : zlib.deflateRawSync(data), crc = crc32(data);
      const header = Buffer.from(e.header), flags = e.flags & ~8;
      header.writeUInt16LE(flags, 6); header.writeUInt32LE(crc, 14); header.writeUInt32LE(raw.length, 18); header.writeUInt32LE(data.length, 22);
      central.writeUInt16LE(flags, 8); central.writeUInt32LE(crc, 16); central.writeUInt32LE(raw.length, 20); central.writeUInt32LE(data.length, 24);
      record = Buffer.concat([header, raw]);
    }
    chunks.push(record); records.set(e.name, central); offset += record.length;
  }
  const central = Buffer.concat(archive.entries.map(e => records.get(e.name))), end = Buffer.from(archive.end);
  end.writeUInt32LE(central.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...chunks, central, end]);
}

// 새 문서용 작은 ZIP. 순서 고정·UTF-8·무압축으로 작성하고 공용 엄격 읽기로 재검사한다.
export function createZip(entries) {
  if (!(entries instanceof Map) || !entries.size || entries.size > 10000) throw new Error('잘못된 새 ZIP 항목입니다.');
  const records = [], directory = []; let offset = 0;
  for (const [name, value] of entries) {
    const filename = Buffer.from(name, 'utf8'), data = Buffer.from(value);
    if (filename.length > 65535 || data.length > 64 * 1024 * 1024) throw new Error('ZIP 항목 크기 한도 초과');
    const crc = crc32(data), local = Buffer.alloc(30), central = Buffer.alloc(46);
    local.writeUInt32LE(0x04034b50); local.writeUInt16LE(20, 4); local.writeUInt16LE(0x800, 6);
    local.writeUInt16LE(33, 12); local.writeUInt32LE(crc, 14); local.writeUInt32LE(data.length, 18); local.writeUInt32LE(data.length, 22); local.writeUInt16LE(filename.length, 26);
    central.writeUInt32LE(0x02014b50); central.writeUInt16LE(20, 4); central.writeUInt16LE(20, 6); central.writeUInt16LE(0x800, 8);
    central.writeUInt16LE(33, 14); central.writeUInt32LE(crc, 16); central.writeUInt32LE(data.length, 20); central.writeUInt32LE(data.length, 24); central.writeUInt16LE(filename.length, 28); central.writeUInt32LE(offset, 42);
    records.push(local, filename, data); directory.push(central, filename); offset += local.length + filename.length + data.length;
    if (offset > 120 * 1024 * 1024) throw new Error('새 ZIP 전체 크기 한도 초과');
  }
  const cd = Buffer.concat(directory), end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50); end.writeUInt16LE(entries.size, 8); end.writeUInt16LE(entries.size, 10); end.writeUInt32LE(cd.length, 12); end.writeUInt32LE(offset, 16);
  const result = Buffer.concat([...records, cd, end]); readZipStrict(result); return result;
}

// ---------------------------------------------
// 1) zip 읽기 (직접 파싱)
//    끝(End of Central Directory) → 중앙 디렉터리 → 각 항목의 Local File Header 순으로 읽는다.
//    저장(method 0, 무압축)과 디플레이트(method 8) 둘 다 처리한다.
// ---------------------------------------------
const SIG_EOCD = 0x06054b50;
const SIG_CEN = 0x02014b50;
const SIG_LOC = 0x04034b50;

function findEocd(buf) {
  // zip 주석이 최대 65535 바이트라 뒤에서 그만큼만 훑으면 된다.
  const min = Math.max(0, buf.length - (0xffff + 22));
  for (let i = buf.length - 22; i >= min; i--) {
    if (buf.readUInt32LE(i) === SIG_EOCD) return i;
  }
  return -1;
}

/**
 * zip 바이트를 읽어 항목 목록을 돌려준다.
 * @returns {{names: string[], read: (name: string) => Buffer|null}}
 */
export function readZip(buf) {
  const eocd = findEocd(buf);
  if (eocd < 0) throw new Error('zip 형식이 아닙니다 (EOCD 를 못 찾음)');

  const count = buf.readUInt16LE(eocd + 10);
  const cenSize = buf.readUInt32LE(eocd + 12);
  const cenOffset = buf.readUInt32LE(eocd + 16);
  if (cenOffset === 0xffffffff || count === 0xffff || cenSize === 0xffffffff) {
    throw new Error('zip64 파일은 지원하지 않습니다');
  }

  const entries = new Map(); // 이름 -> {method, compSize, localOffset}
  const names = [];
  let p = cenOffset;
  for (let i = 0; i < count; i++) {
    if (p + 46 > buf.length || buf.readUInt32LE(p) !== SIG_CEN) {
      throw new Error('zip 중앙 디렉터리가 깨졌습니다');
    }
    const method = buf.readUInt16LE(p + 10);
    const compSize = buf.readUInt32LE(p + 20);
    const nameLen = buf.readUInt16LE(p + 28);
    const extraLen = buf.readUInt16LE(p + 30);
    const commentLen = buf.readUInt16LE(p + 32);
    const localOffset = buf.readUInt32LE(p + 42);
    // 우리가 찾는 이름(Contents/section0.xml, word/document.xml)은 전부 ASCII 라
    // 파일명 인코딩은 신경 쓰지 않는다.
    const name = buf.toString('utf8', p + 46, p + 46 + nameLen);
    if (!entries.has(name)) {
      entries.set(name, { method, compSize, localOffset });
      names.push(name);
    }
    p += 46 + nameLen + extraLen + commentLen;
  }

  function read(name) {
    const e = entries.get(name);
    if (!e) return null;
    if (buf.readUInt32LE(e.localOffset) !== SIG_LOC) {
      throw new Error(`zip 항목 머리말이 깨졌습니다: ${name}`);
    }
    const nameLen = buf.readUInt16LE(e.localOffset + 26);
    const extraLen = buf.readUInt16LE(e.localOffset + 28);
    const start = e.localOffset + 30 + nameLen + extraLen;
    const raw = buf.subarray(start, start + e.compSize);
    if (e.method === 0) return Buffer.from(raw); // 무압축
    if (e.method === 8) return zlib.inflateRawSync(raw); // 디플레이트
    throw new Error(`지원하지 않는 zip 압축 방식(${e.method}): ${name}`);
  }

  return { names, read };
}

/** zip 항목 하나를 UTF-8 글자로 읽는다. 없으면 null. */
export function readZipText(zip, name) {
  const b = zip.read(name);
  if (b === null) return null;
  let s = b.toString('utf8');
  if (s.charCodeAt(0) === 0xfeff) s = s.slice(1); // BOM 은 떼고 본다
  return s;
}

// ---------------------------------------------
// 2) 아주 작은 XML 스캐너 (의존성 없이)
//    태그를 앞에서부터 선형으로 훑으며 이벤트를 넘겨준다.
//    처리하는 것: 자체 종료 태그(<w:br/>), 엔티티, 주석, <?xml ?>, <!DOCTYPE>, CDATA.
//    속성값 안의 '>' 에 속지 않도록 따옴표 상태를 본다.
// ---------------------------------------------
const NAMED_ENTITIES = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'" };

export function decodeEntities(s) {
  if (s.indexOf('&') < 0) return s;
  return s.replace(/&(#x[0-9a-fA-F]+|#[0-9]+|[A-Za-z][A-Za-z0-9]*);/g, (whole, body) => {
    if (body[0] === '#') {
      const code = body[1] === 'x' || body[1] === 'X'
        ? parseInt(body.slice(2), 16)
        : parseInt(body.slice(1), 10);
      if (!Number.isFinite(code) || code < 0 || code > 0x10ffff) return whole;
      try { return String.fromCodePoint(code); } catch { return whole; }
    }
    const named = NAMED_ENTITIES[body];
    return named === undefined ? whole : named; // 모르는 엔티티는 그대로 둔다
  });
}

function localName(qname) {
  const i = qname.indexOf(':');
  return i < 0 ? qname : qname.slice(i + 1);
}

/**
 * XML 을 훑으며 콜백을 부른다.
 * @param {string} xml
 * @param {{open:(local:string, selfClosing:boolean, namespace:string)=>void, close:(local:string, namespace:string)=>void, text:(s:string)=>void}} on
 */
export function scanXml(xml, on) {
  const n = xml.length;
  let i = 0;
  const elements = [];
  while (i < n) {
    const lt = xml.indexOf('<', i);
    if (lt < 0) {
      if (i < n) on.text(decodeEntities(xml.slice(i)));
      break;
    }
    if (lt > i) on.text(decodeEntities(xml.slice(i, lt)));

    if (xml.startsWith('<!--', lt)) {
      const end = xml.indexOf('-->', lt + 4);
      i = end < 0 ? n : end + 3;
      continue;
    }
    if (xml.startsWith('<![CDATA[', lt)) {
      const end = xml.indexOf(']]>', lt + 9);
      const body = xml.slice(lt + 9, end < 0 ? n : end);
      on.text(body); // CDATA 안은 엔티티를 풀지 않는다
      i = end < 0 ? n : end + 3;
      continue;
    }
    if (xml.startsWith('<?', lt)) {
      const end = xml.indexOf('?>', lt + 2);
      i = end < 0 ? n : end + 2;
      continue;
    }
    if (xml.startsWith('<!', lt)) {
      // <!DOCTYPE ...> — 안쪽 [ ... ] 가 있으면 그걸 먼저 닫고 '>' 를 찾는다
      let j = lt + 2;
      const br = xml.indexOf('[', j);
      const gt0 = xml.indexOf('>', j);
      if (br >= 0 && (gt0 < 0 || br < gt0)) {
        const rb = xml.indexOf(']', br);
        j = rb < 0 ? n : rb + 1;
      }
      const end = xml.indexOf('>', j);
      i = end < 0 ? n : end + 1;
      continue;
    }
    if (xml.startsWith('</', lt)) {
      let j = lt + 2;
      while (j < n && !/[\s/>]/.test(xml[j])) j++;
      const name = xml.slice(lt + 2, j);
      const end = xml.indexOf('>', j);
      const index = elements.findLastIndex((element) => element.name === name);
      if (index >= 0) {
        on.close(localName(name), elements[index].namespace);
        elements.length = index;
      }
      i = end < 0 ? n : end + 1;
      continue;
    }

    // 여는 태그
    let j = lt + 1;
    while (j < n && !/[\s/>]/.test(xml[j])) j++;
    const name = xml.slice(lt + 1, j);
    let quote = '';
    let k = j;
    let selfClosing = false;
    for (; k < n; k++) {
      const c = xml[k];
      if (quote) {
        if (c === quote) quote = '';
        continue;
      }
      if (c === '"' || c === "'") { quote = c; continue; }
      if (c === '>') break;
    }
    if (k <= n) {
      // '>' 바로 앞(공백 무시)이 '/' 면 자체 종료 태그
      let m = k - 1;
      while (m > j && /\s/.test(xml[m])) m--;
      selfClosing = xml[m] === '/';
    }
    const namespaces = new Map(elements.at(-1)?.namespaces);
    for (const match of xml.slice(j, k).matchAll(/\s+([^\s=/>]+)\s*=\s*(["'])(.*?)\2/gs)) {
      if (match[1] === 'xmlns' || match[1].startsWith('xmlns:')) {
        namespaces.set(match[1] === 'xmlns' ? '' : match[1].slice(6), decodeEntities(match[3]));
      }
    }
    const colon = name.indexOf(':');
    const namespace = namespaces.get(colon < 0 ? '' : name.slice(0, colon)) ?? '';
    if (!selfClosing) elements.push({ name, namespace, namespaces });
    on.open(localName(name), selfClosing, namespace);
    i = k < n ? k + 1 : n;
  }
}

// ---------------------------------------------
// 3) 문단 뽑기 (hwpx·docx 공용)
//
// [주의 — 기존 .ps1 의 중복을 일부러 그대로 재현한다]
//   표 안의 문단처럼 p 가 p 안에 중첩되면, .ps1 의 XPath '//hp:p' + './/hp:t' 때문에
//   바깥 문단이 안쪽 문단의 글자를 한 번 더 포함한다. 이건 고치지 않는다 —
//   고치면 회사 윈도우 PC 의 기존 변환 결과물과 달라지기 때문이다.
//   아래의 "열려 있는 문단 전부에 글자를 더한다"가 바로 그 재현이다.
// ---------------------------------------------
/**
 * @param {string} xml
 * @param {{namespace:string, paraName?:string, textName?:string, literals?:Record<string,string>}} opt
 * @returns {string[]} 문단 글자들 (문서에 나오는 순서)
 */
export function extractParagraphs(xml, opt) {
  const paraName = opt.paraName ?? 'p';
  const textName = opt.textName ?? 't';
  const literals = opt.literals ?? {};

  const paras = [];      // 결과 (여는 태그 순서 = 문서 순서)
  const openParas = [];  // 지금 열려 있는 문단들의 인덱스
  const stack = [];      // 여는 태그 이름 스택 (닫을 때 짝 맞추기)
  let textDepth = 0;     // 글자 요소(t) 안인가
  let textBuf = '';

  const addToOpen = (s) => {
    if (!s) return;
    for (const idx of openParas) paras[idx] += s;
  };

  scanXml(xml, {
    open(local, selfClosing, namespace) {
      if (!selfClosing) stack.push({ local, namespace });
      if (textDepth > 0) {
        // t 안의 자식 요소는 무시하고 글자만 모은다 (InnerText 와 같다)
        if (!selfClosing) textDepth++;
        return;
      }
      if (namespace !== opt.namespace) return;
      if (local === paraName) {
        paras.push('');
        if (!selfClosing) openParas.push(paras.length - 1);
        return;
      }
      if (local === textName) {
        textBuf = '';
        if (selfClosing) addToOpen('');
        else textDepth = 1;
        return;
      }
      if (Object.prototype.hasOwnProperty.call(literals, local)) {
        addToOpen(literals[local]);
      }
    },
    close(local, namespace) {
      // 짝이 안 맞는 닫는 태그는 무시한다
      let found = -1;
      for (let i = stack.length - 1; i >= 0; i--) {
        if (stack[i].local === local && stack[i].namespace === namespace) { found = i; break; }
      }
      if (found < 0) return;
      const popped = stack.length - found;
      stack.length = found;

      if (textDepth > 0) {
        textDepth -= popped;
        if (textDepth <= 0) {
          textDepth = 0;
          addToOpen(textBuf);
          textBuf = '';
        }
        return;
      }
      if (namespace === opt.namespace && local === paraName && openParas.length > 0) openParas.pop();
    },
    text(s) {
      if (textDepth > 0) textBuf += s;
    },
  });

  return paras;
}

// ---------------------------------------------
// 4) 변환 실행부 (.ps1 의 본문에 해당)
// ---------------------------------------------

// .NET String.Trim()/IsNullOrWhiteSpace 와 같은 공백 집합을 쓴다.
const WS = '[\\t\\n\\v\\f\\r \\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000]';
const RE_TRIM = new RegExp(`^${WS}+|${WS}+$`, 'g');
const RE_ALL_WS = new RegExp(`^${WS}*$`);

function trimNet(s) { return s.replace(RE_TRIM, ''); }
function isNullOrWhiteSpace(s) { return s === null || s === undefined || RE_ALL_WS.test(s); }

/** 지정한 폴더 아래에서 확장자가 맞는 파일을 모은다 (대소문자 무시). */
function listFiles(dir, ext, out = []) {
  const items = fs.readdirSync(dir, { withFileTypes: true })
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  const dirs = [];
  for (const it of items) {
    const full = path.join(dir, it.name);
    if (it.isDirectory()) dirs.push(full);
    else if (it.isFile() && it.name.toLowerCase().endsWith(ext)) out.push(full);
  }
  for (const d of dirs) listFiles(d, ext, out); // 현재 폴더 파일 먼저, 그다음 하위 폴더
  return out;
}

function today() {
  const d = new Date();
  const pad = (v) => String(v).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function saveOfficeMarkdown(outPath, content) {
  if (fs.existsSync(outPath)) {
    const comparable = text => text.replace(/^\uFEFF/, '').replace(/\r\n/g, '\n')
      .replace(/^(---\n출처: [^\n]*\n변환일: )\d{4}-\d{2}-\d{2}(\n---(?:\n|$))/, '$1DATE$2');
    if (!fs.statSync(outPath).isFile() || comparable(fs.readFileSync(outPath, 'utf8')) !== comparable(content)) {
      throw new Error('기존 결과가 원본과 다릅니다. 불완전한 변환 또는 수정 여부를 확인하세요. 기존 파일은 보존했습니다.');
    }
    return false;
  }
  writeAtomic(outPath, content, { mode: 'new' });
  return true;
}

/**
 * @param {{ext:string, kind:string, getParagraphs:(zip)=>string[]}} opt
 *   ext            '.hwpx' 같은 소문자 확장자
 *   kind           안내 문구에 쓰는 이름 ('hwpx')
 *   getParagraphs  readZip() 결과를 받아 문단 글자 배열을 돌려주는 함수
 */
export function runConverter(opt) {
  let args = process.argv.slice(2);
  const usage = '사용법: --input "원본 파일 또는 폴더" --output "결과 폴더"' + (opt.getStructure ? ' [--structure-only]' : '');
  const structureOnly = args.includes('--structure-only');
  if (structureOnly) {
    if (!opt.getStructure || args.filter(a => a === '--structure-only').length !== 1) throw new Error(usage);
    args = args.filter(a => a !== '--structure-only');
  }
  if (args.length === 1 && args[0] === '--help') {
    console.log(usage);
    return;
  }
  const values = {};
  for (let i = 0; i < args.length; i += 2) {
    const flag = args[i];
    if (!['--input', '--output'].includes(flag) || !args[i + 1]?.trim()
        || args[i + 1].startsWith('--') || flag in values) throw new Error(usage);
    values[flag] = args[i + 1];
  }
  if (!values['--input'] || !values['--output']) throw new Error(usage);
  const input = path.resolve(values['--input']);
  const outDir = path.resolve(values['--output']);
  const stat = fs.statSync(input);
  if (!stat.isDirectory() && (!stat.isFile() || path.extname(input).toLowerCase() !== opt.ext)) {
    throw new Error(`입력 파일은 ${opt.ext} 형식이어야 합니다.`);
  }
  const inDir = stat.isDirectory() ? input : path.dirname(input);
  const relativeOut = path.relative(input, outDir);
  if (stat.isDirectory() && (relativeOut === ''
      || (!path.isAbsolute(relativeOut) && relativeOut !== '..' && !relativeOut.startsWith('..' + path.sep)))) {
    throw new Error('결과 폴더는 입력 폴더 밖으로 지정하세요.');
  }
  const files = stat.isDirectory() ? listFiles(input, opt.ext) : [input];
  fs.mkdirSync(outDir, { recursive: true });
  if (files.length === 0) {
    console.log(`지정한 입력 폴더에 변환할 ${opt.ext} 파일이 없습니다.`);
    return;
  }

  console.log(`총 ${files.length}개 파일을 변환합니다...`);
  const day = today();
  let ok = 0, fail = 0, skip = 0, n = 0;

  for (const full of files) {
    n++;
    const rel = path.relative(inDir, full);
    const outPath = path.join(outDir, rel.replace(/\.[^./\\]*$/, '') + (structureOnly ? '.structure.json' : '.md'));
    fs.mkdirSync(path.dirname(outPath), { recursive: true });
    try {
      if (structureOnly) {
        const content = JSON.stringify(opt.getStructure(fs.readFileSync(full)), null, 2) + '\n';
        if (saveOfficeMarkdown(outPath, content)) { ok++; console.log(`[${n}/${files.length}] 구조 완료: ${rel}`); }
        else { skip++; console.log(`[${n}/${files.length}] 구조 재사용: ${rel}`); }
        continue;
      }
      const zip = readZip(fs.readFileSync(full));
      const paras = opt.getParagraphs(zip);
      let md = '';
      md += '---\n';
      md += '출처: ' + path.basename(full) + '\n';
      md += '변환일: ' + day + '\n';
      md += '---\n';
      md += '\n';
      let blank = true;
      for (const t of paras) {
        if (isNullOrWhiteSpace(t)) {
          if (!blank) { md += '\n'; blank = true; }
        } else {
          md += trimNet(t) + '\n\n';
          blank = true;
        }
      }
      // UTF-8, BOM 없이. 줄바꿈은 LF (저장소가 .gitattributes 로 LF 를 강제한다).
      if (saveOfficeMarkdown(outPath, md)) {
        ok++;
        console.log(`[${n}/${files.length}] 완료: ${rel}`);
      } else {
        skip++;
        console.log(`[${n}/${files.length}] 건너뜀(원본 대조 완료): ${rel}`);
      }
    } catch (error) {
      fail++;
      console.log(`[${n}/${files.length}] 실패: ${rel} — ${error.message}`);
    }
  }

  console.log('');
  console.log(`끝났습니다. 성공 ${ok}, 실패 ${fail}, 건너뜀 ${skip}`);
  console.log(`결과 위치: ${outDir}`);
  if (fail > 0) process.exitCode = 1;
}
