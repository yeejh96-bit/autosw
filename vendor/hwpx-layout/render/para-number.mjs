// Derived from kordoc (MIT), Copyright 2026 chrisryugj. See ../LICENSE or package-root LICENSE and PROVENANCE.json.
const HANGUL_SYLLABLE = "가나다라마바사아자차카타파하";
const HANGUL_JAMO = "ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎ";
const IDEOGRAPH = "一二三四五六七八九十";
function roman(n) {
    const t = [
        [
            1000,
            "M"
        ],
        [
            900,
            "CM"
        ],
        [
            500,
            "D"
        ],
        [
            400,
            "CD"
        ],
        [
            100,
            "C"
        ],
        [
            90,
            "XC"
        ],
        [
            50,
            "L"
        ],
        [
            40,
            "XL"
        ],
        [
            10,
            "X"
        ],
        [
            9,
            "IX"
        ],
        [
            5,
            "V"
        ],
        [
            4,
            "IV"
        ],
        [
            1,
            "I"
        ]
    ];
    let s = "";
    for (const [v, r] of t)while(n >= v){
        s += r;
        n -= v;
    }
    return s;
}
export function formatHeadNumber(n, format) {
    const cyc = (set)=>set[(n - 1) % set.length];
    switch(format){
        case "HANGUL_SYLLABLE":
            return cyc(HANGUL_SYLLABLE);
        case "HANGUL_JAMO":
            return cyc(HANGUL_JAMO);
        case "CIRCLED_DIGIT":
            return n >= 1 && n <= 20 ? String.fromCodePoint(0x245f + n) : String(n);
        case "CIRCLED_HANGUL_SYLLABLE":
            return String.fromCodePoint(0x326e + (n - 1) % 14);
        case "CIRCLED_HANGUL_JAMO":
            return String.fromCodePoint(0x3260 + (n - 1) % 14);
        case "LATIN_SMALL":
            return String.fromCharCode(0x61 + (n - 1) % 26);
        case "LATIN_CAPITAL":
            return String.fromCharCode(0x41 + (n - 1) % 26);
        case "ROMAN_SMALL":
            return roman(n).toLowerCase();
        case "ROMAN_CAPITAL":
            return roman(n);
        case "IDEOGRAPH":
            return n >= 1 && n <= 10 ? IDEOGRAPH[n - 1] : String(n);
        default:
            return String(n);
    }
}
export function nextParaNumber(counters, geom) {
    const num = geom?.number;
    if (!num) return "";
    const cur = counters.get(num.id) ?? [];
    const head = num.heads.get(num.level);
    cur[num.level] = (cur[num.level] ?? (head?.start ?? 1) - 1) + 1;
    cur.length = num.level + 1;
    counters.set(num.id, cur);
    return (head?.text ?? "").replace(/\^(\d)?/g, (_m, d)=>{
        if (d === undefined) return "";
        const lv = Number(d), h = num.heads.get(lv);
        return formatHeadNumber(cur[lv] ?? h?.start ?? 1, h?.format ?? "DIGIT");
    });
}
