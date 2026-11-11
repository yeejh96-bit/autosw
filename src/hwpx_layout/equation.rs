//! Derived from kordoc (MIT), Copyright 2026 chrisryugj; see data/kordoc-LICENSE.
//! HML conversion derives from hml-equation-parser (Apache-2.0),
//! Copyright 2018 Open Bapul. Full Apache text, provenance and modifications are
//! retained in data/equation-data.json; see also data/kordoc-NOTICE.
//! Conversion runs entirely in Rust. Intermediate strings retain UTF-16 units,
//! including unpaired surrogates produced by the original string-index rules.
use crate::input::is_space;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use std::sync::LazyLock;

static DATA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("data/equation-data.json")).expect("fixed equation maps")
});
type Text = Vec<u16>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquationText {
    pub units: Vec<u16>,
}
impl EquationText {
    /// Strict text is optional: JS conversion can produce an unpaired surrogate
    /// even from valid input. Callers must retain units between conversions.
    pub fn text(&self) -> Option<String> {
        String::from_utf16(&self.units).ok()
    }
    /// Use only at the final UTF-8 writing boundary where Node Buffer.from(str)
    /// replaces unpaired surrogates. Never use this for intermediate conversion.
    pub fn utf8_at_write_boundary(&self) -> Vec<u8> {
        String::from_utf16_lossy(&self.units).into_bytes()
    }
    #[cfg(test)]
    fn report(&self) -> Value {
        json!({"text":self.text(),"utf16":self.units,"utf8Hex":data_encoding::HEXLOWER.encode(&self.utf8_at_write_boundary())})
    }
}
fn units(s: &str) -> Text {
    s.encode_utf16().collect()
}
fn ws(c: u16) -> bool {
    char::from_u32(u32::from(c)).is_some_and(is_space)
}
fn alpha(c: u16) -> bool {
    matches!(c,65..=90|97..=122)
}
fn alnum(c: u16) -> bool {
    alpha(c) || matches!(c, 48..=57)
}
fn word(c: u16) -> bool {
    alnum(c) || c == 95
}
fn trim(s: &[u16]) -> &[u16] {
    let start = s.iter().position(|&c| !ws(c)).unwrap_or(s.len());
    let end = s.iter().rposition(|&c| !ws(c)).map_or(start, |i| i + 1);
    &s[start..end]
}
fn collapse(s: &[u16]) -> Text {
    let mut out = Text::new();
    for &c in trim(s) {
        if ws(c) {
            if out.last() != Some(&32) {
                out.push(32);
            }
        } else {
            out.push(c);
        }
    }
    out
}
fn find(s: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    s.get(from..)?
        .windows(needle.len())
        .position(|part| part == needle)
        .map(|i| from + i)
}
fn replace(s: &[u16], from: &str, to: &str) -> Text {
    let needle = units(from);
    let replacement = units(to);
    let mut out = Text::new();
    let mut pos = 0;
    while let Some(at) = find(s, &needle, pos) {
        out.extend_from_slice(&s[pos..at]);
        out.extend_from_slice(&replacement);
        pos = at + needle.len();
    }
    out.extend_from_slice(&s[pos..]);
    out
}
fn lookup(table: &str, token: &[u16]) -> Option<&'static str> {
    DATA["maps"][table]
        .get(String::from_utf16(token).ok()?)?
        .as_str()
}
fn table(name: &str) -> &'static serde_json::Map<String, Value> {
    DATA["maps"][name]
        .as_object()
        .expect("fixed equation map object")
}
fn brackets(s: &[u16], start: usize) -> Option<(usize, usize)> {
    let begin = find(s, &[123], start)?;
    let mut depth = 1usize;
    for (at, &c) in s.iter().enumerate().skip(begin + 1) {
        if c == 123 {
            depth += 1;
        } else if c == 125 {
            depth -= 1;
        }
        if depth == 0 {
            return Some((begin, at + 1));
        }
    }
    None
}
fn enclosing(s: &[u16], start: usize) -> Option<(usize, usize)> {
    let mut depth = 0;
    for at in (0..start).rev() {
        match s[at] {
            125 => depth += 1,
            123 => {
                if depth > 0 {
                    depth -= 1;
                    continue;
                }
                return brackets(s, at).filter(|&(a, b)| a == at && b > start);
            }
            _ => {}
        }
    }
    None
}
fn mask(s: &[u16]) -> Text {
    let mut out = s.to_vec();
    let mut pos = 0;
    while let Some(start) = find(&out, &[34], pos) {
        let Some(end) = find(&out, &[34], start + 1) else {
            break;
        };
        out[start..=end].fill(0xffff);
        pos = end + 1;
    }
    let needle = units("\\text{");
    pos = 0;
    while let Some(start) = find(&out, &needle, pos) {
        let Some(end) = find(&out, &[125], start + needle.len()) else {
            break;
        };
        out[start..=end].fill(0xffff);
        pos = end + 1;
    }
    out
}
fn keyword(s: &[u16], key: &str, from: usize) -> Option<usize> {
    let masked = mask(s);
    let key = units(key);
    let mut pos = from;
    while let Some(at) = find(&masked, &key, pos) {
        if (at == 0 || ws(masked[at - 1]))
            && (at + key.len() == masked.len() || ws(masked[at + key.len()]))
        {
            return Some(at);
        }
        pos = at + 1;
    }
    None
}
fn fraction(s: &[u16]) -> Text {
    let masked = mask(s);
    // JS's out is an array, so an inserted "\\frac" is ONE element even
    // though it contains five UTF-16 units. Preserve its splice/index behavior.
    let mut out: Vec<Text> = Vec::new();
    let mut pos = 0;
    let mut search = 0;
    let key = units("over");
    while let Some(cursor) = find(&masked, &key, search) {
        search = cursor + 1;
        if !(cursor == 0 || ws(masked[cursor - 1]))
            || !(cursor + 4 == masked.len() || ws(masked[cursor + 4]))
        {
            continue;
        }
        while pos < cursor {
            out.push(vec![s[pos]]);
            pos += 1;
        }
        let mut end = out.len();
        while end > 0 && out[end - 1].iter().any(|&c| ws(c)) {
            end -= 1;
        }
        let mut start = end;
        if end > 0 && out[end - 1] == [125] {
            let mut depth = 0;
            let mut begin = None;
            for at in (0..end).rev() {
                if out[at] == [125] {
                    depth += 1;
                } else if out[at] == [123] {
                    depth -= 1;
                    if depth == 0 {
                        begin = Some(at);
                        break;
                    }
                }
            }
            let Some(begin) = begin else {
                break;
            };
            out.truncate(end);
            out.insert(begin, units("\\frac"));
        } else {
            while start > 0 && !out[start - 1].iter().any(|&c| ws(c)) {
                start -= 1;
            }
            if start == end {
                break;
            }
            out.truncate(end);
            out.insert(start, units("\\frac"));
            out.insert(start + 1, vec![123]);
            out.push(vec![125]);
        }
        pos = cursor + 4;
    }
    out.into_iter()
        .flatten()
        .chain(s[pos..].iter().copied())
        .collect()
}
fn root_of(mut s: Text) -> Text {
    while let Some(cursor) = keyword(&s, "root", 0) {
        let Some((a, b)) = brackets(&s, cursor) else {
            break;
        };
        let Some(of) = keyword(&s, "of", b) else {
            break;
        };
        let Some((c, d)) = brackets(&s, of) else {
            break;
        };
        let mut out = s[..cursor].to_vec();
        out.extend(units("\\sqrt["));
        out.extend_from_slice(&s[a + 1..b - 1]);
        out.extend(units("]{"));
        out.extend_from_slice(&s[c + 1..d - 1]);
        out.push(125);
        // Deliberately preserve the source's d+1, including discarding one UTF-16
        // unit immediately after the second closing brace.
        out.extend_from_slice(&s[d.saturating_add(1).min(s.len())..]);
        s = out;
    }
    s
}
fn structural(mut s: Text, kind: &str) -> Text {
    for (key, value) in table(kind) {
        let key = units(key);
        while let Some(cursor) = find(&s, &key, 0) {
            let Some((a, b)) = brackets(&s, cursor) else {
                break;
            };
            let (start, end, body) = if kind == "BRACE_CONVERT_MAP" {
                let Some((c, d)) = brackets(&s, b) else {
                    break;
                };
                let mut body = units(value.as_str().unwrap());
                body.extend_from_slice(&s[a..b]);
                body.push(94);
                body.extend_from_slice(&s[c..d]);
                (cursor, d, body)
            } else if kind == "MATRIX_CONVERT_MAP" {
                let element = replace(&replace(&s[a + 1..b - 1], "#", " \\\\ "), "&amp;", "&");
                let outer = if value["removeOutterBrackets"] == true {
                    enclosing(&s, cursor)
                } else {
                    None
                };
                let (start, end) = outer.filter(|&(_, end)| end >= b).unwrap_or((cursor, b));
                let mut body = units(value["begin"].as_str().unwrap());
                body.extend(element);
                body.extend(units(value["end"].as_str().unwrap()));
                (start, end, body)
            } else {
                let (start, end) = enclosing(&s, cursor)
                    .filter(|&(_, end)| end >= b)
                    .unwrap_or((cursor, b));
                let mut body = units(value.as_str().unwrap());
                body.extend_from_slice(&s[a..b]);
                (start, end, body)
            };
            s.splice(start..end, body);
        }
    }
    s
}

