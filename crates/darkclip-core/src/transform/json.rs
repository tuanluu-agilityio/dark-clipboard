//! JSON validation and re-formatting. Strings and numbers are copied verbatim (no key
//! reordering, no number rewriting); only whitespace changes. Depth is counted, so a
//! hostile document cannot overflow the stack.

use super::{TransformError, MAX_INPUT_BYTES, MAX_JSON_DEPTH};

struct Parser<'a> {
    src: &'a str,
    b: &'a [u8],
    pos: usize,
    out: String,
    pretty: bool,
}

pub(super) fn format(input: &str, pretty: bool) -> Result<String, TransformError> {
    let mut p = Parser {
        src: input,
        b: input.as_bytes(),
        pos: 0,
        out: String::with_capacity(input.len()),
        pretty,
    };
    p.value(0)?;
    p.ws();
    if p.pos != p.b.len() {
        return Err(TransformError::InvalidJson);
    }
    Ok(p.out)
}

const BAD: TransformError = TransformError::InvalidJson;

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), TransformError> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(BAD)
        }
    }

    /// A line break and indent (pretty only). Output past the cap is refused as it grows,
    /// so a deep, long document never allocates its full expanded size.
    fn newline(&mut self, depth: usize) -> Result<(), TransformError> {
        if self.pretty {
            self.out.push('\n');
            for _ in 0..depth {
                self.out.push_str("  ");
            }
            if self.out.len() > MAX_INPUT_BYTES {
                return Err(TransformError::TooLarge);
            }
        }
        Ok(())
    }

    fn value(&mut self, depth: usize) -> Result<(), TransformError> {
        self.ws();
        match self.peek().ok_or(BAD)? {
            b'{' => self.container(depth, b'{', b'}'),
            b'[' => self.container(depth, b'[', b']'),
            b'"' => self.string(),
            b't' => self.literal("true"),
            b'f' => self.literal("false"),
            b'n' => self.literal("null"),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(BAD),
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), TransformError> {
        let end = self.pos + word.len();
        if self.b.get(self.pos..end) == Some(word.as_bytes()) {
            self.out.push_str(word);
            self.pos = end;
            Ok(())
        } else {
            Err(BAD)
        }
    }

    fn digits(&mut self) -> Result<(), TransformError> {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos == start {
            Err(BAD)
        } else {
            Ok(())
        }
    }

    fn number(&mut self) -> Result<(), TransformError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => self.digits()?,
            _ => return Err(BAD),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.digits()?;
        }
        // The slice ends on an ASCII byte, so it is a char boundary.
        self.out.push_str(self.src.get(start..self.pos).ok_or(BAD)?);
        Ok(())
    }

    fn string(&mut self) -> Result<(), TransformError> {
        let start = self.pos;
        self.expect(b'"')?;
        loop {
            match self.peek().ok_or(BAD)? {
                b'"' => {
                    self.pos += 1;
                    break;
                }
                b'\\' => {
                    self.pos += 1;
                    match self.peek().ok_or(BAD)? {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => self.pos += 1,
                        b'u' => {
                            self.pos += 1;
                            for _ in 0..4 {
                                if !self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                                    return Err(BAD);
                                }
                                self.pos += 1;
                            }
                        }
                        _ => return Err(BAD),
                    }
                }
                c if c < 0x20 => return Err(BAD),
                _ => self.pos += 1,
            }
        }
        self.out.push_str(self.src.get(start..self.pos).ok_or(BAD)?);
        Ok(())
    }

    fn container(&mut self, depth: usize, open: u8, close: u8) -> Result<(), TransformError> {
        let depth = depth + 1;
        if depth > MAX_JSON_DEPTH {
            return Err(TransformError::TooDeep);
        }
        self.pos += 1;
        self.out.push(char::from(open));
        self.ws();
        if self.peek() == Some(close) {
            self.pos += 1;
            self.out.push(char::from(close));
            return Ok(());
        }
        loop {
            self.newline(depth)?;
            self.ws();
            if open == b'{' {
                if self.peek() != Some(b'"') {
                    return Err(BAD);
                }
                self.string()?;
                self.ws();
                self.expect(b':')?;
                self.out.push_str(if self.pretty { ": " } else { ":" });
            }
            self.value(depth)?;
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.out.push(',');
                }
                Some(c) if c == close => {
                    self.pos += 1;
                    self.newline(depth - 1)?;
                    self.out.push(char::from(close));
                    return Ok(());
                }
                _ => return Err(BAD),
            }
        }
    }
}
