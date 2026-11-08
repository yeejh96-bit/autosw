// Derived from kordoc (MIT), Copyright 2026 chrisryugj. See ../LICENSE or package-root LICENSE and PROVENANCE.json.
import { wingdingsChar } from "./symbol-fonts.mjs";
import { circledNumber } from "./numbering.mjs";
const BMP_SYMBOL_MAP = {
    0x6c: "●",
    0x6d: "●",
    0x6e: "■",
    0x6f: "□",
    0x70: "□",
    0x71: "□",
    0x72: "□",
    0x73: "⬧",
    0x74: "⧫",
    0x75: "◆",
    0x76: "❖",
    0x77: "⬥",
    0x9e: "·",
    0x9f: "•",
    0xa0: "·",
    0xa1: "⚪",
    0xa2: "○",
    0xa3: "○",
    0xa4: "◉",
    0xa5: "◎",
    0xa7: "▪",
    0xa8: "◻",
    0xaa: "✦",
    0xab: "★",
    0xac: "✶",
    0xad: "✴",
    0xae: "✹",
    0x45: "☜",
    0x46: "☞",
    0x47: "☝",
    0x48: "☟",
    0xfb: "✗",
    0xfc: "✔",
    0xfd: "☒",
    0xfe: "☑",
    0xe8: "➔",
    0xef: "⇦",
    0xf0: "⇨",
    0xf1: "⇧",
    0xf2: "⇩",
    0x22: "✂",
    0x36: "⌛",
    0x4a: "☺",
    0x4e: "☠",
    0x52: "☼",
    0x54: "❄",
    0x58: "✠",
    0x59: "✡"
};
const SUPPLEMENTARY_MAP = {
    0xf003b: "↓",
    0xf02ef: "·",
    0xf0854: "《",
    0xf0855: "》",
    0xf00da: "[예]",
    0xf080f: "━",
    0xf0827: "■",
    0xf03c5: "□",
    0xf012b: "(인)",
    0xf00e1: "(인)",
    0xf02fc: "►",
    0xf031c: "■",
    0xf03a0: "↵",
    0xf03ef: "한",
    0xf03f0: "글",
    0xf03f1: "과",
    0xf03f2: "컴",
    0xf03f3: "퓨",
    0xf03f4: "터",
    0xf0090: "✺",
    0xf02ec: "◇",
    0xf02fb: "▸",
    0xf03a7: "⊟",
    0xf03a8: "⊞",
    0xf03da: "□",
    0xf03ff: "□",
    0xf0806: "┌",
    0xf0807: "┬",
    0xf0808: "┐",
    0xf080c: "└",
    0xf080e: "┘",
    0xf0810: "│",
    0xf081c: "┈",
    0xf0832: "═",
    0xf0848: "━"
};
export function enclosedNumber(n) {
    return n === 0 ? "⓪" : circledNumber(n - 1);
}
function tensDigit(code) {
    if (code >= 0xf0288 && code <= 0xf0291) return code - 0xf0288;
    if (code >= 0xf02ba && code <= 0xf02c2) return code - 0xf02b9;
    return undefined;
}
function unitsDigit(code) {
    if (code >= 0xf0292 && code <= 0xf029b) return code - 0xf0292;
    if (code >= 0xf02c3 && code <= 0xf02cc) return code - 0xf02c3;
    return undefined;
}
function numberGlyph(code) {
    const t = tensDigit(code);
    if (t !== undefined) return enclosedNumber(t);
    if (code >= 0xf02b0 && code <= 0xf02b9) return enclosedNumber(code - 0xf02b0);
    const u = unitsDigit(code);
    return u === undefined ? undefined : String(u);
}
export function composeDigits(text) {
    let out = "";
    for (const c of text){
        const code = c.codePointAt(0);
        const d = code >= 0x2460 && code <= 0x2473 ? code - 0x245f : code === 0x24ea ? 0 : code >= 0xf02b0 && code <= 0xf02b9 ? code - 0xf02b0 : tensDigit(code) ?? unitsDigit(code);
        if (d === undefined) return text;
        out += String(d);
    }
    return out || text;
}
export function pairedNumber(a, b) {
    const t = tensDigit(a.codePointAt(0) ?? 0), u = unitsDigit(b.codePointAt(0) ?? 0);
    return t !== undefined && u !== undefined ? enclosedNumber(t * 10 + u) : undefined;
}
export function composeText(chars, enclosed) {
    return enclosed && /^\d{1,2}$/.test(chars) ? enclosedNumber(Number(chars)) : chars;
}
function mapPuaChar(code) {
    if (code >= 0xf020 && code <= 0xf0ff) {
        return BMP_SYMBOL_MAP[code - 0xf000] ?? wingdingsChar(code - 0xf000);
    }
    if (code >= 0xf0000 && code <= 0xf09ff) {
        return numberGlyph(code) ?? SUPPLEMENTARY_MAP[code];
    }
    return undefined;
}
export function mapPuaText(text) {
    let out = "";
    const chars = Array.from(text);
    for(let i = 0; i < chars.length; i++){
        const pair = i + 1 < chars.length ? pairedNumber(chars[i], chars[i + 1]) : undefined;
        if (pair !== undefined) {
            out += pair;
            i++;
            continue;
        }
        out += mapPuaChar(chars[i].codePointAt(0)) ?? chars[i];
    }
    return out;
}
