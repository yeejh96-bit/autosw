// Derived from kordoc (MIT), Copyright 2026 chrisryugj. See ../LICENSE or package-root LICENSE and PROVENANCE.json.
import { createXmlParser } from "../hwpx/parser-shared.mjs";
import { reflowSection } from "./reflow.mjs";
import { elements, ln, num } from "./para-model.mjs";
export function hasKoPubSubst(headXml) {
    return /<hh:font\b[^>]*\bface="KoPub[^"]*"[^>]*\btype="TTF"[^>]*>\s*<hh:substFont\b/.test(headXml);
}
function lineStarts(root) {
    const out = [];
    for (const p of Array.from(root.getElementsByTagName("hp:p"))){
        const lsa = elements(p).find((e)=>ln(e) === "linesegarray");
        const segs = lsa ? elements(lsa).filter((e)=>ln(e) === "lineseg") : [];
        out.push(segs.length >= 2 ? segs.map((s)=>s.getAttribute("textpos") ?? "0").join(",") : null);
    }
    return out;
}
function matchCount(xml, saved, styles) {
    const root = createXmlParser().parseFromString(xml, "text/xml").documentElement;
    const pagePr = root.getElementsByTagName("hp:pagePr")[0];
    if (!pagePr) return 0;
    const margin = pagePr.getElementsByTagName("hp:margin")[0];
    let PW = num(pagePr, "width", 59528), PH = num(pagePr, "height", 84188);
    if (pagePr.getAttribute("landscape") === "NARROWLY" && PW < PH) [PW, PH] = [
        PH,
        PW
    ];
    const gutterTB = pagePr.getAttribute("gutterType") === "TOP_BOTTOM", gutter = num(margin ?? null, "gutter", 0);
    const BODY_W = PW - num(margin ?? null, "left", 8504) - num(margin ?? null, "right", 8504) - (gutterTB ? 0 : gutter);
    const BODY_H = PH - num(margin ?? null, "top", 5668) - num(margin ?? null, "header", 0) - num(margin ?? null, "bottom", 4252) - num(margin ?? null, "footer", 0) - (gutterTB ? gutter : 0);
    for (const p of Array.from(root.getElementsByTagName("hp:p"))){
        const lsa = elements(p).find((e)=>ln(e) === "linesegarray");
        if (lsa) p.removeChild(lsa);
    }
    reflowSection(root, styles, {
        BODY_W,
        BODY_H
    });
    const got = lineStarts(root);
    let n = 0;
    saved.forEach((s, i)=>{
        if (s !== null && s === got[i]) n++;
    });
    return n;
}
export function kopubSubstituted(secXmls, base, alt) {
    let a = 0, b = 0, seen = 0;
    for (const xml of secXmls){
        const saved = lineStarts(createXmlParser().parseFromString(xml, "text/xml").documentElement);
        a += matchCount(xml, saved, base);
        b += matchCount(xml, saved, alt);
        seen += saved.filter((x)=>x !== null).length;
        if (seen >= 60) break;
    }
    return b >= a + 5 && b > a * 1.1;
}
