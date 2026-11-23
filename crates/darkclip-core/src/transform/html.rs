//! Basic HTML to plain text: tags removed, `script`/`style` content and comments dropped,
//! common entities decoded, block tags become line breaks. Not a parser for arbitrary HTML;
//! it only ever produces text (nothing it reads is interpreted or fetched).

const BLOCK: &[&str] = &[
    "p",
    "div",
    "br",
    "li",
    "ul",
    "ol",
    "tr",
    "table",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "section",
    "article",
    "header",
    "footer",
    "blockquote",
    "pre",
    "hr",
];

fn find_ci(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let end = hay.len().checked_sub(needle.len())?;
    (from..=end).find(|&i| {
        hay.get(i..i + needle.len())
            .is_some_and(|w| w.eq_ignore_ascii_case(needle))
    })
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "hellip" => '\u{2026}',
        "copy" => '\u{a9}',
        _ => return None,
    })
}

/// The character an entity body (between `&` and `;`) stands for.
fn entity(body: &str) -> Option<char> {
    if let Some(num) = body.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse::<u32>().ok()?,
        };
        let c = char::from_u32(code)?;
        return (code != 0 && !c.is_control()).then_some(c);
    }
    named_entity(body)
}

struct Out {
    text: String,
    space: bool,
    newline: bool,
}

impl Out {
    fn flush(&mut self) {
        if !self.text.is_empty() {
            if self.newline {
                self.text.push('\n');
            } else if self.space {
                self.text.push(' ');
            }
        }
        self.space = false;
        self.newline = false;
    }

    fn push(&mut self, c: char) {
        self.flush();
        self.text.push(c);
    }
}

pub(super) fn to_text(input: &str) -> String {
    let b = input.as_bytes();
    let mut out = Out {
        text: String::with_capacity(input.len() / 2),
        space: false,
        newline: false,
    };
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        match c {
            b'<' => {
                let next = b.get(i + 1).copied().unwrap_or(0);
                if input.get(i..).is_some_and(|r| r.starts_with("<!--")) {
                    i = input
                        .get(i + 4..)
                        .and_then(|r| r.find("-->"))
                        .map_or(b.len(), |p| i + 4 + p + 3);
                } else if next.is_ascii_alphabetic() || matches!(next, b'/' | b'!' | b'?') {
                    i = tag(input, i, &mut out);
                } else {
                    out.push('<');
                    i += 1;
                }
            }
            b'&' => {
                // `;` is ASCII, so search bytes: a multi-byte character may straddle the
                // 12-byte window. Entity bodies are ASCII, so `get` below is a boundary check.
                let window = b.get(i + 1..(i + 13).min(b.len())).unwrap_or_default();
                let decoded = window
                    .iter()
                    .position(|&c| c == b';')
                    .and_then(|p| input.get(i + 1..i + 1 + p).map(|body| (p, body)))
                    .and_then(|(p, body)| entity(body).map(|c| (p, c)));
                if let Some((p, ch)) = decoded {
                    if ch == ' ' {
                        out.space = true;
                    } else {
                        out.push(ch);
                    }
                    i += p + 2;
                } else {
                    out.push('&');
                    i += 1;
                }
            }
            c if c.is_ascii_whitespace() => {
                out.space = true;
                i += 1;
            }
            _ => {
                // Copy one whole character.
                let Some(ch) = input.get(i..).and_then(|r| r.chars().next()) else {
                    break;
                };
                if ch.is_whitespace() {
                    out.space = true;
                } else {
                    out.push(ch);
                }
                i += ch.len_utf8();
            }
        }
    }
    out.text
}

/// Handles the tag starting at `start` (`<`); returns the index after it (and after the
/// content of `script`/`style`).
fn tag(input: &str, start: usize, out: &mut Out) -> usize {
    let b = input.as_bytes();
    let mut i = start + 1;
    let closing = b.get(i) == Some(&b'/');
    if closing {
        i += 1;
    }
    let name_start = i;
    while b.get(i).is_some_and(|c| c.is_ascii_alphanumeric()) {
        i += 1;
    }
    let name = input.get(name_start..i).unwrap_or("").to_ascii_lowercase();
    // Skip to the closing '>', not stopping inside a quoted attribute value.
    let mut quote: Option<u8> = None;
    while let Some(&c) = b.get(i) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if c == b'>' => break,
            None => {}
        }
        i += 1;
    }
    if i >= b.len() {
        return b.len();
    }
    i += 1;
    if !closing && (name == "script" || name == "style") {
        let close = format!("</{name}");
        return match find_ci(b, close.as_bytes(), i) {
            Some(p) => b
                .get(p..)
                .and_then(|r| r.iter().position(|&c| c == b'>'))
                .map_or(b.len(), |g| p + g + 1),
            None => b.len(),
        };
    }
    if BLOCK.contains(&name.as_str()) {
        out.newline = true;
    }
    i
}