pub fn hml_to_latex_utf16(input: &[u16]) -> Result<EquationText, String> {
    let mut spaced = Text::new();
    let mut pos = 0;
    while pos < input.len() {
        let c = input[pos];
        pos += 1;
        if matches!(c, 9 | 10 | 13) {
            while pos < input.len() && matches!(input[pos], 9 | 10 | 13) {
                pos += 1;
            }
            spaced.push(32);
        } else if c == 96 {
            spaced.push(32);
        } else if matches!(c, 123 | 125 | 38) {
            spaced.extend([32, c, 32]);
        } else {
            spaced.push(c);
        }
    }
    let mut tokens: Vec<Text> = spaced
        .split(|&c| c == 32)
        .map(|token| {
            if let Some(value) =
                lookup("CONVERT_MAP", token).or_else(|| lookup("MIDDLE_CONVERT_MAP", token))
            {
                return units(value);
            }
            if token.len() > 2
                && token.first() == Some(&34)
                && token.last() == Some(&34)
                && !token[1..token.len() - 1]
                    .iter()
                    .any(|&c| matches!(c, 10 | 13 | 0x2028 | 0x2029))
            {
                let mut out = units("\\text{");
                out.extend_from_slice(&token[1..token.len() - 1]);
                out.push(125);
                return out;
            }
            token.to_vec()
        })
        .filter(|t| !t.is_empty())
        .collect();
    for i in 1..tokens.len() {
        if tokens[i] == [123] && tokens[i - 1] == units("\\left") {
            tokens[i] = units("\\{");
        }
        if tokens[i] == [125] && tokens[i - 1] == units("\\right") {
            tokens[i] = units("\\}");
        }
    }
    let joined = tokens.join(&32);
    let out = fraction(&joined);
    let out = root_of(out);
    let out = structural(out, "MATRIX_CONVERT_MAP");
    let out = structural(out, "BAR_CONVERT_MAP");
    Ok(EquationText {
        units: structural(out, "BRACE_CONVERT_MAP"),
    })
}
pub fn hml_to_latex(input: &str) -> Result<EquationText, String> {
    hml_to_latex_utf16(&units(input))
}

