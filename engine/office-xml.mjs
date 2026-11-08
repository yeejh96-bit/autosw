// 작성용 엄격한 XML 읽기. 기존 평문 변환기의 관대한 읽기와 분리한다.
export const HP = 'http://www.hancom.co.kr/hwpml/2011/paragraph';
const XMLNS = 'http://www.w3.org/2000/xmlns/';
const XML = 'http://www.w3.org/XML/1998/namespace';
const validChar = cp => cp === 9 || cp === 10 || cp === 13 || (cp >= 32 && cp <= 0xd7ff) || (cp >= 0xe000 && cp <= 0xfffd) || (cp >= 0x10000 && cp <= 0x10ffff);
export function checkXmlText(text) {
  for (const ch of text) if (!validChar(ch.codePointAt(0))) throw new Error('XML에 쓸 수 없는 문자입니다.');
  return text;
}
export function xmlDecode(text) {
  return checkXmlText(text.replace(/&([^;]*);|&/g, (full, body) => {
    const named = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'" };
    if (Object.hasOwn(named, body)) return named[body];
    if (/^#(?:[0-9]+|x[0-9a-fA-F]+)$/.test(body ?? '')) {
      const cp = body[1] === 'x' ? parseInt(body.slice(2), 16) : Number(body.slice(1));
      if (validChar(cp)) return String.fromCodePoint(cp);
    }
    throw new Error('알 수 없거나 잘못된 XML 문자 참조입니다.');
  }));
}
export const xmlEscape = text => checkXmlText(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/\r/g, '&#13;');
const qname = /^[A-Za-z_][\w.-]*(?::[A-Za-z_][\w.-]*)?$/;
export function parseOfficeXml(xml) {
  checkXmlText(xml);
  const nodes = [], roots = [], stack = [];
  const xmlStart = xml.charCodeAt(0) === 0xfeff ? 1 : 0;
  let pos = xmlStart, declared = false;
  const addText = (text, raw = false) => {
    const decoded = raw ? text : xmlDecode(text.replace(/\r\n?/g, '\n'));
    if (!stack.length) { if (decoded.trim()) throw new Error('XML 루트 밖 글자입니다.'); }
    else stack.at(-1).parts.push(decoded);
  };
  while (pos < xml.length) {
    if (xml[pos] !== '<') {
      let end = xml.indexOf('<', pos); if (end < 0) end = xml.length;
      const text = xml.slice(pos, end);
      if (text.includes(']]>')) throw new Error('잘못된 XML 본문입니다.');
      addText(text); pos = end; continue;
    }
    if (xml.startsWith('<!--', pos)) {
      const end = xml.indexOf('-->', pos + 4);
      if (end < 0 || xml.slice(pos + 4, end).includes('--') || xml.slice(pos + 4, end).endsWith('-')) throw new Error('잘못된 XML 주석입니다.');
      pos = end + 3; continue;
    }
    if (xml.startsWith('<?', pos)) {
      const end = xml.indexOf('?>', pos + 2);
      if (end < 0) throw new Error('잘못된 XML 선언입니다.');
      const body = xml.slice(pos + 2, end);
      if (pos !== xmlStart || declared || !/^xml\s+version\s*=\s*(['"])1\.0\1(?:\s+encoding\s*=\s*(['"])[Uu][Tt][Ff]-8\2)?(?:\s+standalone\s*=\s*(['"])(?:yes|no)\3)?\s*$/.test(body)) throw new Error('지원하지 않거나 잘못된 XML 선언·처리 지시입니다.');
      declared = true;
      pos = end + 2; continue;
    }
    if (xml.startsWith('<![CDATA[', pos)) {
      const end = xml.indexOf(']]>', pos + 9);
      if (end < 0 || !stack.length) throw new Error('잘못된 CDATA입니다.');
      addText(xml.slice(pos + 9, end).replace(/\r\n?/g, '\n'), true); pos = end + 3; continue;
    }
    if (xml.startsWith('<!', pos)) throw new Error('DTD·외부 엔티티는 지원하지 않습니다.');
    let end = pos + 1, quote = '';
    for (; end < xml.length; end++) {
      const ch = xml[end];
      if (quote) { if (ch === quote) quote = ''; }
      else if (ch === '"' || ch === "'") quote = ch;
      else if (ch === '>') break;
    }
    if (end >= xml.length) throw new Error('닫히지 않은 XML 태그입니다.');
    const tag = xml.slice(pos + 1, end);
    if (tag.startsWith('/')) {
      const closing = /^\/([A-Za-z_][\w.-]*(?::[A-Za-z_][\w.-]*)?)\s*$/.exec(tag);
      if (!closing) throw new Error('잘못된 XML 닫는 태그입니다.');
      const name = closing[1];
      const node = stack.pop();
      if (!node || node.name !== name) throw new Error('XML 태그 짝이 맞지 않습니다.');
      node.closeStart = pos; node.end = end + 1;
    } else {
      const selfClosing = tag.endsWith('/');
      const body = selfClosing ? tag.slice(0, -1) : tag;
      const match = /^([^\s]+)([\s\S]*)$/.exec(body);
      if (!match || !qname.test(match[1])) throw new Error('잘못된 XML 이름입니다.');
      const attrs = Object.create(null), namespaces = new Map(stack.at(-1)?.namespaces ?? [['xml', XML]]);
      let rest = match[2];
      while (rest.trim()) {
        const a = /^\s+([^\s=]+)\s*=\s*(['"])([\s\S]*?)\2/.exec(rest);
        if (!a || !qname.test(a[1]) || Object.hasOwn(attrs, a[1]) || a[3].includes('<')) throw new Error('잘못되거나 중복된 XML 속성입니다.');
        attrs[a[1]] = xmlDecode(a[3].replace(/\r\n?/g, '\n').replace(/[\t\n]/g, ' '));
        if (a[1] === 'xmlns' || a[1].startsWith('xmlns:')) {
          const prefix = a[1] === 'xmlns' ? '' : a[1].slice(6);
          if (prefix === 'xmlns' || (prefix === 'xml' && attrs[a[1]] !== XML) || (prefix !== 'xml' && attrs[a[1]] === XML) || attrs[a[1]] === XMLNS || (prefix && !attrs[a[1]])) throw new Error('잘못된 XML namespace입니다.');
          namespaces.set(prefix, attrs[a[1]]);
        }
        rest = rest.slice(a[0].length);
      }
      const [prefix, local] = match[1].includes(':') ? match[1].split(':') : ['', match[1]];
      if (prefix && !namespaces.get(prefix)) throw new Error('선언되지 않은 XML 접두어입니다.');
      const expanded = new Set();
      for (const name of Object.keys(attrs)) {
        const [ap, al] = name.includes(':') ? name.split(':') : ['', name];
        if (ap && ap !== 'xmlns' && !namespaces.get(ap)) throw new Error('선언되지 않은 속성 접두어입니다.');
        const key = `${ap === 'xmlns' || name === 'xmlns' ? XMLNS : ap ? namespaces.get(ap) : ''}|${al}`;
        if (expanded.has(key)) throw new Error('중복된 XML 속성입니다.'); expanded.add(key);
      }
      const node = { name: match[1], local, ns: namespaces.get(prefix) ?? '', attrs, namespaces, parent: stack.at(-1) ?? null, children: [], parts: [], start: pos, openEnd: end + 1, closeStart: selfClosing ? end + 1 : null, end: selfClosing ? end + 1 : null, selfClosing };
      if (node.parent) { node.parent.children.push(node); node.parent.parts.push(node); } else roots.push(node);
      nodes.push(node);
      if (!selfClosing) stack.push(node);
      if (stack.length > 256 || nodes.length > 500000) throw new Error('XML 구조가 너무 큽니다.');
    }
    pos = end + 1;
  }
  if (stack.length || roots.length !== 1) throw new Error('완전한 XML 문서가 아닙니다.');
  return { xml, root: roots[0], nodes };
}
export const isHp = (n, name) => n.ns === HP && n.local === name;
export const childrenHp = (n, name) => n.children.filter(c => isHp(c, name));
export function ancestorHp(n, name) { for (let p = n.parent; p; p = p.parent) if (isHp(p, name)) return p; return null; }
export function textOf(n) {
  if (isHp(n, 'lineBreak')) return '\n';
  if (isHp(n, 'tab')) return '\t';
  return n.parts.map(p => typeof p === 'string' ? p : textOf(p)).join('');
}
export const paragraphText = n => childrenHp(n, 'run').flatMap(r => childrenHp(r, 't').map(textOf)).join('');
