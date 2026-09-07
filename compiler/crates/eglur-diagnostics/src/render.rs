use std::fmt::Write;

use crate::{Diagnostic, DiagnosticSnapshot, PrimarySpan, SourceSpan};

/// Renders one finalized snapshot using the normative plain-text field order.
#[must_use]
pub fn render_plain(snapshot: &DiagnosticSnapshot) -> String {
    let mut rendered = String::new();
    for (index, diagnostic) in snapshot.diagnostics().iter().enumerate() {
        if index > 0 {
            rendered.push('\n');
        }
        render_diagnostic(&mut rendered, diagnostic);
    }
    rendered
}

fn render_diagnostic(rendered: &mut String, diagnostic: &Diagnostic) {
    write!(
        rendered,
        "{}[{}] {}",
        diagnostic.severity(),
        diagnostic.code(),
        diagnostic.message(),
    )
    .expect("writing to a String cannot fail");

    if let PrimarySpan::Source(span) = diagnostic.primary_span() {
        rendered.push('\n');
        rendered.push_str(" --> ");
        render_primary_location(rendered, span);
    }
    if let Some(help) = diagnostic.help() {
        write!(rendered, "\n = help: {help}").expect("writing to a String cannot fail");
    }
    for (key, value) in diagnostic.data() {
        write!(rendered, "\n = {key}: {value}").expect("writing to a String cannot fail");
    }
    for related in diagnostic.related() {
        write!(rendered, "\n = related: {}\n   --> ", related.label())
            .expect("writing to a String cannot fail");
        render_related_location(rendered, related.span());
    }
}

fn render_primary_location(rendered: &mut String, span: &SourceSpan) {
    if let Some(text) = span.text() {
        write!(
            rendered,
            "{}:{}:{}",
            span.source(),
            text.start().line(),
            text.start().column(),
        )
        .expect("writing to a String cannot fail");
    } else {
        render_byte_location(rendered, span);
    }
}

fn render_related_location(rendered: &mut String, span: &SourceSpan) {
    if let Some(text) = span.text() {
        write!(
            rendered,
            "{}:{}:{}-{}:{}",
            span.source(),
            text.start().line(),
            text.start().column(),
            text.end().line(),
            text.end().column(),
        )
        .expect("writing to a String cannot fail");
    } else {
        render_byte_location(rendered, span);
    }
}

fn render_byte_location(rendered: &mut String, span: &SourceSpan) {
    let bytes = span.file_span().range();
    write!(
        rendered,
        "{}:bytes[{}..{})",
        span.source(),
        bytes.start().get(),
        bytes.end().get(),
    )
    .expect("writing to a String cannot fail");
}
