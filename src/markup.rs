//! Minimal span markup for module output: `<span class='a b'>text</span>`,
//! nestable, with `&lt; &gt; &amp; &quot; &apos;` entities. Only modules that
//! opt in to `"output": "json"` pass through here; plain-text modules use
//! `plain` and are never parsed.

/// One styled piece of a line: its text and the class names that apply to it,
/// outermost first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub classes: Vec<String>,
}

/// A displayed line is a sequence of segments. An empty vec is a blank line.
pub type Line = Vec<Segment>;

/// Plain text: one classless segment per line, blank lines stay empty.
pub fn plain(text: &str) -> Vec<Line> {
    text.lines()
        .map(|l| {
            if l.is_empty() {
                Vec::new()
            } else {
                vec![Segment {
                    text: l.to_string(),
                    classes: Vec::new(),
                }]
            }
        })
        .collect()
}

/// Parse markup into lines of segments. `Ok` carries the lines plus warnings
/// (an unclosed span is auto-closed and warned about, not fatal). Hard errors
/// (unknown tag, bad attribute, stray `</span>`, bad entity) return `Err`, and
/// the caller falls back to showing the raw text via `plain`.
pub fn parse(text: &str) -> Result<(Vec<Line>, Vec<String>), String> {
    let mut lines: Vec<Line> = Vec::new();
    let mut line: Line = Vec::new();
    let mut buf = String::new();
    // Class frames of the currently open spans, outermost first.
    let mut stack: Vec<Vec<String>> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => {
                flush(&mut buf, &mut line, &stack);
                lines.push(std::mem::take(&mut line));
            }
            '&' => buf.push(entity(&mut chars)?),
            '<' => {
                let tag = read_tag(&mut chars)?;
                flush(&mut buf, &mut line, &stack);
                if tag.trim() == "/span" {
                    if stack.pop().is_none() {
                        return Err("</span> without matching <span>".to_string());
                    }
                } else if tag == "span" {
                    stack.push(Vec::new());
                } else if let Some(rest) = tag.strip_prefix("span ") {
                    stack.push(span_classes(rest)?);
                } else {
                    return Err(format!("unknown tag <{tag}>"));
                }
            }
            _ => buf.push(c),
        }
    }
    flush(&mut buf, &mut line, &stack);
    if !stack.is_empty() {
        warnings.push("unclosed <span> auto-closed at end of output".to_string());
    }
    if !line.is_empty() {
        lines.push(line);
    }
    Ok((lines, warnings))
}