fn strip_script_space(input: &[u16], mark: u16) -> Text {
    let mut out = Text::new();
    let mut pos = 0;
    while pos < input.len() {
        let c = input[pos];
        pos += 1;
        if c == mark {
            while out.last().is_some_and(|&c| ws(c)) {
                out.pop();
            }
            out.push(c);
            while pos < input.len() && ws(input[pos]) {
                pos += 1;
            }
        } else {
            out.push(c);
        }
    }
    out
}
fn wrap_script(input: &[u16], mark: u16) -> Text {
    let mut out = Text::new();
    let mut pos = 0;
    let eligible = |c| !ws(c) && !matches!(c, 123 | 125 | 95 | 94);
    while pos < input.len() {
        let c = input[pos];
        out.push(c);
        pos += 1;
        if c == mark && input.get(pos).is_some_and(|&c| eligible(c)) {
            out.push(123);
            while pos < input.len() && eligible(input[pos]) {
                out.push(input[pos]);
                pos += 1;
            }
            out.push(125);
        }
    }
    out
}
fn normalize_words(input: &[u16]) -> Text {
    let mut out = Text::new();
    let mut pos = 0;
    while pos < input.len() {
        if alpha(input[pos]) && (pos == 0 || (input[pos - 1] != 92 && !alnum(input[pos - 1]))) {
            let start = pos;
            while pos < input.len() && alnum(input[pos]) {
                pos += 1;
            }
            let original = &input[start..pos];
            let lower: Text = original
                .iter()
                .map(|&c| if matches!(c, 65..=90) { c + 32 } else { c })
                .collect();
            if lookup("CONVERT_MAP", original).is_some()
                || lookup("MIDDLE_CONVERT_MAP", original).is_some()
            {
                out.extend_from_slice(original);
            } else if ["over", "root", "of"]
                .iter()
                .any(|word| lower == units(word))
            {
                out.extend(lower);
            } else if let Some(alias) = lookup("WORD_ALIASES", &lower) {
                out.extend(units(alias));
            } else if lower != original
                && (lookup("CONVERT_MAP", &lower).is_some()
                    || lookup("MIDDLE_CONVERT_MAP", &lower).is_some())
            {
                out.extend(lower);
            } else {
                out.extend_from_slice(original);
            }
        } else {
            out.push(input[pos]);
            pos += 1;
        }
    }
    out
}
fn root_argument(input: &[u16]) -> Text {
    let mut out = Text::new();
    let mut pos = 0;
    let root = units("root");
    let of = units("of");
    let equal = |a: &[u16], b: &[u16]| {
        a.len() == b.len()
            && a.iter().zip(b).all(|(&a, &b)| {
                if matches!(a, 65..=90) {
                    a + 32 == b
                } else {
                    a == b
                }
            })
    };
    while pos < input.len() {
        let mut matched = None;
        if (pos == 0 || !word(input[pos - 1]))
            && input.get(pos..pos + 4).is_some_and(|s| equal(s, &root))
        {
            let mut at = pos + 4;
            let whitespace = at;
            while at < input.len() && ws(input[at]) {
                at += 1;
            }
            if at > whitespace && input.get(at).is_some_and(|&c| c != 123) {
                let start = at;
                while at < input.len() && !ws(input[at]) {
                    at += 1;
                }
                let end = at;
                while at < input.len() && ws(input[at]) {
                    at += 1;
                }
                if end > start
                    && at > end
                    && input.get(at..at + 2).is_some_and(|s| equal(s, &of))
                    && input.get(at + 2).is_none_or(|&c| !alnum(c))
                {
                    matched = Some((start, end, at + 2));
                }
            }
        }
        if let Some((a, b, end)) = matched {
            out.extend(units("root { "));
            out.extend_from_slice(&input[a..b]);
            out.extend(units(" } of"));
            pos = end;
        } else {
            out.push(input[pos]);
            pos += 1;
        }
    }
    out
}
pub fn hwp_equation_to_latex_utf16(input: &[u16]) -> Result<EquationText, String> {
    let clean: Text = input.iter().copied().filter(|&c| c != 0).collect();
    let mut s = collapse(&clean);
    s = strip_script_space(&strip_script_space(&s, 94), 95);
    s = wrap_script(&wrap_script(&s, 94), 95);
    let mut spaced = Text::new();
    for c in s {
        if matches!(c, 94 | 95) {
            spaced.extend([32, c, 32]);
        } else {
            spaced.push(c);
        }
    }
    s = spaced;
    for (from, to) in [
        ("+-", " \\pm "),
        ("-+", " \\mp "),
        ("//", " \\parallel "),
        ("!=", " \\neq "),
        ("<=", " \\leq "),
        (">=", " \\geq "),
        ("==", " \\equiv "),
        ("△", " \\triangle "),
        ("□", " \\square "),
        ("‧", " \\cdot "),
    ] {
        s = replace(&s, from, to);
    }
    s = normalize_words(&s);
    s = root_argument(&s);
    Ok(EquationText {
        units: collapse(&hml_to_latex_utf16(&s)?.units),
    })
}
pub fn hwp_equation_to_latex(input: &str) -> Result<EquationText, String> {
    hwp_equation_to_latex_utf16(&units(input))
}

