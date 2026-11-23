use super::*;

fn ok(t: Transform, input: &str) -> String {
    t.apply(input).unwrap()
}

fn err(t: Transform, input: &str) -> TransformError {
    t.apply(input).unwrap_err()
}

#[test]
fn trim_cases() {
    assert_eq!(ok(Transform::Trim, "  a b \n\t"), "a b");
    assert_eq!(ok(Transform::Trim, ""), "");
    assert_eq!(ok(Transform::Trim, " \n "), "");
    assert_eq!(ok(Transform::Trim, "  Xin chào 👋  "), "Xin chào 👋");
    assert_eq!(ok(Transform::Trim, " a \n b "), "a \n b");
}

#[test]
fn trim_lines_cases() {
    assert_eq!(ok(Transform::TrimLines, " a \n  b  "), "a\nb");
    assert_eq!(ok(Transform::TrimLines, " a \r\n b \r\n"), "a\r\nb\r\n");
    assert_eq!(ok(Transform::TrimLines, ""), "");
    assert_eq!(ok(Transform::TrimLines, "  \n  x"), "\nx");
}

#[test]
fn line_endings() {
    assert_eq!(ok(Transform::LineEndingsLf, "a\r\nb\rc\nd"), "a\nb\nc\nd");
    assert_eq!(ok(Transform::LineEndingsLf, "a\r\n\r\nb\r\n"), "a\n\nb\n");
    assert_eq!(
        ok(Transform::LineEndingsCrlf, "a\nb\r\nc\rd"),
        "a\r\nb\r\nc\r\nd"
    );
    assert_eq!(ok(Transform::LineEndingsCrlf, ""), "");
    assert_eq!(ok(Transform::LineEndingsCrlf, "x"), "x");
}

#[test]
fn case_conversion_is_unicode() {
    assert_eq!(
        ok(Transform::Upper, "Việt Nam 😀 straße"),
        "VIỆT NAM 😀 STRASSE"
    );
    assert_eq!(ok(Transform::Lower, "ĐÀ NẴNG 😀"), "đà nẵng 😀");
    assert_eq!(ok(Transform::Upper, ""), "");
}

#[test]
fn json_pretty_and_minify() {
    let compact = r#"{"a":[1,2.50,{"b":null}],"c":"x\u00e9 é","d":{},"e":[]}"#;
    let pretty = "{\n  \"a\": [\n    1,\n    2.50,\n    {\n      \"b\": null\n    }\n  ],\n  \"c\": \"x\\u00e9 é\",\n  \"d\": {},\n  \"e\": []\n}";
    assert_eq!(ok(Transform::JsonPretty, compact), pretty);
    assert_eq!(ok(Transform::JsonMinify, pretty), compact);
    assert_eq!(ok(Transform::JsonPretty, " [1] \n"), "[\n  1\n]");
    assert_eq!(
        ok(Transform::JsonMinify, " { \"k\" : \"a b\" } "),
        r#"{"k":"a b"}"#
    );
    assert_eq!(ok(Transform::JsonMinify, "true"), "true");
    assert_eq!(ok(Transform::JsonPretty, "\"Việt 😀\""), "\"Việt 😀\"");
}

#[test]
fn json_invalid_inputs() {
    for bad in [
        "",
        "   ",
        "{\"a\":}",
        "{a:1}",
        "[1,]",
        "{\"a\":1,}",
        "[1] x",
        "'x'",
        "01",
        "[1 2]",
        "{\"a\" 1}",
        "\"unterminated",
        "\"bad \\q escape\"",
        "nul",
        "-",
        "1.",
    ] {
        assert_eq!(
            err(Transform::JsonPretty, bad),
            TransformError::InvalidJson,
            "{bad:?}"
        );
        assert_eq!(
            err(Transform::JsonMinify, bad),
            TransformError::InvalidJson,
            "{bad:?}"
        );
    }
}

