// my-system: HWPX 셀·문단 그라데이션. 좌표는 HWPUNIT, 출력은 pt.
export function paintFill(bf,x,y,w,h,ctx){
  const rect=(fill,xx=x,yy=y,ww=w,hh=h)=>`<rect x="${xx/100}" y="${yy/100}" width="${ww/100}" height="${hh/100}" fill="${fill}"/>`;
  if(!bf?.gradient)return bf?.fill?rect(bf.fill):'';
  const g=bf.gradient,first=g.colors[0],last=g.colors.at(-1);
  if(g.type==='SQUARE'){
    const rgb=c=>[1,3,5].map(i=>parseInt(c.slice(i,i+2),16)),a=rgb(last),b=rgb(first);let out='';
    const cx=Math.max(0,Math.min(100,g.centerX))/100,cy=Math.max(0,Math.min(100,g.centerY))/100;
    for(let i=0;i<64;i++){const t=i/64,c='#'+a.map((v,k)=>Math.round(v+(b[k]-v)*t).toString(16).padStart(2,'0')).join('');out+=rect(c,x+w*cx*t,y+h*cy*t,w*(1-t),h*(1-t));}return out;
  }
  const id=`mysystem-gradient-${ctx.defs.length}`;
  const angle=g.angle*Math.PI/180,dx=Math.sin(angle)/2,dy=Math.cos(angle)/2;
  const center=Math.max(0,Math.min(100,Math.abs(dx)>Math.abs(dy)?g.centerX:g.centerY));
  ctx.defs.push(`<linearGradient id="${id}" x1="${(0.5-dx)*100}%" y1="${(0.5-dy)*100}%" x2="${(0.5+dx)*100}%" y2="${(0.5+dy)*100}%"><stop offset="0%" stop-color="${last}"/><stop offset="${center}%" stop-color="${first}"/><stop offset="100%" stop-color="${last}"/></linearGradient>`);
  return rect(`url(#${id})`);
}