fn wrap(v: &[u16]) -> Text {
    let simple = !v.is_empty() && v.iter().all(|&c| word(c) || matches!(c, 46 | 0x2032));
    let parens = v.len() >= 2
        && v[0] == 40
        && v[v.len() - 1] == 41
        && !v[1..v.len() - 1]
            .iter()
            .any(|&c| matches!(c, 10 | 13 | 0x2028 | 0x2029));
    if v.len() > 1 && !simple && !parens {
        [vec![40], v.to_vec(), vec![41]].concat()
    } else {
        v.to_vec()
    }
}
fn scalars(v: &[u16]) -> Vec<&[u16]> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < v.len() {
        let length = if (0xd800..=0xdbff).contains(&v[pos])
            && v.get(pos + 1)
                .is_some_and(|c| (0xdc00..=0xdfff).contains(c))
        {
            2
        } else {
            1
        };
        out.push(&v[pos..pos + length]);
        pos += length;
    }
    out
}
fn script(v: &[u16], map: &str, mark: &str) -> Text {
    let v = trim(v);
    let chars = scalars(v);
    let mapped: Option<Vec<_>> = chars
        .iter()
        .map(|c| lookup(map, c).filter(|s| !s.is_empty()))
        .collect();
    if let Some(mapped) = mapped {
        mapped.into_iter().flat_map(str::encode_utf16).collect()
    } else {
        [units(mark), wrap(v)].concat()
    }
}
struct Plain<'a> {
    s: &'a [u16],
    pos: usize,
}
impl Plain<'_> {
    fn skip(&mut self) {
        while self.s.get(self.pos) == Some(&32) {
            self.pos += 1;
        }
    }
    fn take(&mut self) -> Text {
        let c = self.s.get(self.pos).copied();
        self.pos += 1;
        c.into_iter().collect()
    }
    fn seq(&mut self, end: u16, depth: usize) -> Result<Text, String> {
        if depth > 512 {
            return Err("수식 중첩 처리 한도를 넘었습니다.".into());
        }
        let mut out = Text::new();
        while self.pos < self.s.len() && self.s[self.pos] != end {
            if self.s[self.pos] == 32 {
                self.pos += 1;
                continue;
            }
            out.extend(self.atom(depth + 1)?);
        }
        Ok(out)
    }
    fn arg(&mut self, depth: usize) -> Result<Text, String> {
        self.skip();
        if self.s.get(self.pos) == Some(&123) {
            self.pos += 1;
            let out = self.seq(125, depth + 1)?;
            self.pos += 1;
            Ok(out)
        } else {
            self.atom(depth + 1)
        }
    }
    fn atom(&mut self, depth: usize) -> Result<Text, String> {
        if depth > 512 {
            return Err("수식 중첩 처리 한도를 넘었습니다.".into());
        }
        let Some(&c) = self.s.get(self.pos) else {
            return Ok(vec![]);
        };
        self.pos += 1;
        if c == 92 {
            let name = if self.s.get(self.pos).is_some_and(|&c| alpha(c)) {
                let start = self.pos;
                while self.s.get(self.pos).is_some_and(|&c| alpha(c)) {
                    self.pos += 1;
                }
                self.s[start..self.pos].to_vec()
            } else {
                self.take()
            };
            match String::from_utf16(&name).ok().as_deref() {
                Some("frac") => {
                    let a = self.arg(depth + 1)?;
                    let b = self.arg(depth + 1)?;
                    return Ok([wrap(trim(&a)), vec![47], wrap(trim(&b))].concat());
                }
                Some("sqrt") => {
                    self.skip();
                    let mut root = Text::new();
                    if self.s.get(self.pos) == Some(&91) {
                        self.pos += 1;
                        root = self.seq(93, depth + 1)?;
                        self.pos += 1;
                    }
                    let a = self.arg(depth + 1)?;
                    let mut out = if root.is_empty() {
                        vec![]
                    } else {
                        script(&root, "SUP", "")
                    };
                    out.push(0x221a);
                    out.extend(wrap(trim(&a)));
                    return Ok(out);
                }
                Some("left" | "right" | "big" | "Big" | "bigg" | "Bigg") => {
                    self.skip();
                    let d = if self.s.get(self.pos) == Some(&92) {
                        self.atom(depth + 1)?
                    } else {
                        self.take()
                    };
                    return Ok(if d == [46] { vec![] } else { d });
                }
                Some(accent @ ("overline" | "bar" | "hat" | "vec" | "dot" | "tilde")) => {
                    let a = self.arg(depth + 1)?;
                    let mark = match accent {
                        "overline" => 0x305,
                        "bar" => 0x304,
                        "hat" => 0x302,
                        "vec" => 0x20d7,
                        "dot" => 0x307,
                        _ => 0x303,
                    };
                    let mut out = Text::new();
                    for scalar in scalars(trim(&a)) {
                        out.extend_from_slice(scalar);
                        out.push(mark);
                    }
                    return Ok(out);
                }
                Some("rm" | "it") => return Ok(vec![]),
                Some("mathrm" | "text" | "mathbf" | "mathit" | "operatorname") => {
                    return self.arg(depth + 1);
                }
                _ => return Ok(lookup("SYMBOLS", &name).map_or(name.clone(), units)),
            }
        }
        match c {
            123 => {
                let out = self.seq(125, depth + 1)?;
                self.pos += 1;
                Ok(out)
            }
            94 => Ok(script(&self.arg(depth + 1)?, "SUP", "^")),
            95 => Ok(script(&self.arg(depth + 1)?, "SUB", "_")),
            96 | 126 => Ok(vec![32]),
            _ => Ok(vec![c]),
        }
    }
}
pub fn latex_to_plain_utf16(input: &[u16]) -> Result<EquationText, String> {
    let mut parser = Plain { s: input, pos: 0 };
    Ok(EquationText {
        units: collapse(&parser.seq(0, 0)?),
    })
}
pub fn latex_to_plain(input: &str) -> Result<EquationText, String> {
    latex_to_plain_utf16(&units(input))
}

