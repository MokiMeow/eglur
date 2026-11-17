use eglur_hir::{HirFile, ItemKind, OriginId, SourceOrigin, TypeRefId, TypeRefKind, lower};
use eglur_source::{FileId, FileSpan, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, SyntaxElement, SyntaxKind, parse};

#[path = "type_fields/recovery.rs"]
mod recovery;
#[path = "type_fields/structure.rs"]
mod structure;

struct LoweredTypes {
    parsed: Parse,
    hir: HirFile,
}

impl LoweredTypes {
    fn new(text: &str) -> Self {
        let source = SourceFile::new(FileId::new(23), text.as_bytes().to_vec());
        let FrontendOutcome::Parsed(parsed) = parse(&source) else {
            panic!("UTF-8 source should reach the parser");
        };
        assert_eq!(parsed.syntax().text(), text);
        let hir = lower(&parsed);
        Self { parsed, hir }
    }

    fn valid_type(spelling: &str) -> Self {
        let source = Self::new(&format!("type Selected = {spelling}\n"));
        assert!(
            source.parsed.is_success(),
            "{spelling}: {:?}",
            source.parsed.diagnostics(),
        );
        assert!(source.hir.invalid_occurrences().is_empty());
        assert_eq!(source.hir.roots().len(), 1);
        source
    }

    fn alias(&self, name: &str) -> TypeRefId {
        self.hir
            .roots()
            .iter()
            .find_map(|&id| match self.hir.item(id).kind() {
                ItemKind::TypeAlias(alias) if alias.name.text() == Some(name) => Some(alias.value),
                _ => None,
            })
            .expect("source should retain the selected type alias")
    }

    fn kind(&self, id: TypeRefId, syntax: SyntaxKind, spelling: &str) -> &TypeRefKind {
        let ty = self.hir.type_ref(id);
        self.written(ty.origin(), syntax, spelling);
        ty.kind()
    }

    fn path(&self, id: TypeRefId, spelling: &str, path_spelling: &str) -> &[TypeRefId] {
        let TypeRefKind::Path { path, arguments } = self.kind(id, SyntaxKind::PathType, spelling)
        else {
            panic!("expected path type {spelling}");
        };
        assert_eq!(path.text().as_deref(), Some(path_spelling));
        let names: Vec<_> = path_spelling.split("::").collect();
        assert_eq!(path.segments().len(), names.len());
        for (name, spelling) in path.segments().iter().zip(names) {
            assert_eq!(name.text(), Some(spelling));
            assert!(!name.is_bang());
            self.written(name.origin(), SyntaxKind::Identifier, spelling);
        }

        let SourceOrigin::Written(span) = self.hir.origin(path.origin()) else {
            panic!("path must retain its written origin");
        };
        assert_eq!(span.file(), self.hir.file());
        assert_eq!(self.spelling(span), path_spelling);
        arguments
    }

    fn leaf(&self, id: TypeRefId, spelling: &str) {
        assert!(self.path(id, spelling, spelling).is_empty());
    }

    fn spelling(&self, span: FileSpan) -> String {
        assert_eq!(span.file(), self.hir.file());
        let text = self.parsed.syntax().text();
        text[span.range().start().as_usize()..span.range().end().as_usize()].to_owned()
    }

    fn written(&self, origin: OriginId, kind: SyntaxKind, spelling: &str) {
        let mut spans = Vec::new();
        let mut pending = vec![self.parsed.syntax()];
        while let Some(node) = pending.pop() {
            if node.kind() == kind && node.text().trim() == spelling {
                spans.push(node.span());
            }
            for element in node.children_with_tokens() {
                match element {
                    SyntaxElement::Node(child) => pending.push(child),
                    SyntaxElement::Token(token)
                        if token.kind() == kind && token.text().trim() == spelling =>
                    {
                        spans.push(token.span());
                    }
                    SyntaxElement::Token(_) => {}
                }
            }
        }

        let span = *spans.first().expect("expected syntax occurrence");
        assert!(
            spans.iter().all(|&candidate| candidate == span),
            "unique {kind:?} source range: {spelling}",
        );
        assert_eq!(span.file(), self.hir.file());
        assert_eq!(self.hir.origin(origin), SourceOrigin::Written(span));
    }
}
