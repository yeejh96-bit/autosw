// 실제 HWPX를 다시 읽어 쪽별 SVG로 조판한다. 한글 조판의 정확 복제가 아닌 자체 미리보기다.
import { parseOfficeXml, paragraphText, xmlEscape } from './office-zip.mjs';
import { loadHwpx, sha256 } from './hwpx-document.mjs';
import { launchBrowser } from './render-pdf.mjs';
const child=(n,local)=>n?.children.find(x=>x.local===local);
const descendants=(n,local)=>n.children.flatMap(c=>[...(c.local===local?[c]:[]),...descendants(c,local)]);
const val=(n,k,otherwise=0)=>n?.attrs[k]===undefined?otherwise:Number(n.attrs[k]);
const unit=n=>n/100; // HWPUNIT -> pt
function reject(reason){throw new Error(`자체 렌더 미지원: ${reason}. 실제 한글에서 확인하세요.`);}
function finite(n,min,max,label){if(!Number.isFinite(n)||n<min||n>max)reject(`잘못된 ${label} 수치`);return n;}
export function readRenderModel(bytes){
  const doc=loadHwpx(bytes), z=doc.archive;
  if(doc.structure.sections.length!==1)reject('여러 구역');
  const h=parseOfficeXml(z.read('Contents/header.xml').toString('utf8'));
  const section=parseOfficeXml(z.read(doc.structure.sections[0].name).toString('utf8'));
  let tableCount=0;
  const sec=section.nodes.find(n=>n.local==='secPr'), page=child(sec,'pagePr'), m=child(page,'margin');
  if(!sec||!page||!m)reject('쪽 설정 없음');
  if(sec.attrs.textDirection!=='HORIZONTAL'||val(sec,'masterPageCnt')||val(m,'gutter'))reject('세로쓰기·바탕쪽·제본 여백');
  const cols=section.nodes.filter(n=>n.local==='colPr');if(cols.some(n=>val(n,'colCount',1)!==1))reject('다단');
  const pageModel={width:unit(val(page,'width')),height:unit(val(page,'height')),left:unit(val(m,'left')),right:unit(val(m,'right')),top:unit(val(m,'top')+val(m,'header')),bottom:unit(val(m,'bottom')+val(m,'footer'))};
  for(const [k,v]of Object.entries(pageModel))finite(v,0,2000,k);
  if(pageModel.width-pageModel.left-pageModel.right<40||pageModel.height-pageModel.top-pageModel.bottom<40)reject('잘못된 쪽 크기');
  const resolve=(local,id)=>{const n=h.nodes.find(x=>x.local===local&&x.attrs.id===id);if(!n)reject(`${local} 서식 참조`);return n;};
  function charStyle(id){
    const c=resolve('charPr',id), f=child(c,'fontRef'), fontIds=Object.values(f?.attrs??{});
    if(!fontIds.length)reject('글꼴 참조 없음');
    const fonts=h.nodes.filter(n=>n.local==='fontface').map(n=>child(n,'font')&&n.children.find(x=>x.local==='font'&&x.attrs.id===f.attrs[{HANGUL:'hangul',LATIN:'latin',HANJA:'hanja',JAPANESE:'japanese',OTHER:'other',SYMBOL:'symbol',USER:'user'}[n.attrs.lang]])?.attrs.face);
    if(fonts.some(x=>!x)||new Set(fonts).size!==1)reject('언어별 다른 글꼴');
    for(const [name,expected]of [['ratio','100'],['spacing','0'],['relSz','100'],['offset','0']]){const n=child(c,name);if(n&&Object.values(n.attrs).some(x=>x!==expected))reject('장평·자간·글자 위치 조정');}
    if(child(c,'italic')||child(c,'supscript')||child(c,'subscript')||child(c,'emboss')||child(c,'engrave')||child(c,'outline')?.attrs.type!=='NONE'||child(c,'shadow')?.attrs.type!=='NONE')reject('기울임·첨자·장식 글자');
    if(child(c,'underline')?.attrs.type!=='NONE'||child(c,'strikeout')?.attrs.shape!=='NONE')reject('밑줄·취소선');
    const color=c.attrs.textColor??'#000000';if(!/^#[0-9a-f]{6}$/i.test(color))reject('글자 색');
    if(c.attrs.shadeColor&&c.attrs.shadeColor!=='none')reject('글자 음영');
    return {font:fonts[0],size:finite(unit(val(c,'height')),1,200,'글자 크기'),bold:!!child(c,'bold'),color};
  }
  function paragraph(n){
    if(n.children.some(c=>!['run','linesegarray'].includes(c.local)))reject('문단의 미지원 요소');
    const pp=resolve('paraPr',n.attrs.paraPrIDRef), align=child(pp,'align')?.attrs.horizontal??'LEFT';
    if(!['LEFT','CENTER','RIGHT','JUSTIFY'].includes(align))reject('문단 정렬');
    const heading=child(pp,'heading');if(heading&&heading.attrs.type!=='NONE')reject('자동 번호 문단');
    if(val(pp,'condense'))reject('문단 압축');
    const margin=descendants(pp,'margin')[0], spacing=descendants(pp,'lineSpacing')[0];
    if(!spacing||spacing.attrs.type!=='PERCENT')reject('고정 줄 간격');
    const ma=k=>finite(unit(val(child(margin??{children:[]},k),'value')),0,2000,'문단 여백');
    if(ma('intent'))reject('첫 줄 들여쓰기');
    const styles=[],tables=[];
    for(const run of n.children.filter(c=>c.local==='run')){
      for(const el of run.children){
        if(el.local==='t'){if(descendants(el,'tab').length||el.children.some(x=>x.local!=='lineBreak'))reject('탭·본문 제어 문자');styles.push(charStyle(run.attrs.charPrIDRef));}
        else if(el.local==='tbl')tables.push(table(el));
        else if(el.local==='ctrl'){if(el.children.some(x=>x.local!=='colPr'))reject('머리말·꼬리말·쪽번호·각주 등 제어 요소');}
        else if(el.local!=='secPr')reject(`문단 ${el.local} 요소`);
      }
    }
    if(styles.some(s=>JSON.stringify(s)!==JSON.stringify(styles[0])))reject('한 문단 안의 혼합 글자 서식');
    const content=paragraphText(n);if(tables.length){if(tables.length!==1||content.trim())reject('표와 글이 섞인 문단');return {...tables[0],pageBreak:n.attrs.pageBreak==='1'};}
    if(val(n,'columnBreak'))reject('단 나눔');
    const bs=child(pp,'breakSetting');
    if(bs?.attrs.keepWithNext==='1'||bs?.attrs.keepLines==='1'||bs?.attrs.widowOrphan==='1')reject('문단 묶음·외톨이줄 보호');
    const pb=child(pp,'border');if(pb){const bf=resolve('borderFill',pb.attrs.borderFillIDRef);if(bf.children.some(x=>/Border$/.test(x.local)&&x.attrs.type!=='NONE'&&x.local!=='diagonal')||descendants(bf,'winBrush').some(x=>x.attrs.faceColor!=='none'))reject('문단 테두리·음영');}
    return {type:'paragraph',text:content,style:styles[0]??charStyle(n.children.find(x=>x.local==='run')?.attrs.charPrIDRef??'0'),align:align.toLowerCase(),lineSpacing:finite(val(spacing,'value',160)/100,1,5,'줄 간격'),left:ma('left'),right:ma('right'),before:ma('prev'),after:ma('next'),pageBreak:n.attrs.pageBreak==='1'||bs?.attrs.pageBreakBefore==='1'};
  }
  function table(n){
    const tableNumber=++tableCount;
    if(n.children.some(x=>!['sz','pos','outMargin','inMargin','tr'].includes(x.local)))reject('표 캡션 등 미지원 표 요소');
    if(descendants(n,'tbl').length)reject('중첩표');
    const pos=child(n,'pos');if(!pos||pos.attrs.vertRelTo!=='PARA'||pos.attrs.horzRelTo!=='COLUMN'||pos.attrs.horzAlign!=='LEFT'||pos.attrs.vertAlign!=='TOP'||pos.attrs.treatAsChar!=='0'||pos.attrs.flowWithText!=='1'||val(pos,'vertOffset')||val(pos,'horzOffset')||pos.attrs.allowOverlap==='1'||n.attrs.textWrap!=='TOP_AND_BOTTOM')reject('떠 있는 표의 위치·글 감싸기');
    if(n.attrs.pageBreak==='NONE')reject('쪽 나눔 금지 표');
    const rows=val(n,'rowCnt'),cols=val(n,'colCnt');if(!Number.isInteger(rows)||!Number.isInteger(cols)||rows<1||rows>1000||cols<1||cols>32)reject('표 크기');
    if(val(n,'cellSpacing')!==0)reject('셀 간격');
    const cells=[], widths=Array(cols).fill(null);let row=0;
    for(const tr of n.children.filter(x=>x.local==='tr')){if(tr.children.some(x=>x.local!=='tc'))reject('미지원 표 행 요소');for(const tc of tr.children.filter(x=>x.local==='tc')){
      if(tc.children.some(x=>!['subList','cellAddr','cellSpan','cellSz','cellMargin'].includes(x.local)))reject('미지원 표 셀 요소');
      const addr=child(tc,'cellAddr'), span=child(tc,'cellSpan'), sz=child(tc,'cellSz'), sub=child(tc,'subList');
      const r=val(addr,'rowAddr'),c=val(addr,'colAddr'),rs=val(span,'rowSpan',1),cs=val(span,'colSpan',1);
      if(![r,c,rs,cs].every(Number.isInteger)||r<0||c<0||r!==row||r+rs>rows||c+cs>cols||rs<1||cs<1)reject('잘못된 병합 셀');
      if(rs!==1||cs!==1)reject('병합 표 — 첫 버전은 직사각형 셀만 렌더');
      if(!sub||sub.attrs.textDirection!=='HORIZONTAL'||!['TOP','CENTER','BOTTOM'].includes(sub.attrs.vertAlign))reject('셀 글 방향·정렬');
      const lineWrap=sub.attrs.lineWrap,location=`표 ${tableNumber}, ${r+1}행 ${c+1}열`;
      // HWPX의 생략 기본값은 확정하지 않았으므로 BREAK로 대체하지 않는다.
      if(lineWrap===undefined)reject(`${location}의 lineWrap 생략 — 줄바꿈 설정을 확인할 수 없음`);
      if(lineWrap!=='BREAK')reject(`${location}의 셀 줄바꿈(lineWrap=${JSON.stringify(lineWrap)}) — BREAK만 지원`);
      if(sub.children.some(x=>x.local!=='p'))reject('미지원 셀 본문 요소');
      const cm=tc.attrs.hasMargin==='1'?child(tc,'cellMargin'):child(n,'inMargin');
      const fill=resolve('borderFill',tc.attrs.borderFillIDRef),brush=descendants(fill,'winBrush')[0];
      const fillColor=brush?.attrs.faceColor??'none';if(fillColor!=='none'&&!/^#[0-9a-f]{6}$/i.test(fillColor))reject('셀 배경');
      if(descendants(fill,'imgBrush').length||descendants(fill,'gradation').length)reject('그림·그라데이션 배경');
      const borders={};for(const side of ['left','right','top','bottom']){const border=child(fill,side+'Border');if(border&&!['NONE','SOLID'].includes(border.attrs.type))reject('표의 특수 테두리');const color=border?.attrs.color??'#000000';if(!/^#[0-9a-f]{6}$/i.test(color))reject('테두리 색');if(border?.attrs.type==='SOLID'&&!/^\d+(?:\.\d+)? mm$/.test(border.attrs.width))reject('테두리 두께 단위');borders[side]={color,width:border?.attrs.type==='SOLID'?finite(parseFloat(border.attrs.width)*72/25.4,0,10,'테두리 두께'):0};}
      const w=finite(unit(val(sz,'width')),5,2000,'셀 너비');if(widths[c]!==null&&Math.abs(widths[c]-w)>.02)reject('행마다 다른 열 너비');widths[c]=w;
      const paragraphs=sub.children.filter(x=>x.local==='p').map(paragraph);if(paragraphs.some(p=>p.pageBreak))reject('셀 내부 쪽 나눔');
      cells.push({r,c,width:w,minHeight:finite(unit(val(sz,'height')),0,10000,'셀 높이'),padding:Object.fromEntries(['left','right','top','bottom'].map(k=>[k,finite(unit(val(cm,k)),0,1000,'셀 여백')])),fill:fillColor==='none'?'#ffffff':fillColor,borders,header:tc.attrs.header==='1',vertical:sub.attrs.vertAlign.toLowerCase(),paragraphs});
    }row++;}
    if(row!==rows||cells.length!==rows*cols||new Set(cells.map(c=>`${c.r}:${c.c}`)).size!==cells.length||widths.some(w=>!w||w<5))reject('빠진 표 셀');
    const out=child(n,'outMargin');return {type:'table',rows,cols,cells,widths,repeatHeader:n.attrs.repeatHeader==='1',before:finite(unit(val(out,'top')),0,1000,'표 위 여백'),after:finite(unit(val(out,'bottom')),0,1000,'표 아래 여백'),left:finite(unit(val(out,'left')),0,1000,'표 왼쪽 여백')};
  }
  if(section.root.children.some(n=>n.local!=='p'))reject('구역의 미지원 요소');
  const blocks=section.root.children.filter(n=>n.local==='p').map(paragraph);
  if(!blocks.length)reject('본문 없음');
  return {page:pageModel,blocks,sourceSha256:sha256(bytes)};
}

// 자체 생성 HTML/데이터만 설치된 Chromium에 전달한다. 네트워크·파일 링크는 사용하지 않는다.
export async function renderHwpx(bytes){
  const model=readRenderModel(bytes);
  const {chromium}=await import('playwright-core'), browser=await launchBrowser(chromium);
  try{
    const page=await browser.newPage();await page.route('**/*',route=>route.abort());
    const layout=await page.evaluate(model=>{
      const canvas=document.createElement('canvas'),ctx=canvas.getContext('2d');
      const pages=[[]],warnings=['자체 조판 미리보기입니다. 실제 한글의 줄바꿈·쪽 나눔과 다를 수 있습니다.'];
      const p=model.page,width=p.width-p.left-p.right,bottom=p.height-p.bottom;let y=p.top;
      const font=s=>`${s.bold?'bold ':''}${s.size*4/3}px ${JSON.stringify(s.font)}`;
      const fonts=new Map();for(const b of model.blocks){const ps=b.type==='paragraph'?[b]:b.cells.flatMap(c=>c.paragraphs);for(const q of ps){fonts.set(q.style.font,q.style);if(q.align==='justify'&&!warnings.includes('양쪽 정렬은 왼쪽 정렬로 미리보기합니다.'))warnings.push('양쪽 정렬은 왼쪽 정렬로 미리보기합니다.');}}
      for(const s of fonts.values()){
        const sample='가나다라마바사 ABCDEFG ilMW 0123456789';let found=false;
        for(const fallback of ['monospace','serif']){ctx.font=`20px ${fallback}`;const base=ctx.measureText(sample).width;ctx.font=`20px ${JSON.stringify(s.font)},${fallback}`;if(Math.abs(ctx.measureText(sample).width-base)>.01)found=true;}
        if(!found)throw new Error(`설치 글꼴을 확인할 수 없습니다: ${s.font}`);
      }
      const measure=(t,s)=>{ctx.font=font(s);return ctx.measureText(t).width*3/4;};
      function lines(q,w){
        const available=w-q.left-q.right;if(available<q.style.size)throw new Error('본문·셀 너비가 글자보다 작습니다.');
        const out=[];
        for(const explicit of q.text.split('\n')){
          if(!explicit){out.push('');continue;}
          // 한글은 글자, 영문은 단어를 우선 보존. 긴 단어는 한 글자씩 분리.
          const tokens=explicit.match(/[A-Za-z0-9]+(?:[._/@-][A-Za-z0-9]+)*|[^A-Za-z0-9]/gu)??[];let line='';
          for(const token of tokens){if(measure(line+token,q.style)<=available){line+=token;continue;}if(line){out.push(line);line='';}for(const ch of token){if(measure(line+ch,q.style)>available&&line){out.push(line);line='';}line+=ch;}}
          out.push(line);
        }
        return out;
      }
      const next=()=>{if(pages.length>=500)throw new Error('미리보기 500쪽 한도 초과');pages.push([]);y=p.top;};
      const add=shape=>pages.at(-1).push(shape);
      function drawLine(q,line,x,top,w){const tw=measure(line,q.style),avail=w-q.left-q.right;let left=x+q.left;if(q.align==='center')left+=(avail-tw)/2;if(q.align==='right')left+=avail-tw;add({kind:'text',text:line,x:left,y:top+q.style.size*.85,style:q.style});}
      function para(q,x,w){const ls=lines(q,w),lh=q.style.size*q.lineSpacing;if(q.before+lh>bottom-p.top)throw new Error('문단 한 줄과 여백이 본문 높이를 넘습니다.');if(y+q.before+lh>bottom)next();y+=q.before;for(const line of ls){if(y+lh>bottom)next();drawLine(q,line,x,y,w);y+=lh;}y+=q.after;}
      for(const b of model.blocks){
        if(b.pageBreak&&pages.at(-1).length)next();
        if(b.type==='paragraph'){para(b,p.left,width);continue;}
        const tableWidth=b.widths.reduce((a,v)=>a+v,0);if(tableWidth+b.left>width+.1)throw new Error('표가 본문 너비를 넘습니다.');
        y+=b.before;
        const rowLayout=Array.from({length:b.rows},(_,r)=>{
          const cells=b.cells.filter(c=>c.r===r).sort((a,b)=>a.c-b.c).map(c=>{
            let cy=c.padding.top;const qs=c.paragraphs.map(q=>{const ls=lines(q,c.width-c.padding.left-c.padding.right);const at=cy+q.before;cy=at+ls.length*q.style.size*q.lineSpacing+q.after;return {q,ls,at};});
            return {c,qs,contentHeight:cy+c.padding.bottom};
          });return {cells,height:Math.max(...cells.map(x=>Math.max(x.c.minHeight,x.contentHeight)))};
        });
        const header=b.repeatHeader&&rowLayout[0].cells.every(x=>x.c.header)?rowLayout[0]:null;
        function drawRow(row){let x=p.left+b.left;for(const {c,qs,contentHeight}of row.cells){add({kind:'rect',x,y,width:c.width,height:row.height,fill:c.fill,borders:c.borders});let extra=c.vertical==='center'?(row.height-contentHeight)/2:c.vertical==='bottom'?row.height-contentHeight:0;for(const {q,ls,at}of qs){let yy=y+at+extra;for(const line of ls){drawLine(q,line,x+c.padding.left,yy,c.width-c.padding.left-c.padding.right);yy+=q.style.size*q.lineSpacing;}}x+=c.width;}y+=row.height;}
        for(let r=0;r<rowLayout.length;r++){
          const row=rowLayout[r];if(row.height>bottom-p.top)throw new Error('한 쪽보다 높은 표 행은 자체 렌더에서 지원하지 않습니다.');
          if(r===0&&header&&rowLayout[1]&&y+row.height+rowLayout[1].height>bottom&&y>p.top)next();
          if(y+row.height>bottom){next();if(r>0&&header){if(header.height+row.height>bottom-p.top)throw new Error('반복 머리행과 본문 행이 한 쪽에 들어가지 않습니다.');drawRow(header);}}
          drawRow(row);
        }
        y+=b.after;
      }
      return {pages,warnings,fonts:[...fonts.keys()]};
    },model);
    const escape=xmlEscape;
    const svgs=layout.pages.map((shapes,i)=>`<svg xmlns="http://www.w3.org/2000/svg" width="${model.page.width}pt" height="${model.page.height}pt" viewBox="0 0 ${model.page.width} ${model.page.height}" role="img" aria-label="자체 미리보기 ${i+1}쪽"><rect width="100%" height="100%" fill="white"/>${shapes.map(s=>s.kind==='text'?`<text x="${s.x}" y="${s.y}" font-family="${escape(s.style.font)}" font-size="${s.style.size}" font-weight="${s.style.bold?700:400}" fill="${s.style.color}" xml:space="preserve">${escape(s.text)}</text>`:`<rect x="${s.x}" y="${s.y}" width="${s.width}" height="${s.height}" fill="${s.fill}"/>${Object.entries(s.borders).map(([side,b])=>b.width?`<line x1="${s.x+(side==='right'?s.width:0)}" y1="${s.y+(side==='bottom'?s.height:0)}" x2="${s.x+(side==='left'?0:s.width)}" y2="${s.y+(side==='top'?0:s.height)}" stroke="${b.color}" stroke-width="${b.width}"/>`:'').join('')}`).join('')}</svg>`);
    const report={schemaVersion:1,sourceSha256:model.sourceSha256,status:'approximate-preview',pageCount:svgs.length,fonts:layout.fonts,warnings:layout.warnings,hangulVerified:false};
    return {report,svgs,model};
  }finally{await browser.close();}
}
export function previewHtml(result){
  return '<!doctype html><html lang="ko"><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src \'none\'; img-src data:; font-src data:; style-src \'unsafe-inline\'"><title>한글 문서 자체 미리보기</title><style>body{margin:0;background:#e5e7eb;font-family:굴림,sans-serif}.notice{padding:16px;background:#fff3cd;line-height:1.7}main{display:flex;flex-direction:column;align-items:center;gap:24px;padding:24px}svg{background:white;box-shadow:0 2px 8px #999;max-width:100%;height:auto}@media print{.notice{display:none}main{display:block;padding:0}svg{display:block;box-shadow:none;break-after:page;max-width:none}}'+(result.fontCss??'')+'</style><div class="notice"><strong>자체 미리보기 · 실제 한글 확인 전</strong><br>'+result.report.warnings.map(xmlEscape).join('<br>')+'</div><main>'+result.svgs.join('')+'</main></html>';
}