#[test]
fn json_depth_limit() {
    let nest = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(Transform::JsonMinify.apply(&nest(MAX_JSON_DEPTH)).is_ok());
    assert_eq!(
        err(Transform::JsonMinify, &nest(MAX_JSON_DEPTH + 1)),
        TransformError::TooDeep
    );
    // An unclosed deep document is refused as too deep, not by running out of stack.
    assert_eq!(
        err(Transform::JsonPretty, &"[".repeat(100_000)),
        TransformError::TooDeep
    );
}

#[test]
fn oversize_input_refused() {
    let big = "a".repeat(MAX_INPUT_BYTES + 1);
    for t in Transform::ALL {
        assert_eq!(err(t, &big), TransformError::TooLarge, "{t:?}");
    }
    let at_cap = "a".repeat(MAX_INPUT_BYTES);
    assert!(Transform::Trim.apply(&at_cap).is_ok());
}

#[test]
fn url_encode_decode() {
    assert_eq!(
        ok(Transform::UrlEncode, "a b&c=d/é~-_.Z9"),
        "a%20b%26c%3Dd%2F%C3%A9~-_.Z9"
    );
    assert_eq!(ok(Transform::UrlEncode, "😀"), "%F0%9F%98%80");
    assert_eq!(ok(Transform::UrlEncode, ""), "");
    assert_eq!(
        ok(Transform::UrlDecode, "a%20b%C3%A9+x%f0%9f%98%80"),
        "a bé+x😀"
    );
    assert_eq!(ok(Transform::UrlDecode, ""), "");
    assert_eq!(ok(Transform::UrlDecode, "Việt"), "Việt");
    for bad in ["%zz", "%2", "%", "abc%", "%FF", "%C3", "%C3%28"] {
        assert_eq!(
            err(Transform::UrlDecode, bad),
            TransformError::InvalidUrl,
            "{bad:?}"
        );
    }
}

#[test]
fn base64_encode_decode() {
    assert_eq!(ok(Transform::Base64Encode, "Hello"), "SGVsbG8=");
    assert_eq!(ok(Transform::Base64Encode, "Hell"), "SGVsbA==");
    assert_eq!(ok(Transform::Base64Encode, "Hel"), "SGVs");
    assert_eq!(ok(Transform::Base64Encode, ""), "");
    assert_eq!(ok(Transform::Base64Decode, "SGVsbG8="), "Hello");
    assert_eq!(ok(Transform::Base64Decode, "SGVs\nbG8=\r\n"), "Hello");
    assert_eq!(ok(Transform::Base64Decode, "SGVsbG8"), "Hello");
    assert_eq!(ok(Transform::Base64Decode, ""), "");
    let s = "Xin chào Việt Nam 😀\r\n";
    let enc = ok(Transform::Base64Encode, s);
    assert_eq!(ok(Transform::Base64Decode, &enc), s);
    for bad in ["SGV$", "SGVsb", "=SGVs", "SG=s", "SGVsbG8==="] {
        assert_eq!(
            err(Transform::Base64Decode, bad),
            TransformError::InvalidBase64,
            "{bad:?}"
        );
    }
    // Valid Base64 whose bytes are not UTF-8.
    assert_eq!(
        err(Transform::Base64Decode, "/w=="),
        TransformError::NotUtf8
    );
}

