//! Match highlighting for previews (Task 204). Folding follows the search index: SQLite FTS5
//! `unicode61 remove_diacritics 2` lower-cases and removes diacritics from Latin letters that
//! have a canonical decomposition (so `ệ` matches `e`), but keeps letters such as `đ`, `ø`, `æ`
//! and `ß` as they are. Ranges are byte ranges into the original text. Never logged (GC-06).

/// Latin letters with a canonical decomposition into an ASCII letter plus combining marks,
/// mapped to the lower-case ASCII letter. Generated from Unicode 16.0 (`unicodedata.normalize`
/// NFD over U+00C0..U+024F and U+1E00..U+1EFF) and checked against SQLite 3.50 FTS5
/// `remove_diacritics 2` (U+01E0/U+01E1, which FTS5 keeps, are left out). Sorted by code point.
const FOLD: &[(char, char)] = &[
    ('\u{00C0}', 'a'),
    ('\u{00C1}', 'a'),
    ('\u{00C2}', 'a'),
    ('\u{00C3}', 'a'),
    ('\u{00C4}', 'a'),
    ('\u{00C5}', 'a'),
    ('\u{00C7}', 'c'),
    ('\u{00C8}', 'e'),
    ('\u{00C9}', 'e'),
    ('\u{00CA}', 'e'),
    ('\u{00CB}', 'e'),
    ('\u{00CC}', 'i'),
    ('\u{00CD}', 'i'),
    ('\u{00CE}', 'i'),
    ('\u{00CF}', 'i'),
    ('\u{00D1}', 'n'),
    ('\u{00D2}', 'o'),
    ('\u{00D3}', 'o'),
    ('\u{00D4}', 'o'),
    ('\u{00D5}', 'o'),
    ('\u{00D6}', 'o'),
    ('\u{00D9}', 'u'),
    ('\u{00DA}', 'u'),
    ('\u{00DB}', 'u'),
    ('\u{00DC}', 'u'),
    ('\u{00DD}', 'y'),
    ('\u{00E0}', 'a'),
    ('\u{00E1}', 'a'),
    ('\u{00E2}', 'a'),
    ('\u{00E3}', 'a'),
    ('\u{00E4}', 'a'),
    ('\u{00E5}', 'a'),
    ('\u{00E7}', 'c'),
    ('\u{00E8}', 'e'),
    ('\u{00E9}', 'e'),
    ('\u{00EA}', 'e'),
    ('\u{00EB}', 'e'),
    ('\u{00EC}', 'i'),
    ('\u{00ED}', 'i'),
    ('\u{00EE}', 'i'),
    ('\u{00EF}', 'i'),
    ('\u{00F1}', 'n'),
    ('\u{00F2}', 'o'),
    ('\u{00F3}', 'o'),
    ('\u{00F4}', 'o'),
    ('\u{00F5}', 'o'),
    ('\u{00F6}', 'o'),
    ('\u{00F9}', 'u'),
    ('\u{00FA}', 'u'),
    ('\u{00FB}', 'u'),
    ('\u{00FC}', 'u'),
    ('\u{00FD}', 'y'),
    ('\u{00FF}', 'y'),
    ('\u{0100}', 'a'),
    ('\u{0101}', 'a'),
    ('\u{0102}', 'a'),
    ('\u{0103}', 'a'),
    ('\u{0104}', 'a'),
    ('\u{0105}', 'a'),
    ('\u{0106}', 'c'),
    ('\u{0107}', 'c'),
    ('\u{0108}', 'c'),
    ('\u{0109}', 'c'),
    ('\u{010A}', 'c'),
    ('\u{010B}', 'c'),
    ('\u{010C}', 'c'),
    ('\u{010D}', 'c'),
    ('\u{010E}', 'd'),
    ('\u{010F}', 'd'),
    ('\u{0112}', 'e'),
    ('\u{0113}', 'e'),
    ('\u{0114}', 'e'),
    ('\u{0115}', 'e'),
    ('\u{0116}', 'e'),
    ('\u{0117}', 'e'),
    ('\u{0118}', 'e'),
    ('\u{0119}', 'e'),
    ('\u{011A}', 'e'),
    ('\u{011B}', 'e'),
    ('\u{011C}', 'g'),
    ('\u{011D}', 'g'),
    ('\u{011E}', 'g'),
    ('\u{011F}', 'g'),
    ('\u{0120}', 'g'),
    ('\u{0121}', 'g'),
    ('\u{0122}', 'g'),
    ('\u{0123}', 'g'),
    ('\u{0124}', 'h'),
    ('\u{0125}', 'h'),
    ('\u{0128}', 'i'),
    ('\u{0129}', 'i'),
    ('\u{012A}', 'i'),
    ('\u{012B}', 'i'),
    ('\u{012C}', 'i'),
    ('\u{012D}', 'i'),
    ('\u{012E}', 'i'),
    ('\u{012F}', 'i'),
    ('\u{0130}', 'i'),
    ('\u{0134}', 'j'),
    ('\u{0135}', 'j'),
    ('\u{0136}', 'k'),
    ('\u{0137}', 'k'),
    ('\u{0139}', 'l'),
    ('\u{013A}', 'l'),
    ('\u{013B}', 'l'),
    ('\u{013C}', 'l'),
    ('\u{013D}', 'l'),
    ('\u{013E}', 'l'),
    ('\u{0143}', 'n'),
    ('\u{0144}', 'n'),
    ('\u{0145}', 'n'),
    ('\u{0146}', 'n'),
    ('\u{0147}', 'n'),
    ('\u{0148}', 'n'),
    ('\u{014C}', 'o'),
    ('\u{014D}', 'o'),
    ('\u{014E}', 'o'),
    ('\u{014F}', 'o'),
    ('\u{0150}', 'o'),
    ('\u{0151}', 'o'),
    ('\u{0154}', 'r'),
    ('\u{0155}', 'r'),
    ('\u{0156}', 'r'),
    ('\u{0157}', 'r'),
    ('\u{0158}', 'r'),
    ('\u{0159}', 'r'),
    ('\u{015A}', 's'),
    ('\u{015B}', 's'),
    ('\u{015C}', 's'),
    ('\u{015D}', 's'),
    ('\u{015E}', 's'),
    ('\u{015F}', 's'),
    ('\u{0160}', 's'),
    ('\u{0161}', 's'),
    ('\u{0162}', 't'),
    ('\u{0163}', 't'),
    ('\u{0164}', 't'),
    ('\u{0165}', 't'),
    ('\u{0168}', 'u'),
    ('\u{0169}', 'u'),
    ('\u{016A}', 'u'),
    ('\u{016B}', 'u'),
    ('\u{016C}', 'u'),
    ('\u{016D}', 'u'),
    ('\u{016E}', 'u'),
    ('\u{016F}', 'u'),
    ('\u{0170}', 'u'),
    ('\u{0171}', 'u'),
    ('\u{0172}', 'u'),
    ('\u{0173}', 'u'),
    ('\u{0174}', 'w'),
    ('\u{0175}', 'w'),
    ('\u{0176}', 'y'),
    ('\u{0177}', 'y'),
    ('\u{0178}', 'y'),
    ('\u{0179}', 'z'),
    ('\u{017A}', 'z'),
    ('\u{017B}', 'z'),
    ('\u{017C}', 'z'),
    ('\u{017D}', 'z'),
    ('\u{017E}', 'z'),
    ('\u{01A0}', 'o'),
    ('\u{01A1}', 'o'),
    ('\u{01AF}', 'u'),
    ('\u{01B0}', 'u'),
    ('\u{01CD}', 'a'),
    ('\u{01CE}', 'a'),
    ('\u{01CF}', 'i'),
    ('\u{01D0}', 'i'),
    ('\u{01D1}', 'o'),
    ('\u{01D2}', 'o'),
    ('\u{01D3}', 'u'),
    ('\u{01D4}', 'u'),
    ('\u{01D5}', 'u'),
    ('\u{01D6}', 'u'),
    ('\u{01D7}', 'u'),
    ('\u{01D8}', 'u'),
    ('\u{01D9}', 'u'),
    ('\u{01DA}', 'u'),
    ('\u{01DB}', 'u'),
    ('\u{01DC}', 'u'),
    ('\u{01DE}', 'a'),
    ('\u{01DF}', 'a'),
    ('\u{01E6}', 'g'),
    ('\u{01E7}', 'g'),
    ('\u{01E8}', 'k'),
    ('\u{01E9}', 'k'),
    ('\u{01EA}', 'o'),
    ('\u{01EB}', 'o'),
    ('\u{01EC}', 'o'),
    ('\u{01ED}', 'o'),
    ('\u{01F0}', 'j'),
    ('\u{01F4}', 'g'),
    ('\u{01F5}', 'g'),
    ('\u{01F8}', 'n'),
    ('\u{01F9}', 'n'),
    ('\u{01FA}', 'a'),
    ('\u{01FB}', 'a'),
    ('\u{0200}', 'a'),
    ('\u{0201}', 'a'),
    ('\u{0202}', 'a'),
    ('\u{0203}', 'a'),
    ('\u{0204}', 'e'),
    ('\u{0205}', 'e'),
    ('\u{0206}', 'e'),
    ('\u{0207}', 'e'),
    ('\u{0208}', 'i'),
    ('\u{0209}', 'i'),
    ('\u{020A}', 'i'),
    ('\u{020B}', 'i'),
    ('\u{020C}', 'o'),
    ('\u{020D}', 'o'),
    ('\u{020E}', 'o'),
    ('\u{020F}', 'o'),
    ('\u{0210}', 'r'),
    ('\u{0211}', 'r'),
    ('\u{0212}', 'r'),
    ('\u{0213}', 'r'),
    ('\u{0214}', 'u'),
    ('\u{0215}', 'u'),
    ('\u{0216}', 'u'),
    ('\u{0217}', 'u'),
    ('\u{0218}', 's'),
    ('\u{0219}', 's'),
    ('\u{021A}', 't'),
    ('\u{021B}', 't'),
    ('\u{021E}', 'h'),
    ('\u{021F}', 'h'),
    ('\u{0226}', 'a'),
    ('\u{0227}', 'a'),
    ('\u{0228}', 'e'),
    ('\u{0229}', 'e'),
    ('\u{022A}', 'o'),
    ('\u{022B}', 'o'),
    ('\u{022C}', 'o'),
    ('\u{022D}', 'o'),
    ('\u{022E}', 'o'),
    ('\u{022F}', 'o'),
    ('\u{0230}', 'o'),
    ('\u{0231}', 'o'),
    ('\u{0232}', 'y'),
    ('\u{0233}', 'y'),
    ('\u{1E00}', 'a'),
    ('\u{1E01}', 'a'),
    ('\u{1E02}', 'b'),
    ('\u{1E03}', 'b'),
    ('\u{1E04}', 'b'),
    ('\u{1E05}', 'b'),
    ('\u{1E06}', 'b'),
    ('\u{1E07}', 'b'),
    ('\u{1E08}', 'c'),
    ('\u{1E09}', 'c'),
    ('\u{1E0A}', 'd'),
    ('\u{1E0B}', 'd'),
    ('\u{1E0C}', 'd'),
    ('\u{1E0D}', 'd'),
    ('\u{1E0E}', 'd'),
    ('\u{1E0F}', 'd'),
    ('\u{1E10}', 'd'),
    ('\u{1E11}', 'd'),
    ('\u{1E12}', 'd'),
    ('\u{1E13}', 'd'),
    ('\u{1E14}', 'e'),
    ('\u{1E15}', 'e'),
    ('\u{1E16}', 'e'),
    ('\u{1E17}', 'e'),
    ('\u{1E18}', 'e'),
    ('\u{1E19}', 'e'),
    ('\u{1E1A}', 'e'),
    ('\u{1E1B}', 'e'),
    ('\u{1E1C}', 'e'),
    ('\u{1E1D}', 'e'),
    ('\u{1E1E}', 'f'),
    ('\u{1E1F}', 'f'),
    ('\u{1E20}', 'g'),
    ('\u{1E21}', 'g'),
    ('\u{1E22}', 'h'),
    ('\u{1E23}', 'h'),
    ('\u{1E24}', 'h'),
    ('\u{1E25}', 'h'),
    ('\u{1E26}', 'h'),
    ('\u{1E27}', 'h'),
    ('\u{1E28}', 'h'),
    ('\u{1E29}', 'h'),
    ('\u{1E2A}', 'h'),
    ('\u{1E2B}', 'h'),
    ('\u{1E2C}', 'i'),
    ('\u{1E2D}', 'i'),
    ('\u{1E2E}', 'i'),
    ('\u{1E2F}', 'i'),
    ('\u{1E30}', 'k'),
    ('\u{1E31}', 'k'),
    ('\u{1E32}', 'k'),
    ('\u{1E33}', 'k'),
    ('\u{1E34}', 'k'),
    ('\u{1E35}', 'k'),
    ('\u{1E36}', 'l'),
    ('\u{1E37}', 'l'),
    ('\u{1E38}', 'l'),
    ('\u{1E39}', 'l'),
    ('\u{1E3A}', 'l'),
    ('\u{1E3B}', 'l'),
    ('\u{1E3C}', 'l'),
    ('\u{1E3D}', 'l'),
    ('\u{1E3E}', 'm'),
    ('\u{1E3F}', 'm'),
    ('\u{1E40}', 'm'),
    ('\u{1E41}', 'm'),
    ('\u{1E42}', 'm'),
    ('\u{1E43}', 'm'),
    ('\u{1E44}', 'n'),
    ('\u{1E45}', 'n'),
    ('\u{1E46}', 'n'),
    ('\u{1E47}', 'n'),
    ('\u{1E48}', 'n'),
    ('\u{1E49}', 'n'),
    ('\u{1E4A}', 'n'),
    ('\u{1E4B}', 'n'),
    ('\u{1E4C}', 'o'),
    ('\u{1E4D}', 'o'),
    ('\u{1E4E}', 'o'),
    ('\u{1E4F}', 'o'),
    ('\u{1E50}', 'o'),
    ('\u{1E51}', 'o'),
    ('\u{1E52}', 'o'),
    ('\u{1E53}', 'o'),
    ('\u{1E54}', 'p'),
    ('\u{1E55}', 'p'),
    ('\u{1E56}', 'p'),
    ('\u{1E57}', 'p'),
    ('\u{1E58}', 'r'),
    ('\u{1E59}', 'r'),
    ('\u{1E5A}', 'r'),
    ('\u{1E5B}', 'r'),
    ('\u{1E5C}', 'r'),
    ('\u{1E5D}', 'r'),
    ('\u{1E5E}', 'r'),
    ('\u{1E5F}', 'r'),
    ('\u{1E60}', 's'),
    ('\u{1E61}', 's'),
    ('\u{1E62}', 's'),
    ('\u{1E63}', 's'),
    ('\u{1E64}', 's'),
    ('\u{1E65}', 's'),
    ('\u{1E66}', 's'),
    ('\u{1E67}', 's'),
    ('\u{1E68}', 's'),
    ('\u{1E69}', 's'),
    ('\u{1E6A}', 't'),
    ('\u{1E6B}', 't'),
    ('\u{1E6C}', 't'),
    ('\u{1E6D}', 't'),
    ('\u{1E6E}', 't'),
    ('\u{1E6F}', 't'),
    ('\u{1E70}', 't'),
    ('\u{1E71}', 't'),
    ('\u{1E72}', 'u'),
    ('\u{1E73}', 'u'),
    ('\u{1E74}', 'u'),
    ('\u{1E75}', 'u'),
    ('\u{1E76}', 'u'),
    ('\u{1E77}', 'u'),
    ('\u{1E78}', 'u'),
    ('\u{1E79}', 'u'),
    ('\u{1E7A}', 'u'),
    ('\u{1E7B}', 'u'),
    ('\u{1E7C}', 'v'),
    ('\u{1E7D}', 'v'),
    ('\u{1E7E}', 'v'),
    ('\u{1E7F}', 'v'),
    ('\u{1E80}', 'w'),
    ('\u{1E81}', 'w'),
    ('\u{1E82}', 'w'),
    ('\u{1E83}', 'w'),
    ('\u{1E84}', 'w'),
    ('\u{1E85}', 'w'),
    ('\u{1E86}', 'w'),
    ('\u{1E87}', 'w'),
    ('\u{1E88}', 'w'),
    ('\u{1E89}', 'w'),
    ('\u{1E8A}', 'x'),
    ('\u{1E8B}', 'x'),
    ('\u{1E8C}', 'x'),
    ('\u{1E8D}', 'x'),
    ('\u{1E8E}', 'y'),
    ('\u{1E8F}', 'y'),
    ('\u{1E90}', 'z'),
    ('\u{1E91}', 'z'),
    ('\u{1E92}', 'z'),
    ('\u{1E93}', 'z'),
    ('\u{1E94}', 'z'),
    ('\u{1E95}', 'z'),
    ('\u{1E96}', 'h'),
    ('\u{1E97}', 't'),
    ('\u{1E98}', 'w'),
    ('\u{1E99}', 'y'),
    ('\u{1EA0}', 'a'),
    ('\u{1EA1}', 'a'),
    ('\u{1EA2}', 'a'),
    ('\u{1EA3}', 'a'),
    ('\u{1EA4}', 'a'),
    ('\u{1EA5}', 'a'),
    ('\u{1EA6}', 'a'),
    ('\u{1EA7}', 'a'),
    ('\u{1EA8}', 'a'),
    ('\u{1EA9}', 'a'),
    ('\u{1EAA}', 'a'),
    ('\u{1EAB}', 'a'),
    ('\u{1EAC}', 'a'),
    ('\u{1EAD}', 'a'),
    ('\u{1EAE}', 'a'),
    ('\u{1EAF}', 'a'),
    ('\u{1EB0}', 'a'),
    ('\u{1EB1}', 'a'),
    ('\u{1EB2}', 'a'),
    ('\u{1EB3}', 'a'),
    ('\u{1EB4}', 'a'),
    ('\u{1EB5}', 'a'),
    ('\u{1EB6}', 'a'),
    ('\u{1EB7}', 'a'),
    ('\u{1EB8}', 'e'),
    ('\u{1EB9}', 'e'),
    ('\u{1EBA}', 'e'),
    ('\u{1EBB}', 'e'),
    ('\u{1EBC}', 'e'),
    ('\u{1EBD}', 'e'),
    ('\u{1EBE}', 'e'),
    ('\u{1EBF}', 'e'),
    ('\u{1EC0}', 'e'),
    ('\u{1EC1}', 'e'),
    ('\u{1EC2}', 'e'),
    ('\u{1EC3}', 'e'),
    ('\u{1EC4}', 'e'),
    ('\u{1EC5}', 'e'),
    ('\u{1EC6}', 'e'),
    ('\u{1EC7}', 'e'),
    ('\u{1EC8}', 'i'),
    ('\u{1EC9}', 'i'),
    ('\u{1ECA}', 'i'),
    ('\u{1ECB}', 'i'),
    ('\u{1ECC}', 'o'),
    ('\u{1ECD}', 'o'),
    ('\u{1ECE}', 'o'),
    ('\u{1ECF}', 'o'),
    ('\u{1ED0}', 'o'),
    ('\u{1ED1}', 'o'),
    ('\u{1ED2}', 'o'),
    ('\u{1ED3}', 'o'),
    ('\u{1ED4}', 'o'),
    ('\u{1ED5}', 'o'),
    ('\u{1ED6}', 'o'),
    ('\u{1ED7}', 'o'),
    ('\u{1ED8}', 'o'),
    ('\u{1ED9}', 'o'),
    ('\u{1EDA}', 'o'),
    ('\u{1EDB}', 'o'),
    ('\u{1EDC}', 'o'),
    ('\u{1EDD}', 'o'),
    ('\u{1EDE}', 'o'),
    ('\u{1EDF}', 'o'),
    ('\u{1EE0}', 'o'),
    ('\u{1EE1}', 'o'),
    ('\u{1EE2}', 'o'),
    ('\u{1EE3}', 'o'),
    ('\u{1EE4}', 'u'),
    ('\u{1EE5}', 'u'),
    ('\u{1EE6}', 'u'),
    ('\u{1EE7}', 'u'),
    ('\u{1EE8}', 'u'),
    ('\u{1EE9}', 'u'),
    ('\u{1EEA}', 'u'),
    ('\u{1EEB}', 'u'),
    ('\u{1EEC}', 'u'),
    ('\u{1EED}', 'u'),
    ('\u{1EEE}', 'u'),
    ('\u{1EEF}', 'u'),
    ('\u{1EF0}', 'u'),
    ('\u{1EF1}', 'u'),
    ('\u{1EF2}', 'y'),
    ('\u{1EF3}', 'y'),
    ('\u{1EF4}', 'y'),
    ('\u{1EF5}', 'y'),
    ('\u{1EF6}', 'y'),
    ('\u{1EF7}', 'y'),
    ('\u{1EF8}', 'y'),
    ('\u{1EF9}', 'y'),
];

