import fs from 'node:fs';
import path from 'node:path';

export function previewEmbeddingAllowed(bytes){
  if(bytes.length<12||bytes.readUInt32BE(0)!==0x00010000)return false;
  const count=bytes.readUInt16BE(4);if(12+count*16>bytes.length)return false;
  for(let i=0;i<count;i++){
    const at=12+i*16;if(bytes.toString('ascii',at,at+4)!=='OS/2')continue;
    const offset=bytes.readUInt32BE(at+8),size=bytes.readUInt32BE(at+12);
    if(size<10||offset+size>bytes.length)return false;
    const flags=bytes.readUInt16BE(offset+8);
    return !(flags&0x0002)&&!(flags&0x0200); // 제한 임베딩·비트맵 전용 거부
  }return false;
}
export function preparePreviewFont(input){
  // 오래된 TTF의 없는 글자 표시(0xffff)는 Chromium OTS가 거부한다.
  // 설치 파일은 건드리지 않고 미리보기 사본의 잘못된 glyph 참조만 .notdef로 고친다.
  const b=Buffer.from(input),tables=new Map();
  for(let i=0;i<b.readUInt16BE(4);i++){const at=12+i*16;tables.set(b.toString('ascii',at,at+4),{at,offset:b.readUInt32BE(at+8),length:b.readUInt32BE(at+12)});}
  const cmap=tables.get('cmap'),maxp=tables.get('maxp');if(!cmap||!maxp)return b;
  const glyphs=b.readUInt16BE(maxp.offset+4);let changed=false;
  for(let i=0;i<b.readUInt16BE(cmap.offset+2);i++){
    const start=cmap.offset+b.readUInt32BE(cmap.offset+4+i*8+4);if(b.readUInt16BE(start)!==4)continue;
    const length=b.readUInt16BE(start+2),segments=b.readUInt16BE(start+6)/2;
    for(let j=0;j<segments;j++){
      const end=b.readUInt16BE(start+14+j*2),begin=b.readUInt16BE(start+16+segments*2+j*2),delta=start+16+segments*4+j*2,range=b.readUInt16BE(start+16+segments*6+j*2);
      if(begin===65535&&end===65535&&range===0&&b.readUInt16BE(delta)===0){b.writeUInt16BE(1,delta);changed=true;}
    }
    for(let at=start+16+segments*8;at<start+length;at+=2)if(b.readUInt16BE(at)>=glyphs){b.writeUInt16BE(0,at);changed=true;}
  }
  if(changed){
    const head=tables.get('head');if(head)b.writeUInt32BE(0,head.offset+8);
    const sum=(offset,length)=>{let s=0;for(let i=0;i<length;i+=4){let word=0;for(let j=0;j<4;j++)word=(word*256)+(i+j<length?b[offset+i+j]:0);s=(s+word)>>>0;}return s;};
    for(const t of tables.values())b.writeUInt32BE(sum(t.offset,t.length),t.at+4);
    if(head)b.writeUInt32BE((0xB1B0AFBA-sum(0,b.length))>>>0,head.offset+8);
  }return b;
}
export function previewFonts(svg){
  let css='';const fonts=[],warnings=[];
  for(const [family,file]of [['HYHeadLine-Medium','H2HDRM.TTF'],['HYSinMyeongJo-Medium','H2MJSM.TTF']]){
    if(!svg.includes(family))continue;
    const location=path.join(process.env.SystemRoot??'C:/Windows','Fonts',file);
    if(!fs.existsSync(location)){warnings.push(`미리보기 글꼴을 찾지 못했습니다: ${family}`);continue;}
    const bytes=fs.readFileSync(location);
    if(bytes.length>10*1024*1024||!previewEmbeddingAllowed(bytes)){warnings.push(`글꼴 미리보기 포함이 허용되지 않아 대체 글꼴로 표시합니다: ${family}`);continue;}
    css+=`@font-face{font-family:'${family}';src:url(data:font/ttf;base64,${preparePreviewFont(bytes).toString('base64')}) format('truetype');font-weight:normal;font-style:normal;}`;
    fonts.push(family);
  }return {css,fonts,warnings};
}