/// Decode `&name;` after the `&` has been consumed. Longest entity is 4 chars.
fn entity(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<char, String> {
    let mut name = String::new();
    loop {
        match chars.next() {
            Some(';') => break,
            Some(c) if name.len() < 4 => name.push(c),
            Some(_) => return Err("bare '&' (use &amp;)".to_string()),
            None => return Err("bare '&' at end of text (use &amp;)".to_string()),
        }
    }
    match name.as_str() {
        "lt" => Ok('<'),
        "gt" => Ok('>'),
        "amp" => Ok('&'),
        "quot" => Ok('"'),
        "apos" => Ok('\''),
        other => Err(format!("unknown entity '&{other};'")),
    }
}

/// End the text run in `buf` as a segment carrying the open spans' classes.
fn flush(buf: &mut String, line: &mut Line, stack: &[Vec<String>]) {
    if buf.is_empty() {
        return;
    }
    line.push(Segment {
        text: std::mem::take(buf),
        classes: stack.iter().flatten().cloned().collect(),
    });
}

/// Consume up to and including `>`, returning the tag's inner text.
fn read_tag(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<String, String> {
    let mut tag = String::new();
    for c in chars.by_ref() {
        if c == '>' {
            return Ok(tag);
        }
        tag.push(c);
    }
    Err("'<' without a closing '>'".to_string())
}

/// Parse the attribute part of `<span ...>`: only `class='a b'`, either quote.
fn span_classes(rest: &str) -> Result<Vec<String>, String> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Ok(Vec::new());
    }
    let value = rest
        .strip_prefix("class")
        .map(str::trim_start)
        .and_then(|s| s.strip_prefix('='))
        .map(str::trim_start)
        .ok_or_else(|| format!("expected class attribute in <span {rest}>"))?;
    let inner = value
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .or_else(|| value.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
        .ok_or_else(|| format!("class value must be quoted in <span {rest}>"))?;
    Ok(inner.split_whitespace().map(str::to_string).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, classes: &[&str]) -> Segment {
        Segment {
            text: text.to_string(),
            classes: classes.iter().map(|c| c.to_string()).collect(),
        }
    }

    #[test]
    fn plain_maps_each_line_to_one_classless_segment() {
        assert_eq!(
            plain("CPU 12%\n42 C"),
            vec![vec![seg("CPU 12%", &[])], vec![seg("42 C", &[])]]
        );
    }

    #[test]
    fn plain_keeps_blank_lines_empty() {
        assert_eq!(
            plain("a\n\nb"),
            vec![vec![seg("a", &[])], vec![], vec![seg("b", &[])]]
        );
    }

    #[test]
    fn plain_empty_string_has_no_lines() {
        assert_eq!(plain(""), Vec::<Line>::new());
    }

    #[test]
    fn plain_never_parses_markup_or_entities() {
        assert_eq!(
            plain("<span class='x'>&amp;</span>"),
            vec![vec![seg("<span class='x'>&amp;</span>", &[])]]
        );
    }

    fn ok(text: &str) -> Vec<Line> {
        let (lines, warnings) = parse(text).expect("markup should parse");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        lines
    }

    #[test]
    fn parses_text_without_markup() {
        assert_eq!(ok("MEM 92%"), vec![vec![seg("MEM 92%", &[])]]);
    }

    #[test]
    fn parses_single_span() {
        assert_eq!(
            ok("<span class='title'>MEM</span>"),
            vec![vec![seg("MEM", &["title"])]]
        );
    }

    #[test]
    fn parses_text_around_a_span() {
        assert_eq!(
            ok("use <span class='critical'>92%</span> now"),
            vec![vec![
                seg("use ", &[]),
                seg("92%", &["critical"]),
                seg(" now", &[])
            ]]
        );
    }

    #[test]
    fn multiple_classes_split_on_whitespace() {
        assert_eq!(
            ok("<span class='big red'>!</span>"),
            vec![vec![seg("!", &["big", "red"])]]
        );
    }

    #[test]
    fn nested_spans_accumulate_outer_to_inner() {
        assert_eq!(
            ok("<span class='a'>x<span class='b'>y</span>z</span>"),
            vec![vec![
                seg("x", &["a"]),
                seg("y", &["a", "b"]),
                seg("z", &["a"])
            ]]
        );
    }

    #[test]
    fn span_with_no_class_is_allowed() {
        assert_eq!(ok("<span>x</span>"), vec![vec![seg("x", &[])]]);
    }

    #[test]
    fn newline_splits_lines() {
        assert_eq!(ok("a\nb"), vec![vec![seg("a", &[])], vec![seg("b", &[])]]);
    }

    #[test]
    fn span_continues_across_newline() {
        assert_eq!(
            ok("<span class='c'>a\nb</span>"),
            vec![vec![seg("a", &["c"])], vec![seg("b", &["c"])]]
        );
    }

    #[test]
    fn blank_lines_and_trailing_newline_match_plain() {
        assert_eq!(ok("a\n\nb"), plain("a\n\nb"));
        assert_eq!(ok("a\n"), plain("a\n"));
        assert_eq!(ok(""), Vec::<Line>::new());
    }

    #[test]
    fn stray_close_is_an_error() {
        assert!(parse("x</span>").is_err());
    }

    #[test]
    fn unknown_tag_is_an_error() {
        assert!(parse("<b>x</b>").is_err());
        assert!(parse("<spanx>y</spanx>").is_err());
    }

    #[test]
    fn unterminated_tag_is_an_error() {
        assert!(parse("a <span class='x'").is_err());
    }

    #[test]
    fn entities_decode() {
        assert_eq!(
            ok("&lt;3 &amp; &gt;4 &quot;q&quot; &apos;a&apos;"),
            vec![vec![seg("<3 & >4 \"q\" 'a'", &[])]]
        );
    }

    #[test]
    fn entities_decode_inside_spans() {
        assert_eq!(
            ok("<span class='x'>&lt;ok&gt;</span>"),
            vec![vec![seg("<ok>", &["x"])]]
        );
    }

    #[test]
    fn bare_ampersand_is_an_error() {
        assert!(parse("Tom & Jerry").is_err());
        assert!(parse("bad &nope; here").is_err());
    }

    #[test]
    fn double_quoted_class_works() {
        assert_eq!(
            ok("<span class=\"title\">M</span>"),
            vec![vec![seg("M", &["title"])]]
        );
    }

    #[test]
    fn non_class_attribute_is_an_error() {
        assert!(parse("<span style='color:red'>x</span>").is_err());
        assert!(parse("<span class=title>x</span>").is_err());
    }

    #[test]
    fn unclosed_span_auto_closes_with_warning() {
        let (lines, warnings) = parse("<span class='c'>oops").expect("tolerated");
        assert_eq!(lines, vec![vec![seg("oops", &["c"])]]);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unclosed"));
    }
}
