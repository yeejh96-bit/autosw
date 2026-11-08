// Derived from kordoc (MIT), Copyright 2026 chrisryugj. See ../LICENSE or package-root LICENSE and PROVENANCE.json.
const HANGUL_INITIALS = [
    0,
    2,
    3,
    5,
    6,
    7,
    9,
    11,
    12,
    14,
    15,
    16,
    17,
    18
];
const HANGUL_MEDIALS = [
    0,
    4,
    8,
    13,
    18,
    20
];
export function hangulOrdinal(n) {
    const cols = HANGUL_INITIALS.length;
    const vowel = HANGUL_MEDIALS[Math.min(Math.floor(n / cols), HANGUL_MEDIALS.length - 1)];
    const init = HANGUL_INITIALS[n % cols];
    return String.fromCodePoint(0xac00 + init * 588 + vowel * 28);
}
export function circledNumber(n) {
    if (n < 20) return String.fromCodePoint(0x2460 + n);
    if (n < 35) return String.fromCodePoint(0x3251 + (n - 20));
    if (n < 50) return String.fromCodePoint(0x32b1 + (n - 35));
    return `(${n + 1})`;
}
export function circledHangul(n) {
    return n < 14 ? String.fromCodePoint(0x326e + n) : hangulOrdinal(n);
}
const AMOUNT_DIGITS = "영일이삼사오육칠팔구";
const AMOUNT_SMALL = [
    "",
    "십",
    "백",
    "천"
];
const AMOUNT_BIG = [
    "",
    "만",
    "억",
    "조",
    "경",
    "해"
];
export function hangulAmount(n) {
    const digits = String(n).replace(/\D/g, "").replace(/^0+(?=\d)/, "");
    if (!digits || digits === "0") return "영";
    const groups = [];
    for(let end = digits.length; end > 0; end -= 4)groups.unshift(digits.slice(Math.max(0, end - 4), end));
    let out = "";
    groups.forEach((g, i)=>{
        let part = "";
        const padded = g.padStart(4, "0");
        for(let k = 0; k < 4; k++){
            const d = Number(padded[k]);
            if (d) part += AMOUNT_DIGITS[d] + AMOUNT_SMALL[3 - k];
        }
        if (part) out += part + (AMOUNT_BIG[groups.length - 1 - i] ?? "");
    });
    return out;
}
export function romanNumeral(n, upper) {
    if (n <= 0 || n > 3999) return String(n);
    const values = [
        1000,
        900,
        500,
        400,
        100,
        90,
        50,
        40,
        10,
        9,
        5,
        4,
        1
    ];
    const symbols = upper ? [
        "M",
        "CM",
        "D",
        "CD",
        "C",
        "XC",
        "L",
        "XL",
        "X",
        "IX",
        "V",
        "IV",
        "I"
    ] : [
        "m",
        "cm",
        "d",
        "cd",
        "c",
        "xc",
        "l",
        "xl",
        "x",
        "ix",
        "v",
        "iv",
        "i"
    ];
    let result = "";
    let num = n;
    for(let i = 0; i < values.length; i++){
        while(num >= values[i]){
            result += symbols[i];
            num -= values[i];
        }
    }
    return result;
}
