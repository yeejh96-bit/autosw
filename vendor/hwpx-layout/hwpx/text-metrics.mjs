// Derived from kordoc (MIT), Copyright 2026 chrisryugj. See ../LICENSE or package-root LICENSE and PROVENANCE.json.
import { HANCOM_FALLBACK_W, fontWidthFn, hasFontMetrics, isHalfwidthForm } from "./font-metrics.mjs";
const ASCII_W = [
    300,
    320,
    320,
    610,
    610,
    830,
    724,
    320,
    320,
    320,
    550,
    550,
    320,
    550,
    320,
    550,
    550,
    550,
    550,
    550,
    550,
    550,
    550,
    550,
    550,
    550,
    320,
    320,
    550,
    550,
    550,
    550,
    830,
    706,
    605,
    685,
    719,
    627,
    617,
    683,
    734,
    305,
    315,
    660,
    605,
    839,
    734,
    732,
    603,
    705,
    660,
    627,
    664,
    731,
    706,
    910,
    705,
    705,
    626,
    320,
    550,
    320,
    550,
    550,
    320,
    569,
    597,
    552,
    597,
    536,
    356,
    562,
    635,
    287,
    288,
    582,
    287,
    907,
    635,
    588,
    597,
    579,
    478,
    496,
    356,
    635,
    563,
    720,
    542,
    543,
    486,
    320,
    320,
    320,
    550,
    0
];
const SYM_W = {
    0xa0: 300,
    0xa3: 568,
    0xa5: 707,
    0xa7: 498,
    0xab: 440,
    0xac: 564,
    0xb0: 291,
    0xb1: 798,
    0xb6: 606,
    0xb7: 320,
    0xbb: 440,
    0xd7: 617,
    0xf7: 678,
    0x2013: 625,
    0x2014: 875,
    0x2015: 875,
    0x2018: 320,
    0x2019: 320,
    0x201c: 480,
    0x201d: 480,
    0x2020: 558,
    0x2021: 438,
    0x2025: 640,
    0x2026: 960,
    0x2030: 988,
    0x2032: 335,
    0x2033: 474,
    0x203b: 770,
    0x20ac: 656,
    0x261c: 1012,
    0x261e: 1012,
    0x274d: 970,
    0x2bc: 164,
    0x2022: 399,
    0x2023: 369,
    0x2024: 289,
    0x2027: 289
};
export function charWidthEm1000(cp) {
    if (cp >= 0x20 && cp <= 0x7e) return ASCII_W[cp - 0x20];
    const sym = SYM_W[cp];
    if (sym !== undefined) return sym;
    if (isHalfwidthForm(cp)) return 500;
    if (cp >= 0xac00 && cp <= 0xd7a3) return 970;
    if (cp >= 0x1100 && cp <= 0x11ff) return 970;
    if (cp >= 0x3131 && cp <= 0x318e) return 970;
    if (cp >= 0x4e00 && cp <= 0x9fff || cp >= 0xf900 && cp <= 0xfaff) return 1000;
    if (cp >= 0x3008 && cp <= 0x3011 || cp >= 0x3014 && cp <= 0x301b) return 500;
    if (cp === 0x3000) return 970;
    if (cp >= 0x2160 && cp <= 0x2183) return 970;
    if (cp >= 0x2190 && cp <= 0x22ff) return 970;
    if (cp >= 0x2460 && cp <= 0x24ff) return 970;
    if (cp >= 0x25a0 && cp <= 0x26ff) return 970;
    if (cp >= 0x3200 && cp <= 0x33ff) return 970;
    if (cp >= 0xff01 && cp <= 0xff60) return 970;
    if (cp >= 0x2300 && cp <= 0x23ff || cp >= 0x2500 && cp <= 0x259f || cp >= 0x2700 && cp <= 0x27bf) return 970;
    if (cp >= 0xf0000) return 485;
    return cp >= 0x2e80 ? 970 : 550;
}
function isZeroWidthCp(cp) {
    return cp >= 0x200b && cp <= 0x200d || cp === 0x2060 || cp === 0xfeff || cp >= 0x300 && cp <= 0x36f;
}
export const SPACE_EM_FIXED = 500;
export const SPACE_EM_FONT = 300;
const FIXED_PITCH_FACES = /^(굴림체|돋움체|바탕체|궁서체|명조|고딕)$/;
export function faceClassOf(face) {
    if (face && hasFontMetrics(face)) return `font:${face.trim()}`;
    return face && FIXED_PITCH_FACES.test(face.trim()) ? "fixedPitch" : "hcr";
}
export function syntheticBoldEm(face, hft = false) {
    if (!face) return 0;
    const f = face.trim();
    return /^(명조|고딕|필기)$/.test(f) || hft && (/^(한양|휴먼)/.test(f) || /^(#세나루|강낭콩|HCI Tulip|신명 태고딕)$/.test(f)) ? 50 : 0;
}
const REFLOW_FACE_ALIAS = {
    "Haansoft Batang": "한컴바탕",
    "Haansoft Dotum": "한컴돋움",
    "HCR Batang": "함초롬바탕",
    "HCR Dotum": "함초롬돋움"
};
export function faceClassForReflow(face) {
    face = face && (REFLOW_FACE_ALIAS[face.trim()] ?? face);
    const c = faceClassOf(face);
    if (c !== "hcr" || !face || /^함초롬/.test(face.trim())) return c;
    return "cjk";
}
export function faceClassForGen(face) {
    if (!face) return "hcr";
    const f = face.trim();
    if (hasFontMetrics(f)) return `font:${f}`;
    if (FIXED_PITCH_FACES.test(f)) return "fixedPitch";
    if (/^함초롬/.test(f)) return "hcr";
    return "gothic";
}
function gothicWidthEm1000(cp) {
    if (cp >= 0x80) return 1000;
    if (cp >= 0x41 && cp <= 0x5a) return 720;
    if (cp >= 0x61 && cp <= 0x7a) return 560;
    if (cp >= 0x30 && cp <= 0x39) return 580;
    if (cp === 0x28 || cp === 0x29 || cp === 0x5b || cp === 0x5d) return 350;
    if (cp === 0x2e || cp === 0x2c || cp === 0x3a || cp === 0x3b || cp === 0x27) return 280;
    return 500;
}
function cjkWidthEm1000(cp) {
    return isHalfwidthForm(cp) ? 500 : cp >= 0x1100 ? 1000 : charWidthEm1000(cp);
}
function fixedPitchWidthEm1000(cp) {
    return cp < 0x80 || isHalfwidthForm(cp) ? 500 : 1000;
}
const isPuaFull = (cp)=>cp <= 0xf02fb || cp >= 0xf035f && cp <= 0xf03f4;
function widthFnOf(faceClass, fallback = true) {
    const base = baseWidthFnOf(faceClass, fallback);
    const hcr = faceClass === "hcr" || faceClass === "font:함초롬돋움" || faceClass === "font:함초롬바탕";
    const f = (cp)=>cp >= 0xf0000 ? isPuaFull(cp) ? 1000 : 500 : cp === 0xf0a0 ? base(0x20) : !hcr && cp >= 0x2776 && cp <= 0x2793 ? 1000 : base(cp);
    if (fallback && (faceClass === "fixedPitch" || faceClass === "cjk")) return (cp)=>isZeroWidthCp(cp) ? 0 : HANCOM_FALLBACK_W[cp] ?? f(cp);
    return (cp)=>isZeroWidthCp(cp) ? 0 : f(cp);
}
function baseWidthFnOf(faceClass, fallback = true) {
    if (faceClass === "fixedPitch") return fixedPitchWidthEm1000;
    if (faceClass === "gothic") return (cp)=>isHalfwidthForm(cp) ? 500 : gothicWidthEm1000(cp);
    if (faceClass === "cjk") return cjkWidthEm1000;
    if (faceClass?.startsWith("font:")) {
        const face = faceClass.slice(5);
        return fontWidthFn(face, fallback) ?? gothicWidthEm1000;
    }
    return charWidthEm1000;
}
function isSpaceCp(cp) {
    return cp === 0x20 || cp === 0xa0;
}
export function measureTextWidth(text, height, ratioPct, opts) {
    const spaceEm = opts?.spaceEm ?? SPACE_EM_FIXED;
    const spacing = opts?.spacingPct ?? 0;
    const widthEm = widthFnOf(opts?.faceClass);
    let em = 0;
    for (const ch of text){
        const cp = ch.codePointAt(0);
        const w = cp === 0x2005 ? 250 : isSpaceCp(cp) ? spaceEm : widthEm(cp);
        em += w * (1 + spacing / 100);
    }
    return em / 1000 * height * (ratioPct / 100);
}
export function mulDiv(a, b, c) {
    const e = a * b;
    return Math.trunc(e < 0 ? (e - c / 2) / c : (e + c / 2) / c);
}
export function hancomCharWidth(ch, height, ratioPct, spacingPct, faceClass, emAdd = 0, fallback = true) {
    const cp = ch.codePointAt(0);
    let w;
    if (cp === 0x2005) {
        const s = Math.trunc(height / 16);
        w = 4 * (ratioPct === 100 ? s : mulDiv(s, ratioPct, 100));
    } else if (isSpaceCp(cp)) {
        const s = Math.trunc(height / 8);
        w = 4 * (ratioPct === 100 ? s : mulDiv(s, ratioPct, 100));
    } else return hancomEmWidth(widthFnOf(faceClass, fallback)(cp) + emAdd, height, ratioPct, spacingPct);
    if (spacingPct) w += 4 * mulDiv(Math.trunc(w / 4), spacingPct, 100);
    return w;
}
export function hancomEmWidth(em, height, ratioPct, spacingPct) {
    const q = Math.trunc(height / 4);
    let w = ratioPct === 100 ? 4 * mulDiv(em, q, 1000) : 4 * Math.trunc(em * q * ratioPct / 100000);
    if (spacingPct) w += 4 * mulDiv(Math.trunc(w / 4), spacingPct, 100);
    return w;
}
const FORBID_START = new Set([
    ..."!%),.:;?]}¢°′″℃〉》」』】〕!%),.:;?]}₩~…·、。〃"
]);
const FORBID_END = new Set([
    ..."$([{£¥〈《「『【〔$([{₩"
]);
const HANCOM_FORBID_START = new Set([
    ..."!%),.:;>?]^`}~°·〉》」』】〕〗〙〛・！％），．：；＞？］＾｀｝～･ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ、。〃々ー゛゜ゝゞヽヾ’”․‥…‰‱′″‴‵‶‷℃℉"
]);
const HANCOM_FORBID_END = new Set([
    ..."(<[\\{§‘“〈《「『【〔〖〘〚＄（＜［＼｛￠￡￥￦$¢£¤¥৲৳฿",
    ...Array.from({
        length: 13
    }, (_, i)=>String.fromCharCode(0x20a0 + i))
]);
const isWrapSpace = (ch)=>ch === " " || ch === "\u2005";
export function prepareWrap(text, mode, opts) {
    const widthEm = widthFnOf(opts?.faceClass), spaceEm = opts?.spaceEm ?? SPACE_EM_FIXED;
    const advances = [], steps = [];
    let i = 0;
    for (const ch of text){
        const cp = ch.codePointAt(0);
        advances[i] = cp === 0x2005 ? 250 : isSpaceCp(cp) ? spaceEm : widthEm(cp);
        steps[i] = ch.length;
        i += ch.length;
    }
    return {
        units: splitWrapUnits(text, mode),
        advances,
        steps
    };
}
function splitWrapUnits(text, mode, latinBreak = false) {
    if (latinBreak) return text.match(mode === "keep" ? /[ \u2005]+|[\x21-\x7e]|[^ \u2005\x21-\x7e]+/gu : /[ \u2005]+|[^ \u2005](?:\u00a0[^ \u2005\u00a0]?)*/gu) ?? [];
    return text.match(mode === "keep" ? /[ \u2005]+|\ufdd0+(?=[^ \u2005\ufdd0])|[^ \u2005]+/gu : /[ \u2005]+|\u2060*(?:[\x21-\x7e\u00a1-\u024f\u2018-\u201f]+|[^ \u2005])(?:\u00a0(?:[\x21-\x7e\u00a1-\u024f\u2018-\u201f]+|[^ \u2005\u00a0])?)*/gu) ?? [];
}
export function simulateWrap(text, firstWidth, contWidth, height, ratioPct, mode = "keep", opts) {
    if (!opts?.prepared && text.includes("\n")) {
        const starts = [];
        let off = 0, lastLineWidth = 0;
        for(;;){
            const end = text.indexOf("\n", off), stop = end < 0 ? text.length : end, li = starts.length;
            const lw = opts?.lineWidth;
            const sub = simulateWrap(text.slice(off, stop), li === 0 ? firstWidth : contWidth, contWidth, height, ratioPct, mode, {
                ...opts,
                widths: opts?.widths?.slice(off, stop),
                spacings: opts?.spacings?.slice(off, stop),
                lineWidth: lw ? (i)=>lw(li + i) : undefined
            });
            for (const st of sub.starts)starts.push(off + st);
            lastLineWidth = sub.lastLineWidth;
            if (end < 0) break;
            off = end + 1;
        }
        return {
            lines: starts.length,
            starts,
            lastLineWidth
        };
    }
    const EPS = 0.5;
    const spaceEm = opts?.spaceEm ?? SPACE_EM_FIXED;
    const spacing = opts?.spacingPct ?? 0;
    const prepared = opts?.prepared;
    const widthEm = prepared ? null : widthFnOf(opts?.faceClass);
    const k = height * ratioPct / 100 / 1000;
    const unitW = opts?.widths;
    const forbidStart = opts?.kinsoku === "hancom" ? HANCOM_FORBID_START : FORBID_START;
    const forbidEnd = opts?.kinsoku === "hancom" ? HANCOM_FORBID_END : FORBID_END;
    const condense = opts?.condensePct ?? 0;
    const hancom = opts?.kinsoku === "hancom";
    const spAt = opts?.spacings;
    const lastSp = (from, to)=>{
        if (!spAt || to <= from) return 0;
        if (opts?.rightTabs?.size) {
            for (const t of opts.rightTabs)if (t >= lineStart() && t < to) return 0;
        }
        let i = to - 1;
        if (i > from && /[\uDC00-\uDFFF]/.test(text[i])) i--;
        return spAt[i] ?? 0;
    };
    const charWAt = (i, ch)=>{
        if (unitW) return (unitW[i] ?? 0) + (ch.length === 2 ? unitW[i + 1] ?? 0 : 0);
        if (prepared) return prepared.advances[i] * (1 + spacing / 100) * k;
        const cp = ch.codePointAt(0);
        return (cp === 0x2005 ? 250 : isSpaceCp(cp) ? spaceEm : widthEm(cp)) * (1 + spacing / 100) * k;
    };
    const rangeW = (from, to)=>{
        let w = 0;
        if (prepared && !unitW) {
            for(let i = from; i < to; i += prepared.steps[i])w += prepared.advances[i] * (1 + spacing / 100) * k;
            return w;
        }
        for(let i = from; i < to;){
            const ch = String.fromCodePoint(text.codePointAt(i));
            w += charWAt(i, ch);
            i += ch.length;
        }
        return w;
    };
    const units = prepared?.units ?? splitWrapUnits(text, mode, opts?.latinBreak);
    const starts = [
        0
    ];
    let lineW = 0;
    const lineWidth = opts?.lineWidth;
    let avail = lineWidth ? lineWidth(0) : firstWidth;
    let pos = 0;
    let spaceW = 0;
    let realSeen = false;
    let lineEnded = false;
    let placed = 0;
    const empty = ()=>lineW === 0 && (!hancom || placed === 0);
    const cap = ()=>avail + (condense > 0 ? Math.trunc(spaceW * condense / 100) : 0) + EPS;
    const lineStart = ()=>starts[starts.length - 1];
    const breakBefore = (unitPos, w, split = false)=>{
        let bp = unitPos;
        const u = text[unitPos];
        const prevStart = (end)=>end - (end >= 2 && /[\uDC00-\uDFFF]/.test(text[end - 1]) && /[\uD800-\uDBFF]/.test(text[end - 2]) ? 2 : 1);
        const old = opts?.before2024;
        const splitIn = opts?.kinsoku === "hancom" && split && u !== undefined && !isWrapSpace(u) && !isWrapSpace(text[bp - 1]);
        const inWord = old && splitIn;
        if (!splitIn && u !== undefined && forbidStart.has(u) && prevStart(bp) > lineStart() && !isWrapSpace(text[bp - 1])) {
            bp = prevStart(bp);
            if (old && forbidStart.has(text[bp]) && prevStart(bp) > lineStart() && !isWrapSpace(text[bp - 1])) bp = prevStart(bp);
        }
        while(!inWord && bp - 1 >= lineStart() && forbidEnd.has(text[bp - 1]))bp--;
        if (bp <= lineStart()) bp = unitPos;
        starts.push(bp);
        avail = lineWidth ? lineWidth(starts.length - 1) : contWidth;
        spaceW = 0;
        realSeen = w > 0;
        lineEnded = false;
        placed = unitPos > bp ? 1 : 0;
        for(let i = bp; i < unitPos; i++){
            if (!isWrapSpace(text[i])) realSeen = true;
            else if (text[i] === " " && condense > 0 && (realSeen || !hancom)) spaceW += rangeW(i, i + 1) - (spAt?.[i] ?? 0);
        }
        lineW = rangeW(bp, unitPos) + w;
    };
    const breakAt = (at, leadW)=>{
        starts.push(at);
        avail = lineWidth ? lineWidth(starts.length - 1) : contWidth;
        spaceW = 0;
        realSeen = false;
        lineEnded = false;
        placed = 0;
        lineW = leadW;
    };
    for (const u of units){
        if (isWrapSpace(u[0])) {
            if (hancom) {
                let k = 0;
                for(; k < u.length; k++){
                    if (!lineEnded && lineW >= avail) lineEnded = true;
                    if (lineEnded && (k >= 2 || u[k] === "\u2005") && pos + k > lineStart()) break;
                    if (!lineEnded) {
                        const sw = rangeW(pos + k, pos + k + 1);
                        lineW += sw;
                        if (realSeen && u[k] === " ") spaceW += sw - (spAt?.[pos + k] ?? 0);
                    }
                }
                if (k < u.length) breakAt(pos + k, rangeW(pos + k, pos + u.length));
                pos += u.length;
                continue;
            }
            const sw = rangeW(pos, pos + u.length);
            lineW += sw;
            for(let k = 0; k < u.length; k++)if (u[k] === " ") spaceW += rangeW(pos + k, pos + k + 1);
            pos += u.length;
            continue;
        }
        if (u === "\ufdd0") {
            placed++;
            pos += u.length;
            continue;
        }
        if (u === "\u3000" && !empty() && lineW + rangeW(pos, pos + 1) - lastSp(pos, pos + 1) > cap()) {
            lineW += rangeW(pos, pos + 1);
            placed++;
            pos += u.length;
            continue;
        }
        if (lineEnded) breakAt(pos, 0);
        const w = rangeW(pos, pos + u.length);
        if (lineW + w - lastSp(pos, pos + u.length) <= cap()) {
            lineW += w;
            realSeen = true;
            placed++;
            pos += u.length;
            continue;
        }
        if (empty() || w > (lineWidth ? lineWidth(starts.length) : contWidth) + EPS) {
            if ((mode === "keep" || /^\u2060*[\x21-\x7e\u00a1-\u024f\u2018-\u201f]{2,}/.test(u)) && !empty()) breakBefore(pos, 0);
            let sub = 0;
            for (const ch of u){
                const c = charWAt(pos + sub, ch);
                if (lineW + c - lastSp(pos + sub, pos + sub + ch.length) > cap() && !empty() && ch !== "\u3000" && ch !== "\ufdd0") breakBefore(pos + sub, 0, true);
                lineW += c;
                realSeen = true;
                placed++;
                sub += ch.length;
            }
            pos += u.length;
            continue;
        }
        breakBefore(pos, w);
        placed++;
        pos += u.length;
    }
    return {
        lines: starts.length,
        starts,
        lastLineWidth: lineW
    };
}
export function simulateWrapKeepWord(text, firstWidth, contWidth, height, ratioPct, opts) {
    return simulateWrap(text, firstWidth, contWidth, height, ratioPct, "keep", opts);
}
export function fitRatioForFewerLines(text, firstWidth, contWidth, height, baseRatio, minRatio, opts) {
    const base = simulateWrap(text, firstWidth, contWidth, height, baseRatio, "keep", opts);
    if (base.lines < 2) return null;
    for(let r = baseRatio - 1; r >= minRatio; r--){
        const sim = simulateWrap(text, firstWidth, contWidth, height, r, "keep", opts);
        if (sim.lines < base.lines) return r;
    }
    return null;
}