#[cfg(test)]
pub(super) fn run(op: &str, input: &Value) -> Result<Value, String> {
    if op == "batch" {
        let cases = input["cases"]
            .as_array()
            .ok_or("cases 배열이 필요합니다.")?;
        return Ok(Value::Array(
            cases
                .iter()
                .map(|case| {
                    let result = case["operation"]
                        .as_str()
                        .ok_or_else(|| "수식 연산 이름이 필요합니다.".to_string())
                        .and_then(|operation| {
                            if operation == "batch" {
                                Err("중첩 batch는 지원하지 않습니다.".into())
                            } else {
                                run(operation, &case["input"])
                            }
                        });
                    match result {
                        Ok(value) => json!({"ok":true,"value":value}),
                        Err(error) => json!({"ok":false,"error":error}),
                    }
                })
                .collect(),
        ));
    }
    if op == "maps" {
        return Ok(DATA["maps"].clone());
    }
    let input = if let Some(units) = input.get("utf16") {
        units
            .as_array()
            .ok_or("utf16 배열이 필요합니다.")?
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or_else(|| "잘못된 UTF-16 단위입니다.".to_string())
            })
            .collect::<Result<Text, _>>()?
    } else {
        units(input["text"].as_str().ok_or("text 문자열이 필요합니다.")?)
    };
    match op {
        "hmlToLatex" => Ok(hml_to_latex_utf16(&input)?.report()),
        "hwpEquationToLatex" => Ok(hwp_equation_to_latex_utf16(&input)?.report()),
        "latexToPlain" => Ok(latex_to_plain_utf16(&input)?.report()),
        "chain" => {
            let latex = hwp_equation_to_latex_utf16(&input)?;
            let plain = latex_to_plain_utf16(&latex.units)?;
            Ok(json!({"latex":latex.report(),"plain":plain.report()}))
        }
        _ => Err("알 수 없는 수식 연산입니다.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_order_and_utf16_are_preserved() {
        assert_eq!(
            hml_to_latex("a over {b}").unwrap().text().unwrap(),
            "\\frac{a} { b }"
        );
        assert_eq!(
            latex_to_plain("\\frac{a+b}{2} + x^{2}")
                .unwrap()
                .text()
                .unwrap(),
            "(a+b)/2+x²"
        );
        let result = latex_to_plain("\\bar😀").unwrap();
        assert_eq!(result.units, [0xd83d, 0x304, 0xde00]);
        assert_eq!(result.text(), None);
        assert_eq!(
            String::from_utf8(result.utf8_at_write_boundary()).unwrap(),
            "�̄�"
        );
    }
    #[test]
    fn malformed_brackets_follow_original_partial_conversion() {
        assert_eq!(
            hml_to_latex("matrix { a # b").unwrap().text().unwrap(),
            "HULKMATRIX { a # b"
        );
        assert_eq!(
            latex_to_plain("\\frac{a}{b").unwrap().text().unwrap(),
            "a/b"
        );
        assert_eq!(latex_to_plain("a\0b").unwrap().text().unwrap(), "a");
        assert!(latex_to_plain(&"{".repeat(300)).is_err());
    }
}