/// A highlight range: byte offsets into the original text, `start < end`.
pub type Range = (usize, usize);

/// One piece of a preview: text plus whether it matched the query.
#[derive(Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub hit: bool,
}

/// Never prints the text (GC-06).
impl std::fmt::Debug for Segment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Segment")
            .field("len", &self.text.len())
            .field("hit", &self.hit)
            .finish()
    }
}

/// Combining diacritical marks (U+0300..U+036F): FTS5 drops them, so they never take part in
/// a match; they stay inside the highlight of the letter before them.
fn is_combining(c: char) -> bool {
    ('\u{0300}'..='\u{036F}').contains(&c)
}

/// Folds one lower-case character like the search index does.
fn fold_lower(c: char) -> char {
    match FOLD.binary_search_by(|(k, _)| k.cmp(&c)) {
        Ok(i) => FOLD.get(i).map_or(c, |&(_, base)| base),
        Err(_) => c,
    }
}

/// `text` folded for matching: one entry per folded character with the byte range of the
/// original character it came from (combining marks widen the range of the letter before).
struct Folded {
    chars: Vec<char>,
    spans: Vec<Range>,
}

fn fold(text: &str) -> Folded {
    let mut chars = Vec::with_capacity(text.len());
    let mut spans: Vec<Range> = Vec::with_capacity(text.len());
    for (start, c) in text.char_indices() {
        let end = start + c.len_utf8();
        for lc in c.to_lowercase() {
            if is_combining(lc) {
                if let Some(last) = spans.last_mut() {
                    last.1 = end;
                }
                continue;
            }
            chars.push(fold_lower(lc));
            spans.push((start, end));
        }
    }
    Folded { chars, spans }
}

