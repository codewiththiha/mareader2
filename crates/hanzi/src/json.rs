//! Just enough JSON for pypinyin's two files: strings, lists, one object.

/// A value in those files.
pub enum Json {
    Text(String),
    List(Vec<Json>),
}

/// Hands each member of a top-level object to `member`, in file order.
pub fn each(text: &str, mut member: impl FnMut(String, Json) -> Option<()>) -> Option<()> {
    let mut reader = Reader { rest: text };
    reader.eat('{')?;
    if reader.peek()? == '}' {
        return Some(());
    }
    loop {
        let key = reader.text()?;
        reader.eat(':')?;
        member(key, reader.value()?)?;
        match reader.next()? {
            ',' => continue,
            '}' => return Some(()),
            _ => return None,
        }
    }
}

struct Reader<'a> {
    rest: &'a str,
}

impl Reader<'_> {
    /// The next character that isn't white space, left unread.
    fn peek(&mut self) -> Option<char> {
        self.rest = self.rest.trim_start();
        self.rest.chars().next()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.rest = self.rest.get(c.len_utf8()..)?;
        Some(c)
    }

    fn eat(&mut self, want: char) -> Option<()> {
        (self.next()? == want).then_some(())
    }

    fn value(&mut self) -> Option<Json> {
        if self.peek()? == '"' {
            return self.text().map(Json::Text);
        }
        self.eat('[')?;
        let mut items = Vec::new();
        if self.peek()? == ']' {
            self.next();
            return Some(Json::List(items));
        }
        loop {
            items.push(self.value()?);
            match self.next()? {
                ',' => continue,
                ']' => return Some(Json::List(items)),
                _ => return None,
            }
        }
    }

    /// A string, its escapes undone.
    fn text(&mut self) -> Option<String> {
        self.eat('"')?;
        let mut out = String::new();
        let mut chars = self.rest.char_indices();
        while let Some((at, c)) = chars.next() {
            match c {
                '"' => {
                    self.rest = self.rest.get(at + 1..)?;
                    return Some(out);
                }
                '\\' => out.push(match chars.next()?.1 {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    'u' => unicode(&mut chars)?,
                    c => c,
                }),
                c => out.push(c),
            }
        }
        None
    }
}

/// `\uXXXX`, joining a surrogate pair into one character.
fn unicode(chars: &mut std::str::CharIndices<'_>) -> Option<char> {
    let hex = |chars: &mut std::str::CharIndices<'_>| {
        let digits: String = chars.by_ref().take(4).map(|(_, c)| c).collect();
        u32::from_str_radix(&digits, 16).ok()
    };
    let high = hex(chars)?;
    if !(0xD800..0xDC00).contains(&high) {
        return char::from_u32(high);
    }
    if chars.next()?.1 != '\\' || chars.next()?.1 != 'u' {
        return None;
    }
    let low = hex(chars)?;
    char::from_u32(0x10000 + ((high - 0xD800) << 10) + (low.checked_sub(0xDC00)?))
}