#[test]
fn html_to_text_basic() {
    assert_eq!(
        ok(
            Transform::HtmlToText,
            "<p>Hello <b>world</b></p><ul><li>one</li><li>two</li></ul>"
        ),
        "Hello world\none\ntwo"
    );
    assert_eq!(
        ok(Transform::HtmlToText, "a<br>b<br/>c<BR />d"),
        "a\nb\nc\nd"
    );
    assert_eq!(
        ok(
            Transform::HtmlToText,
            "<div>a &amp; b &lt;c&gt; &quot;q&quot; &#39;s&#39; &#x41;&nbsp;z &amp;lt; &foo;</div>"
        ),
        "a & b <c> \"q\" 's' A z &lt; &foo;"
    );
    assert_eq!(
        ok(Transform::HtmlToText, "<a title=\"a>b\">link</a>"),
        "link"
    );
    assert_eq!(ok(Transform::HtmlToText, "<!-- hidden -->shown"), "shown");
    assert_eq!(
        ok(Transform::HtmlToText, "<p>Xin chào 😀</p>\n  <p>Việt</p>"),
        "Xin chào 😀\nViệt"
    );
    assert_eq!(ok(Transform::HtmlToText, ""), "");
    assert_eq!(
        ok(Transform::HtmlToText, "plain  text\n  here"),
        "plain text here"
    );
}

#[test]
fn html_script_and_style_content_dropped() {
    let secret = concat!("SCRIPT", "_BODY_TEXT");
    let html = format!(
        "x<script>{secret}</script>y<style>.k{{color:red}}</style>z<SCRIPT type=\"a\">{secret}</SCRIPT>w"
    );
    let out = ok(Transform::HtmlToText, &html);
    assert_eq!(out, "xyzw");
    assert!(!out.contains(secret));
    assert!(!out.contains("color"));
    // Unterminated: the rest is dropped, never shown.
    let out = ok(Transform::HtmlToText, &format!("a<script>{secret}"));
    assert_eq!(out, "a");
    let out = ok(
        Transform::HtmlToText,
        &format!("<script>var a='{secret}';</script>ok"),
    );
    assert_eq!(out, "ok");
}

#[test]
fn errors_carry_no_content() {
    let secret = concat!("TOPSECRET", "_VALUE");
    for t in Transform::ALL {
        if let Err(e) = t.apply(&format!("{secret}%zz{{")) {
            assert!(!format!("{e:?}{e}").contains(secret));
        }
    }
}

#[test]
fn output_is_capped_like_the_input() {
    // Deep pretty-print: 127 levels of indent for every element would be about 128 MiB.
    let doc = format!(
        "{}{}0{}",
        "[".repeat(127),
        "0,".repeat(400_000),
        "]".repeat(127)
    );
    assert!(doc.len() <= MAX_INPUT_BYTES);
    assert_eq!(err(Transform::JsonPretty, &doc), TransformError::TooLarge);
    // Growth of the other transforms, each from an input within the cap.
    let n_hat = "\u{149}".repeat(500_000); // upper case: 2 bytes -> 3 bytes
    assert_eq!(err(Transform::Upper, &n_hat), TransformError::TooLarge);
    let i_dot = "\u{130}".repeat(500_000); // lower case: 2 bytes -> 3 bytes
    assert_eq!(err(Transform::Lower, &i_dot), TransformError::TooLarge);
    assert_eq!(
        err(Transform::UrlEncode, &"é".repeat(400_000)),
        TransformError::TooLarge
    );
    assert_eq!(
        err(Transform::Base64Encode, &"a".repeat(800_000)),
        TransformError::TooLarge
    );
    assert_eq!(
        err(Transform::LineEndingsCrlf, &"\n".repeat(600_000)),
        TransformError::TooLarge
    );
    // At the limit is fine.
    assert!(Transform::UrlEncode
        .apply(&"a".repeat(MAX_INPUT_BYTES))
        .is_ok());
}

#[test]
fn html_entities_next_to_multibyte_characters() {
    assert_eq!(ok(Transform::HtmlToText, "&nbsp;Đà Nẵng"), "Đà Nẵng");
    assert_eq!(ok(Transform::HtmlToText, "&amp;ệệệ"), "&ệệệ");
    assert_eq!(ok(Transform::HtmlToText, "ệệệ&amp;ệệệ"), "ệệệ&ệệệ");
    assert_eq!(
        ok(Transform::HtmlToText, "&notanentityệệệệệ;"),
        "&notanentityệệệệệ;"
    );
}
