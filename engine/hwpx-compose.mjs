// 새 HWPX 작성. 사용자 양식/한글 설치/화면 조작에 의존하지 않는다.
import fs from 'node:fs';
import { createZip, xmlEscape, checkXmlText } from './office-zip.mjs';
import { loadHwpx } from './hwpx-document.mjs';
const defaults = JSON.parse(fs.readFileSync(new URL('./hwpx-defaults.json', import.meta.url), 'utf8'));
export const XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';
export const NS = defaults.namespace;
const mm = value => Math.round(value * 7200 / 25.4);
const own = (o, keys, label) => { if (!o || typeof o !== 'object' || Array.isArray(o) || Object.keys(o).some(k => !keys.includes(k))) throw new Error(`${label}: 알 수 없는 필드·형식`); };
const number = (n, min, max, label) => { if (typeof n !== 'number' || !Number.isFinite(n) || n < min || n > max) throw new Error(`${label}: 허용 범위 ${min}~${max}`); return n; };
function text(s) { if (typeof s !== 'string' || s.length > 50000 || /[\t\r]/.test(s)) throw new Error('본문은 탭·CR 없는 문자열, 항목당 50,000자 이하로 지정하세요.'); checkXmlText(s); return s; }
export function normalizeDocument(input) {
  own(input, ['schemaVersion', 'title', 'page', 'style', 'blocks'], '문서');
  if (input.schemaVersion !== 1) throw new Error('schemaVersion은 1이어야 합니다.');
  const p = input.page ?? {}, s = input.style ?? {}; own(p, ['widthMm', 'heightMm', 'marginsMm'], '쪽'); own(s, ['font', 'fontSize', 'lineSpacing'], '기본 서식');
  const m = p.marginsMm ?? {}; own(m, ['left', 'right', 'top', 'bottom'], '여백');
  const page = { widthMm: number(p.widthMm ?? 210, 100, 420, '쪽 너비'), heightMm: number(p.heightMm ?? 297, 100, 594, '쪽 높이'), marginsMm: Object.fromEntries(['left', 'right', 'top', 'bottom'].map(k => [k, number(m[k] ?? 20, 5, 80, '여백')])) };
  if (page.widthMm - page.marginsMm.left - page.marginsMm.right < 40 || page.heightMm - page.marginsMm.top - page.marginsMm.bottom < 40) throw new Error('본문 영역이 너무 작습니다.');
  const style = { font: text(s.font ?? '굴림'), fontSize: number(s.fontSize ?? 11, 8, 32, '글자 크기'), lineSpacing: number(s.lineSpacing ?? 160, 100, 250, '줄 간격') };
  if (!style.font || style.font.length > 80 || /[\n;{}<>]/.test(style.font)) throw new Error('잘못된 글꼴 이름');
  if (!Array.isArray(input.blocks) || !input.blocks.length || input.blocks.length > 2000) throw new Error('본문 블록은 1~2,000개여야 합니다.');
  let characters = 0;
  const blocks = input.blocks.map((b, i) => {
    own(b, ['type', 'text', 'level', 'align', 'items', 'rows', 'columnWidths', 'header'], `블록 ${i + 1}`);
    const allowed = { paragraph: ['type','text','align'], heading: ['type','text','level','align'], list: ['type','items'], table: ['type','rows','columnWidths','header'], pageBreak: ['type'] }[b.type];
    if (!allowed || Object.keys(b).some(k => !allowed.includes(k))) throw new Error(`블록 ${i + 1}: 미지원 종류·속성`);
    const take = v => { const t = text(v); characters += t.length; return t; };
    if (b.type === 'pageBreak') { if (!i || i === input.blocks.length - 1 || input.blocks[i - 1].type === 'pageBreak') throw new Error('처음·마지막·연속 쪽 나눔은 허용하지 않습니다.'); return { type: b.type }; }
    if (b.type === 'list') { if (!Array.isArray(b.items) || !b.items.length || b.items.length > 500) throw new Error('목록 항목 수 오류'); return { type: b.type, items: b.items.map(take) }; }
    if (b.type === 'table') {
      if (!Array.isArray(b.rows) || !b.rows.length || b.rows.length > 500 || !Array.isArray(b.rows[0])) throw new Error('표 행 수 오류');
      const cols = b.rows[0].length;
      if (!cols || cols > 12 || b.rows.some(r => !Array.isArray(r) || r.length !== cols)) throw new Error('표는 1~12열의 직사각형이어야 합니다.');
      const widths = b.columnWidths ?? Array(cols).fill(1);
      if (!Array.isArray(widths) || widths.length !== cols) throw new Error('열 너비 개수 오류');
      widths.forEach(w => number(w, .1, 100, '열 너비 비율'));
      if (b.header !== undefined && typeof b.header !== 'boolean') throw new Error('header는 true/false');
      return { type: b.type, rows: b.rows.map(r => r.map(take)), columnWidths: [...widths], header: b.header ?? true };
    }
    const align = b.align ?? 'left'; if (!['left', 'center', 'right'].includes(align)) throw new Error('정렬은 left/center/right');
    const level = b.type === 'heading' ? (b.level ?? 1) : undefined;
    if (level !== undefined && ![1,2,3].includes(level)) throw new Error('제목 수준은 1~3');
    return { type: b.type, text: take(b.text), align, ...(level === undefined ? {} : { level }) };
  });
  if (characters > 500000) throw new Error('본문 500,000자 한도 초과');
  return { schemaVersion: 1, title: text(input.title ?? ''), page, style, blocks };
}