/// Every match of `needle` in `hay` as folded index ranges; with `word_start`, only where a
/// word begins.
fn find_all(hay: &[char], needle: &[char], word_start: bool, out: &mut Vec<(usize, usize)>) {
    if needle.is_empty() || needle.len() > hay.len() {
        return;
    }
    for i in 0..=hay.len() - needle.len() {
        let after_letter = i > 0 && hay.get(i - 1).is_some_and(|c| c.is_alphanumeric());
        if word_start && after_letter {
            continue;
        }
        if hay.get(i..i + needle.len()) == Some(needle) {
            out.push((i, i + needle.len()));
        }
    }
}

/// Matched ranges of `query` in `text`, sorted and merged. Mirrors the storage search: with
/// at least 3 characters and a letter or digit, every word of the query matches the start of
/// a word in the text (FTS prefix tokens); otherwise the whole query matches anywhere
/// (the substring fallback).
pub fn match_ranges(text: &str, query: &str) -> Vec<Range> {
    let q = query.trim();
    if q.is_empty() || text.is_empty() {
        return Vec::new();
    }
    let folded = fold(text);
    let mut found: Vec<(usize, usize)> = Vec::new();
    let fts = q.chars().count() >= 3 && q.chars().any(char::is_alphanumeric);
    if fts {
        // FTS tokens are runs of letters and digits; each one matches a word start.
        for word in q.split(|c: char| !c.is_alphanumeric()) {
            find_all(&folded.chars, &fold(word).chars, true, &mut found);
        }
    } else {
        find_all(&folded.chars, &fold(q).chars, false, &mut found);
    }
    // Folded indexes back to byte ranges of the original text.
    let mut ranges: Vec<Range> = found
        .into_iter()
        .filter_map(|(a, b)| {
            let start = folded.spans.get(a)?.0;
            let end = folded.spans.get(b.checked_sub(1)?)?.1;
            (start < end).then_some((start, end))
        })
        .collect();
    ranges.sort_unstable();
    let mut merged: Vec<Range> = Vec::with_capacity(ranges.len());
    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.0 <= last.1 => last.1 = last.1.max(r.1),
            _ => merged.push(r),
        }
    }
    merged
}

