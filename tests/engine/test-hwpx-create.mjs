import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {composeHwpx,normalizeDocument,documentFromMarkdown} from '../../engine/hwpx-compose.mjs';
import {readRenderModel,renderHwpx,previewHtml} from '../../engine/hwpx-render.mjs';
import {readZipStrict,patchZip,createZip,parseOfficeXml} from '../../engine/office-zip.mjs';
import {loadHwpx,sha256} from '../../engine/hwpx-document.mjs';
import {previewCreation,applyCreation,verifyCreation,validateCreation,renderFile} from '../../engine/hwpx-create.mjs';
const root=fs.mkdtempSync(path.join(os.tmpdir(),'autosw-hwpx-create-'));
const file=name=>path.join(root,name);let count=0;
const test=async(name,fn)=>{try{await fn();count++;}catch(e){throw new Error(`${name}: ${e.message}`,{cause:e});}};
const model={schemaVersion:1,title:'새 생성 시험',blocks:[{type:'heading',text:'한글 생성 < & >',level:1},{type:'paragraph',text:'첫째 줄\n둘째 줄 12345'},{type:'list',items:['항목 가','항목 나']},{type:'table',rows:[['항목','값'],['특수문자','<tag> & "인용"']]},{type:'pageBreak'},{type:'paragraph',text:'마지막 본문'}]};
const {bytes}=composeHwpx(model);
const mutate=(name,from,to)=>{const z=readZipStrict(bytes);assert(z.read(name).toString().includes(from));return patchZip(z,new Map([[name,Buffer.from(z.read(name).toString().replace(from,to))]]));};
const lineWrapBytes=composeHwpx({schemaVersion:1,blocks:[model.blocks[3],model.blocks[3]]}).bytes;
function cellLineWrap(index,value){
 const z=readZipStrict(lineWrapBytes),name='Contents/section0.xml',xml=z.read(name).toString();
 const sub=parseOfficeXml(xml).nodes.filter(n=>n.local==='subList'&&n.parent?.local==='tc')[index];
 assert(sub);assert.equal(sub.attrs.lineWrap,'BREAK');
 const opening=xml.slice(sub.start,sub.openEnd).replace(' lineWrap="BREAK"',value===undefined?'':` lineWrap="${value}"`);
 return patchZip(z,new Map([[name,Buffer.from(xml.slice(0,sub.start)+opening+xml.slice(sub.openEnd))]]));
}
function lineWrapError(index,value){return error=>{
 assert.match(error.message,/자체 렌더 미지원/);
 assert(error.message.includes(`표 ${Math.floor(index/4)+1}, ${Math.floor(index%4/2)+1}행 ${index%2+1}열`),error.message);
 assert(error.message.includes(value===undefined?'lineWrap 생략':`lineWrap=${JSON.stringify(value)}`),error.message);
 if(value===undefined)assert.match(error.message,/줄바꿈 설정을 확인할 수 없/);
 assert.match(error.message,/실제 한글에서 확인하세요/);return true;
};}
try{
 await test('원고 결정성·본문·특수문자·표',()=>{assert(bytes.equals(composeHwpx(model).bytes));const s=loadHwpx(bytes).structure.sections[0];assert.deepEqual(s.paragraphs.map(p=>p.text),['한글 생성 < & >','첫째 줄\n둘째 줄 12345','• 항목 가','• 항목 나','','항목','값','특수문자','<tag> & "인용"','마지막 본문']);assert.equal(s.tables.length,1);assert.equal(readZipStrict(bytes).entries[0].name,'mimetype');});
 await test('기본 글꼴과 명시 쪽 나눔',()=>{const m=readRenderModel(bytes);assert.equal(m.blocks[0].style.font,'굴림');assert.equal(m.blocks[1].style.size,11);assert.equal(m.blocks.at(-1).pageBreak,true);});
 await test('첫 블록 표와 문단 ID',()=>{const b=composeHwpx({schemaVersion:1,blocks:[{type:'table',rows:[['가','나']]},{type:'pageBreak'},{type:'table',rows:[['다','라']]}]}).bytes;const s=parseOfficeXml(readZipStrict(b).read('Contents/section0.xml').toString());assert.equal(s.nodes.filter(n=>n.local==='secPr').length,1);const ids=s.nodes.filter(n=>n.local==='p').map(n=>n.attrs.id);assert.equal(ids.length,new Set(ids).size);assert.equal(readRenderModel(b).blocks[1].pageBreak,true);});
 await test('잘못된 문서·지원 외 블록 차단',()=>{for(const bad of [{...model,unknown:1},{...model,schemaVersion:2},{...model,blocks:[]},{...model,blocks:[{type:'picture',text:'x'}]},{...model,blocks:[{type:'pageBreak'}]},{...model,style:{fontSize:NaN}},{...model,page:{widthMm:100,marginsMm:{left:70,right:70}}}])assert.throws(()=>normalizeDocument(bad));});
 await test('본문 제어문자·표·열 폭 검증',()=>{for(const blocks of [[{type:'paragraph',text:'a\tb'}],[{type:'paragraph',text:'a\u0000b'}],[{type:'table',rows:[['a'],['a','b']]}],[{type:'table',rows:[['a','b']],columnWidths:[.1,100]}]])assert.throws(()=>composeHwpx({schemaVersion:1,blocks}));});
 await test('제한 Markdown 구조·구분선',()=>{const m=documentFromMarkdown('# 제목\n\n본문\n두 줄\n\n- 하나\n- 둘\n\n| 열1 | 열2 |\n| --- | --- |\n| 가 | 나 |\n\n마지막\n***\n다음');assert.deepEqual(m.blocks.map(b=>b.type),['heading','paragraph','list','table','paragraph','pageBreak','paragraph']);assert.equal(m.blocks[1].text,'본문\n두 줄');});
 await test('Markdown 미지원 내용 거부',()=>{for(const md of ['* item','본문 *강조*','본문 _강조_','![사진](a)','<script>alert(1)</script>','`code`','| a | b |\n| --- |\n| c | d |'])assert.throws(()=>documentFromMarkdown(md));});
 await test('실제 파일 재읽기',()=>{const changed=mutate('Contents/section0.xml','마지막 본문','변경된 본문');assert.equal(readRenderModel(changed).blocks.at(-1).text,'변경된 본문');assert.notEqual(readRenderModel(changed).sourceSha256,readRenderModel(bytes).sourceSha256);});
 await test('병합·중첩·그림 등 조용한 누락 금지',()=>{for(const [from,to]of [['colSpan="1"','colSpan="2"'],['<hp:t>마지막 본문</hp:t>','<hp:pic/><hp:t>마지막 본문</hp:t>'],['horzAlign="LEFT"','horzAlign="CENTER"'],['vertRelTo="PARA"','vertRelTo="PAGE"'],['treatAsChar="0"','treatAsChar="1"']])assert.throws(()=>readRenderModel(mutate('Contents/section0.xml',from,to)));});
 await test('수치·줄 간격·문단 묶음 거부',()=>{for(const [from,to]of [['height="1100"','height="x"'],['height="1100"','height="-100"'],['value="160"','value="-1"'],['keepLines="0"','keepLines="1"'],['keepWithNext="0"','keepWithNext="1"']])assert.throws(()=>readRenderModel(mutate('Contents/header.xml',from,to)));});
 await test('ZIP 생성 검증',()=>{assert.throws(()=>createZip(new Map([['../x','y']])));assert.throws(()=>createZip(new Map()));const z=createZip(new Map([['mimetype','application/hwp+zip'],['한글.txt','가나다']]));assert.equal(readZipStrict(z).read('한글.txt').toString(),'가나다');});
 await test('캡션과 셀의 미지원 본문 누락 차단',()=>{assert.throws(()=>readRenderModel(mutate('Contents/section0.xml','<hp:tr>','<hp:caption><hp:subList><hp:p><hp:run><hp:t>CAPTION</hp:t></hp:run></hp:p></hp:subList></hp:caption><hp:tr>')),/캡션/);assert.throws(()=>readRenderModel(mutate('Contents/section0.xml','<hp:cellAddr','<hp:unknown/><hp:cellAddr')),/셀 요소/);});
 await test('모든 표 셀의 SQUEEZE·KEEP와 위치별 차단',()=>{
   for(let index=0;index<8;index++)for(const value of ['SQUEEZE','KEEP'])assert.throws(()=>readRenderModel(cellLineWrap(index,value)),lineWrapError(index,value));
 });
 await test('뒤쪽 셀의 알 수 없는·빈·생략 줄바꿈 차단',()=>{
   for(const value of ['UNKNOWN','','break',' BREAK ',undefined])assert.throws(()=>readRenderModel(cellLineWrap(7,value)),lineWrapError(7,value));
 });
 await test('명시 BREAK의 본문·셀 구조·원본 보존',()=>{
   const before=Buffer.from(lineWrapBytes),m=readRenderModel(lineWrapBytes);
   assert.equal(m.blocks.length,2);for(const table of m.blocks){assert.equal(table.rows,2);assert.equal(table.cols,2);assert.deepEqual(table.cells.map(c=>[c.r,c.c,c.paragraphs.map(p=>p.text)]),[[0,0,['항목']],[0,1,['값']],[1,0,['특수문자']],[1,1,['<tag> & "인용"']]]);}
   assert.deepEqual(readRenderModel(cellLineWrap(7,'BREAK')).blocks,m.blocks);assert(lineWrapBytes.equals(before));
 });
 let result;
 await test('실제 Chromium 렌더·글꼴·쪽·HTML 이스케이프',async()=>{result=await renderHwpx(bytes);assert.equal(result.report.pageCount,2);assert.equal(result.report.status,'approximate-preview');assert.equal(result.report.hangulVerified,false);assert(result.svgs[0].includes('&lt;tag&gt;'));assert(!result.svgs[0].includes('<tag>'));assert(previewHtml(result).includes("default-src 'none'"));assert(!result.svgs.join('').includes('NaN'));});
 await test('렌더의 뒤쪽 셀 미지원 줄바꿈 실패 전달',async()=>{
   for(const value of ['SQUEEZE','KEEP','UNKNOWN','',undefined])await assert.rejects(renderHwpx(cellLineWrap(7,value)),lineWrapError(7,value));
 });
 await test('여러 쪽 표 머리행 반복·본문 전체 보존',async()=>{const b=composeHwpx({schemaVersion:1,blocks:[{type:'table',columnWidths:[1,3],rows:[['순번','설명'],...Array.from({length:60},(_,i)=>[String(i),'행-'+i])]}]}).bytes;const r=await renderHwpx(b);assert(r.svgs.length>=2);for(const svg of r.svgs)assert(svg.includes('>순번</text>'));for(let i=0;i<60;i++)assert.equal(r.svgs.join('').split('>행-'+i+'</text>').length-1,1);});
 await test('설치되지 않은 글꼴 차단',async()=>{const b=composeHwpx({...model,style:{font:'NONEXISTENT_9D8B6C_FONT'}}).bytes;await assert.rejects(renderHwpx(b),/글꼴/);});
 const input=file('원고 & 시험.json'),output=file('문서.hwpx');fs.writeFileSync(input,JSON.stringify(model));
 let plan;
 await test('생성 미리보기·변조 거부',async()=>{plan=await previewCreation(input,output);assert(!fs.existsSync(output));assert.throws(()=>applyCreation(plan,'wrong'));assert.throws(()=>validateCreation({...plan,outputPath:file('다른.hwpx')}));fs.writeFileSync(input,JSON.stringify({...model,title:'변경'}));assert.throws(()=>applyCreation(plan,plan.planSha256));assert(!fs.existsSync(output));fs.writeFileSync(input,JSON.stringify(model));});
 await test('손상 UTF-8 원고 거부',async()=>{const invalid=file('손상.md');fs.writeFileSync(invalid,Buffer.from([0xb0,0xa1]));await assert.rejects(previewCreation(invalid,file('손상.hwpx')),/encoding|encoded|UTF/i);assert(!fs.existsSync(file('손상.hwpx')));});
 if(process.platform==='win32'){
   await test('실제 독립 대조·저장·덮어쓰기 차단',()=>{const r=applyCreation(plan,plan.planSha256);assert.equal(r.independentCheck,'passed');const before=sha256(fs.readFileSync(output));assert.throws(()=>applyCreation(plan,plan.planSha256));assert.equal(before,sha256(fs.readFileSync(output)));assert.equal(verifyCreation(plan).sha256,before);fs.appendFileSync(output,'bad');assert.throws(()=>verifyCreation(plan));fs.writeFileSync(output,bytes);});
   await test('PowerShell 실제 진입점·특수문자 경로',()=>{const cli=fileURLToPath(new URL('../../engine/hwpx-create.ps1',import.meta.url));const pp=file('미리보기 & 시험.json');const r=spawnSync('powershell.exe',['-NoProfile','-File',cli,'-Command','preview','-InputPath',input,'-OutputPath',file('새 & 문서.hwpx'),'-PlanPath',pp],{encoding:'utf8',windowsHide:true,timeout:60000});assert.equal(r.status,0,r.stderr);assert(fs.existsSync(pp));const p=JSON.parse(fs.readFileSync(pp,'utf8'));assert.equal(p.model.title,model.title);});
   await test('PowerShell render 줄바꿈 실패 코드·원인·신규 결과 없음·기존 파일 보존',()=>{
     const cli=fileURLToPath(new URL('../../engine/hwpx-create.ps1',import.meta.url));
     const source=file('줄바꿈 & 원본.hwpx'),html=file('줄바꿈 & 미리보기.html'),report=file('줄바꿈 & 보고.json');
     const run=()=>{const r=spawnSync('powershell.exe',['-NoProfile','-File',cli,'-Command','render','-InputPath',source,'-OutputPath',html,'-ReportPath',report],{encoding:'utf8',windowsHide:true,timeout:60000});assert.ifError(r.error);assert.equal(r.signal,null);assert.equal(r.status,1,r.stderr);return r;};
     for(const value of ['SQUEEZE','KEEP','UNKNOWN','',undefined]){
       const sourceBytes=cellLineWrap(7,value);fs.writeFileSync(source,sourceBytes);
       const names=fs.readdirSync(root).sort(),r=run();lineWrapError(7,value)(new Error(r.stderr));
       assert.equal(r.stdout.trim(),'');assert(!fs.existsSync(html));assert(!fs.existsSync(report));
       assert.deepEqual(fs.readdirSync(root).sort(),names);assert(fs.readFileSync(source).equals(sourceBytes));
     }
     const sourceBytes=fs.readFileSync(source),savedHtml=Buffer.from('<html>기존 미리보기</html>'),savedReport=Buffer.from('{"status":"기존 보고"}');
     fs.writeFileSync(html,savedHtml);fs.writeFileSync(report,savedReport);const names=fs.readdirSync(root).sort();
     const r=run();assert.match(r.stderr,/기존 파일/);assert.deepEqual(fs.readdirSync(root).sort(),names);
     assert(fs.readFileSync(source).equals(sourceBytes));assert(fs.readFileSync(html).equals(savedHtml));assert(fs.readFileSync(report).equals(savedReport));
   });
 }
 await test('렌더 출력·기존 미리보기 보존',async()=>{const html=file('preview.html'),report=file('report.json');if(!fs.existsSync(output))fs.writeFileSync(output,bytes);const r=await renderFile(output,html,report);assert.equal(r.pageCount,2);const before=fs.readFileSync(html);await assert.rejects(renderFile(output,html,report));assert(before.equals(fs.readFileSync(html)));});
 console.log(`SELFTEST_OK (HWPX 새 생성·자체 렌더 ${count}개)`);
}finally{
 for(const name of fs.readdirSync(root)){const target=path.join(root,name);if(fs.statSync(target).isFile())fs.unlinkSync(target);else throw new Error('검사용 폴더에 예상 밖 하위 폴더가 생겼습니다.');}
 fs.rmdirSync(root);
}