// 제한된 Markdown: 제목, 일반 문단/LF, 한 단계 글머리표, 파이프 표, --- 쪽 나눔.
// 지원하지 않는 강조/링크/그림/HTML 등은 추측하거나 조용히 버리지 않는다.
export function documentFromMarkdown(markdown) {
  if (typeof markdown !== 'string' || markdown.length > 600000) throw new Error('Markdown 크기 초과');
  const lines = markdown.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n').split('\n'), blocks = [];
  const cell = line => line.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map(x => x.trim());
  for (let i = 0; i < lines.length;) {
    const line = lines[i]; if (!line.trim()) { i++; continue; }
    if (/^(?:---|\*\*\*)\s*$/.test(line)) { blocks.push({ type: 'pageBreak' }); i++; continue; }
    const heading = /^(#{1,3})\s+(.+)$/.exec(line);
    if (heading) { blocks.push({ type: 'heading', level: heading[1].length, text: heading[2] }); i++; continue; }
    if (/^[-+]\s/.test(line)) { const items = []; while (i < lines.length && /^[-+]\s/.test(lines[i])) items.push(lines[i++].replace(/^[-+]\s+/, '')); blocks.push({ type: 'list', items }); continue; }
    if (line.trim().startsWith('|') && i + 1 < lines.length && cell(lines[i + 1]).every(x => /^-{3,}$/.test(x))) {
      const rows = [cell(line)]; if (cell(lines[i + 1]).length !== rows[0].length) throw new Error('표 구분행의 열 수가 다릅니다.'); i += 2;
      while (i < lines.length && lines[i].trim().startsWith('|')) rows.push(cell(lines[i++]));
      blocks.push({ type: 'table', rows }); continue;
    }
    const paragraph = [];
    while (i < lines.length && lines[i].trim() && !/^(?:#{1,3}\s|[-+]\s|---\s*$|\*\*\*\s*$|\|)/.test(lines[i])) paragraph.push(lines[i++]);
    if (!paragraph.length) throw new Error(`Markdown ${i + 1}행: 지원하지 않는 구문`);
    blocks.push({ type: 'paragraph', text: paragraph.join('\n') });
  }
  for (const b of blocks) for (const t of b.text !== undefined ? [b.text] : b.items ?? b.rows?.flat() ?? []) {
    if (/\*|_(?:[^_\n]+)_|__|`|!\[|\]\(|<\/?[A-Za-z!]|^\s+(?:[-+*]|\d+\.)\s|^#{4,}\s|^>\s|\\\|/m.test(t)) throw new Error('강조·코드·링크·그림·HTML·중첩 목록 등은 현재 Markdown 지원 범위가 아닙니다. 명시적 JSON을 사용하세요.');
  }
  return normalizeDocument({ schemaVersion: 1, title: blocks.find(b => b.type === 'heading')?.text ?? '', blocks });
}

const border = (id, line, fill) => `<hh:borderFill id="${id}" threeD="0" shadow="0" centerLine="NONE" breakCellSeparateLine="0"><hh:slash type="NONE" Crooked="0" isCounter="0"/><hh:backSlash type="NONE" Crooked="0" isCounter="0"/>${['left','right','top','bottom'].map(d=>`<hh:${d}Border type="${line}" width="0.12 mm" color="#333333"/>`).join('')}<hh:diagonal type="SOLID" width="0.1 mm" color="#000000"/>${fill ? `<hc:fillBrush><hc:winBrush faceColor="${fill}" hatchColor="#999999" alpha="0"/></hc:fillBrush>` : ''}</hh:borderFill>`;
function header(model) {
  const fonts = ['HANGUL','LATIN','HANJA','JAPANESE','OTHER','SYMBOL','USER'].map(lang=>`<hh:fontface lang="${lang}" fontCnt="1"><hh:font id="0" face="${xmlEscape(model.style.font)}" type="TTF" isEmbedded="0"><hh:typeInfo familyType="FCAT_GOTHIC" weight="6" proportion="4" contrast="0" strokeVariation="1" armStyle="1" letterform="1" midline="1" xHeight="1"/></hh:font></hh:fontface>`).join('');
  const sizes = [model.style.fontSize,20,16,13,model.style.fontSize];
  const chars = sizes.map((size,i)=>defaults.charPr.replace('id="0"',`id="${i}"`).replace('height="1000"',`height="${Math.round(size*100)}"`).replace('<hh:underline',`${i ? '<hh:bold/>' : ''}<hh:underline`)).join('');
  const paras = ['LEFT','CENTER','RIGHT'].map((align,i)=>defaults.paraPr.replace('id="0"',`id="${i}"`).replace('horizontal="JUSTIFY"',`horizontal="${align}"`).replaceAll('value="160"',`value="${model.style.lineSpacing}"`).replace('snapToGrid="1"','snapToGrid="0"')).join('');
  return XML+`<hh:head ${NS} version="1.4" secCnt="1"><hh:beginNum page="1" footnote="1" endnote="1" pic="1" tbl="1" equation="1"/><hh:refList><hh:fontfaces itemCnt="7">${fonts}</hh:fontfaces><hh:borderFills itemCnt="4">${border(1,'NONE')}${border(2,'NONE')}${border(3,'SOLID')}${border(4,'SOLID','#EDEDED')}</hh:borderFills><hh:charProperties itemCnt="5">${chars}</hh:charProperties><hh:tabProperties itemCnt="1"><hh:tabPr id="0" autoTabLeft="0" autoTabRight="0"/></hh:tabProperties>${defaults.numberings}<hh:paraProperties itemCnt="3">${paras}</hh:paraProperties><hh:styles itemCnt="1"><hh:style id="0" type="PARA" name="바탕글" engName="Normal" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0" langID="1042" lockForm="0"/></hh:styles></hh:refList><hh:compatibleDocument targetProgram="HWP201X"><hh:layoutCompatibility/></hh:compatibleDocument><hh:docOption><hh:linkinfo path="" pageInherit="0" footnoteInherit="0"/></hh:docOption></hh:head>`;
}
export function composeHwpx(input) {
  const model=normalizeDocument(input), page=model.page, margin=page.marginsMm;
  const width=mm(page.widthMm-margin.left-margin.right); let id=0, first=true, pageBreak=false;
  let sectionProperty=defaults.secPr.replace(/<hp:pagePr[\s\S]*?<\/hp:pagePr>/, `<hp:pagePr landscape="WIDELY" width="${mm(page.widthMm)}" height="${mm(page.heightMm)}" gutterType="LEFT_ONLY"><hp:margin header="0" footer="0" gutter="0" left="${mm(margin.left)}" right="${mm(margin.right)}" top="${mm(margin.top)}" bottom="${mm(margin.bottom)}"/></hp:pagePr>`);
  sectionProperty += '<hp:ctrl><hp:colPr id="" type="NEWSPAPER" layout="LEFT" colCount="1" sameSz="1" sameGap="0"/></hp:ctrl>';
  const para=(value,char=0,align='left',control='',cell=false)=>{
    const leading=!cell&&first?sectionProperty:''; if(!cell)first=false;
    const pb=!cell&&pageBreak; if(!cell)pageBreak=false;
    return `<hp:p id="${id++}" paraPrIDRef="${['left','center','right'].indexOf(align)}" styleIDRef="0" pageBreak="${pb?1:0}" columnBreak="0" merged="0"><hp:run charPrIDRef="${char}">${leading}${control}<hp:t>${xmlEscape(value).replaceAll('\n','<hp:lineBreak/>')}</hp:t></hp:run></hp:p>`;
  };
  const paragraphs=[];
  for(const b of model.blocks){
    if(b.type==='pageBreak'){pageBreak=true;continue;}
    if(b.type==='list'){for(const t of b.items)paragraphs.push(para('• '+t));continue;}
    if(b.type!=='table'){paragraphs.push(para(b.text,b.type==='heading'?b.level:0,b.align));continue;}
    const sum=b.columnWidths.reduce((a,v)=>a+v,0);let remaining=width;
    const widths=b.columnWidths.map((w,i)=>{const n=i===b.columnWidths.length-1?remaining:Math.round(width*w/sum);remaining-=n;return n;});
    if(widths.some(w=>w<1500))throw new Error('표 열 너비가 너무 좁습니다. 열 수·비율·쪽 너비를 조정하세요.');
    const rowHeight=Math.ceil(model.style.fontSize*100*model.style.lineSpacing/100)+800;
    const rows=b.rows.map((row,r)=>'<hp:tr>'+row.map((t,c)=>`<hp:tc name="" header="${r===0&&b.header?1:0}" hasMargin="1" protect="0" editable="0" dirty="0" borderFillIDRef="${r===0&&b.header?4:3}"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0">${para(t,r===0&&b.header?4:0,'left','',true)}</hp:subList><hp:cellAddr colAddr="${c}" rowAddr="${r}"/><hp:cellSpan colSpan="1" rowSpan="1"/><hp:cellSz width="${widths[c]}" height="${rowHeight}"/><hp:cellMargin left="400" right="400" top="400" bottom="400"/></hp:tc>`).join('')+'</hp:tr>').join('');
    const table=`<hp:tbl id="${100000+id}" zOrder="0" numberingType="TABLE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" pageBreak="CELL" repeatHeader="${b.header?1:0}" rowCnt="${b.rows.length}" colCnt="${widths.length}" cellSpacing="0" borderFillIDRef="3" noAdjust="0"><hp:sz width="${width}" widthRelTo="ABSOLUTE" height="${rowHeight*b.rows.length}" heightRelTo="ABSOLUTE" protect="0"/><hp:pos treatAsChar="0" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="COLUMN" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/><hp:outMargin left="0" right="0" top="0" bottom="600"/><hp:inMargin left="400" right="400" top="400" bottom="400"/>${rows}</hp:tbl>`;
    paragraphs.push(para('',0,'left',table));
  }
  const section=XML+`<hs:sec ${NS}>${paragraphs.join('')}</hs:sec>`;
  const entries=new Map([
    ['mimetype','application/hwp+zip'],
    ['version.xml',XML+'<hv:HCFVersion xmlns:hv="http://www.hancom.co.kr/hwpml/2011/version" targetApplication="WORDPROCESSOR" major="5" minor="1" micro="0" buildNumber="1" os="1" xmlVersion="1.4" application="autosw" appVersion="0.1.0"/>'],
    ['Contents/header.xml',header(model)],['Contents/section0.xml',section],
    ['Preview/PrvText.txt',model.blocks.flatMap(b=>b.text!==undefined?[b.text]:b.items??b.rows?.map(r=>r.join('\t'))??[]).join('\r\n')],
    ['settings.xml',XML+'<ha:HWPApplicationSetting xmlns:ha="http://www.hancom.co.kr/hwpml/2011/app"><ha:CaretPosition listIDRef="0" paraIDRef="0" pos="0"/></ha:HWPApplicationSetting>'],
    ['META-INF/container.xml',XML+'<ocf:container xmlns:ocf="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><ocf:rootfiles><ocf:rootfile full-path="Contents/content.hpf" media-type="application/hwpml-package+xml"/></ocf:rootfiles></ocf:container>'],
    ['META-INF/manifest.xml',XML+'<odf:manifest xmlns:odf="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>'],
    ['Contents/content.hpf',XML+`<opf:package ${NS} version="" unique-identifier="" id=""><opf:metadata><opf:title>${xmlEscape(model.title)}</opf:title><opf:language>ko</opf:language><opf:meta name="creator" content="text">autosw</opf:meta></opf:metadata><opf:manifest><opf:item id="header" href="Contents/header.xml" media-type="application/xml"/><opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/><opf:item id="settings" href="settings.xml" media-type="application/xml"/></opf:manifest><opf:spine><opf:itemref idref="header" linear="no"/><opf:itemref idref="section0" linear="yes"/></opf:spine></opf:package>`]
  ]);
  const bytes=createZip(entries);loadHwpx(bytes);return {bytes,model};
}