fn push_segment(out: &mut Vec<Segment>, text: &str, a: usize, b: usize, hit: bool) {
    if let Some(t) = text.get(a..b).filter(|t| !t.is_empty()) {
        out.push(Segment {
            text: t.to_string(),
            hit,
        });
    }
}

/// Splits `text` into hit and non-hit segments (`ranges` sorted and merged).
pub fn segments(text: &str, ranges: &[Range]) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut at = 0;
    for &(a, b) in ranges {
        if a < at || b > text.len() {
            continue;
        }
        push_segment(&mut out, text, at, a, false);
        push_segment(&mut out, text, a, b, true);
        at = b;
    }
    push_segment(&mut out, text, at, text.len(), false);
    out
}

/// Characters kept before the first hit when the start of the text is skipped.
const LEAD_CHARS: usize = 12;

/// Segments for a one-line row of at most `max_chars` characters. When the first hit lies
/// beyond the visible part, the text starts a few characters before it, behind an ellipsis.
pub fn row_segments(text: &str, query: &str, max_chars: usize) -> Vec<Segment> {
    let ranges = match_ranges(text, query);
    // Byte offset of every char, plus the end.
    let bounds: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let total = bounds.len().saturating_sub(1);
    let char_at = |byte: usize| bounds.partition_point(|&b| b < byte);
    let mut first = 0;
    if let Some(&(hit_start, hit_end)) = ranges.first() {
        if char_at(hit_end) > max_chars {
            first = char_at(hit_start).saturating_sub(LEAD_CHARS);
        }
    }
    let lead = first > 0;
    let budget = if lead {
        max_chars.saturating_sub(1)
    } else {
        max_chars
    };
    let last = first.saturating_add(budget).min(total);
    let from = bounds.get(first).copied().unwrap_or(0);
    let to = bounds.get(last).copied().unwrap_or(text.len());
    let window = text.get(from..to).unwrap_or("");
    let shifted: Vec<Range> = ranges
        .iter()
        .filter(|&&(a, b)| a >= from && b <= to)
        .map(|&(a, b)| (a - from, b - from))
        .collect();
    let mut out = Vec::new();
    if lead {
        out.push(Segment {
            text: "…".into(),
            hit: false,
        });
    }
    out.extend(segments(window, &shifted));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(text: &str, query: &str) -> Vec<String> {
        match_ranges(text, query)
            .iter()
            .map(|&(a, b)| text[a..b].to_string())
            .collect()
    }

    #[test]
    fn fold_table_is_sorted_for_binary_search() {
        assert!(FOLD.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn vietnamese_diacritics_are_ignored() {
        assert_eq!(hits("Tiếng Việt rất hay", "tieng viet"), ["Tiếng", "Việt"]);
        assert_eq!(hits("Tiếng Việt", "TIẾNG"), ["Tiếng"]);
        assert_eq!(hits("Nguyễn Thị Ánh", "nguyen anh"), ["Nguyễn", "Ánh"]);
        // Each query word is its own hit; the space between them is not highlighted.
        assert_eq!(hits("ủy ban nhân dân", "uy ban"), ["ủy", "ban"]);
        // Query words match word starts (FTS prefix tokens).
        assert_eq!(hits("cà phê sữa đá", "phe sua"), ["phê", "sữa"]);
        // Three or more characters: only word starts match (FTS prefix tokens).
        assert_eq!(hits("Nguyễn", "uyen"), Vec::<String>::new());
    }

    #[test]
    fn letters_without_a_decomposition_are_kept_like_fts() {
        // FTS5 does not fold đ to d, so neither does the highlight.
        assert_eq!(hits("đồng hồ", "dong"), Vec::<String>::new());
        assert_eq!(hits("đồng hồ", "đông"), ["đồng"]);
        assert_eq!(hits("ĐÀ NẴNG", "đà"), ["ĐÀ"]);
    }

    #[test]
    fn decomposed_input_keeps_its_marks_inside_the_hit() {
        // "Việt" written with combining marks (NFD): e + U+0323 + U+0302.
        let nfd = "Vie\u{0323}\u{0302}t Nam";
        assert_eq!(hits(nfd, "viet"), ["Vie\u{0323}\u{0302}t"]);
    }

    #[test]
    fn short_queries_match_anywhere() {
        assert_eq!(hits("kubectl get pods", "ub"), ["ub"]);
        assert_eq!(hits("a-b a-b", "-b"), ["-b", "-b"]);
        // Folding applies to short queries too: "ở" and "ò" both fold to "o".
        assert_eq!(hits("Phở bò", "ở"), ["ở", "ò"]);
        assert!(hits("anything", "").is_empty());
        assert!(hits("anything", "   ").is_empty());
    }

    #[test]
    fn long_queries_without_letters_match_anywhere() {
        assert_eq!(hits("a --- b", "---"), ["---"]);
    }

    #[test]
    fn overlapping_hits_are_merged_and_sorted() {
        assert_eq!(
            match_ranges("kubectl kube", "kube kubectl"),
            [(0, 7), (8, 12)]
        );
        assert_eq!(hits("abcabc", "abc"), ["abc"]);
    }

    #[test]
    fn segments_cover_the_whole_text_in_order() {
        let text = "Tiếng Việt rất hay";
        let segs = segments(text, &match_ranges(text, "viet"));
        let joined: String = segs.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, text);
        let flags: Vec<bool> = segs.iter().map(|s| s.hit).collect();
        assert_eq!(flags, [false, true, false]);
        assert_eq!(segs[1].text, "Việt");
        assert_eq!(
            segments("abc", &[]),
            [Segment {
                text: "abc".into(),
                hit: false
            }]
        );
        assert!(segments("", &[]).is_empty());
    }

    #[test]
    fn row_segments_bring_a_late_hit_into_view() {
        let text = format!("{} needle tail", "x".repeat(200));
        let segs = row_segments(&text, "needle", 40);
        let shown: String = segs.iter().map(|s| s.text.as_str()).collect();
        assert!(shown.starts_with('…'));
        assert!(shown.chars().count() <= 41);
        assert!(segs.iter().any(|s| s.hit && s.text == "needle"));
        // An early hit keeps the start; the end is cut.
        let segs = row_segments(&format!("needle {}", "y".repeat(100)), "needle", 20);
        assert_eq!(segs[0].text, "needle");
        let total: usize = segs.iter().map(|s| s.text.chars().count()).sum();
        assert_eq!(total, 20);
        // No query: plain text, cut to the limit.
        let segs = row_segments("Tiếng Việt", "", 5);
        assert_eq!(
            segs,
            [Segment {
                text: "Tiếng".into(),
                hit: false
            }]
        );
    }

    #[test]
    fn segment_debug_hides_the_text() {
        let s = Segment {
            text: "secret words".into(),
            hit: true,
        };
        assert!(!format!("{s:?}").contains("secret"));
    }
}
