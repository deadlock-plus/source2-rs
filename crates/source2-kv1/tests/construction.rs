//! Built as an external crate, so `#[non_exhaustive]` structs cannot be built with literals.

use source2_kv1::{
    ConditionAt, Directive, Document, DocumentLayout, Encoding, Entry, Layout, LineEnding, Options,
    Quote, Spelling, Trivia, Value,
};

#[test]
fn options_build_and_edit() {
    let built = Options::new()
        .with_escape_sequences(false)
        .with_max_depth(16);
    assert!(!built.escape_sequences);
    assert_eq!(built.max_depth, 16);
    assert_eq!(Options::new(), Options::default());

    let mut edited = Options::default();
    edited.max_depth = 4;
    assert_eq!(edited.max_depth, 4);

    let doc = Document::parse_with(r#""P" { "d" "C:\temp" }"#, &built).unwrap();
    assert_eq!(doc.get("P").unwrap().get_str("d"), Some(r"C:\temp"));
}

#[test]
fn spelling_builds() {
    let s = Spelling::new().with_quote(Quote::Bare).with_raw(r"a\?b");
    assert_eq!(s.quote, Quote::Bare);
    assert_eq!(s.raw.as_deref(), Some(r"a\?b"));
    assert_eq!(Spelling::new(), Spelling::default());
}

#[test]
fn layout_builds() {
    let l = Layout::new()
        .with_key(Spelling::new().with_quote(Quote::Bare))
        .with_value(Spelling::new().with_quote(Quote::Bare))
        .with_condition_raw(" $X ")
        .with_condition_at(ConditionAt::AfterKey)
        .with_leading(vec![Trivia::Newline, Trivia::Indent])
        .with_key_gap(vec![Trivia::space(" ")])
        .with_condition_gap(vec![Trivia::space(" ")])
        .with_before_close(vec![Trivia::Newline])
        .with_trailing(vec![Trivia::space(" "), Trivia::comment("t")]);
    assert_eq!(l.key.quote, Quote::Bare);
    assert_eq!(l.condition_at, Some(ConditionAt::AfterKey));
    assert_eq!(l.trailing_comment(), Some("t"));
    assert_eq!(Layout::new(), Layout::default());
}

#[test]
fn document_layout_builds() {
    let l = DocumentLayout::new()
        .with_bom(true)
        .with_line_ending(LineEnding::CrLf)
        .with_leading(vec![Trivia::comment("h"), Trivia::Newline])
        .with_trailing(vec![])
        .with_end_marker(false);
    assert!(l.bom && !l.end_marker);
    assert_eq!(l.line_ending, LineEnding::CrLf);
    assert_eq!(l.trailing, Some(vec![]));
    assert_eq!(DocumentLayout::new(), DocumentLayout::default());
}

#[test]
fn document_entry_directive_build() {
    let entry = Entry::string("k", "v").with_layout(Layout::new().with_key(Spelling::new()));
    let dir = Directive::include("a.vdf").with_layout(Layout::new());
    let doc = Document::new(vec![entry])
        .with_directive(dir)
        .with_escapes(false)
        .with_encoding(Encoding::Utf8)
        .with_layout(DocumentLayout::new().with_line_ending(LineEnding::CrLf));
    assert!(!doc.escapes);
    let text = doc.to_text().unwrap();
    assert!(text.contains("\r\n"), "{text:?}");
    assert_eq!(doc.get("k").unwrap().value, Value::String("v".into()));
}

#[test]
fn model_structs_edit_through_fields() {
    let mut doc = Document::default();
    doc.escapes = false;
    doc.layout.bom = true;
    let mut e = Entry::string("a", r"a?b");
    e.condition = Some("$X".into());
    doc.roots.push(e);
    assert!(doc.to_text().unwrap().starts_with('\u{feff}'));
}

#[test]
fn trailing_nul_builder() {
    use source2_kv1::DocumentLayout;
    assert!(!DocumentLayout::new().trailing_nul);
    assert!(DocumentLayout::new().with_trailing_nul(true).trailing_nul);
}
