use std::fmt;

use eglur_source::{ByteOffset, ByteRange, FileId, FileSpan};
use rowan::{GreenNode, GreenNodeBuilder, Language, NodeOrToken};

use crate::event::Event;
use crate::syntax_kind::ALL_SYNTAX_KINDS;
use crate::{Lexed, SyntaxKind};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum EglurLanguage {}

impl Language for EglurLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        ALL_SYNTAX_KINDS
            .get(usize::from(raw.0))
            .copied()
            .unwrap_or(SyntaxKind::ErrorNode)
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GreenTree(GreenNode);

impl GreenTree {
    pub(crate) fn root(&self, file: FileId) -> SyntaxNode {
        SyntaxNode {
            file,
            raw: rowan::SyntaxNode::new_root(self.0.clone()),
        }
    }
}

/// One immutable node view over the lossless syntax tree.
#[derive(Clone)]
pub struct SyntaxNode {
    file: FileId,
    raw: rowan::SyntaxNode<EglurLanguage>,
}

impl SyntaxNode {
    /// Returns the Eglur-owned node kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.raw.kind()
    }

    /// Returns the source-bound range covered by this node.
    #[must_use]
    pub fn span(&self) -> FileSpan {
        FileSpan::new(self.file, byte_range(self.raw.text_range()))
    }

    /// Reconstructs the exact source text covered by this node.
    #[must_use]
    pub fn text(&self) -> String {
        self.raw.text().to_string()
    }

    /// Iterates direct child nodes without exposing rowan types.
    pub fn children(&self) -> impl Iterator<Item = Self> + '_ {
        let file = self.file;
        self.raw.children().map(move |raw| Self { file, raw })
    }

    /// Iterates direct child nodes and tokens in source order.
    pub fn children_with_tokens(&self) -> impl Iterator<Item = SyntaxElement> + '_ {
        let file = self.file;
        self.raw
            .children_with_tokens()
            .map(move |element| match element {
                NodeOrToken::Node(raw) => SyntaxElement::Node(Self { file, raw }),
                NodeOrToken::Token(raw) => SyntaxElement::Token(SyntaxToken { file, raw }),
            })
    }
}

impl fmt::Debug for SyntaxNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxNode")
            .field("kind", &self.kind())
            .field("span", &self.span())
            .finish_non_exhaustive()
    }
}

/// One immutable token view over the lossless syntax tree.
#[derive(Clone)]
pub struct SyntaxToken {
    file: FileId,
    raw: rowan::SyntaxToken<EglurLanguage>,
}

impl SyntaxToken {
    /// Returns the Eglur-owned token kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.raw.kind()
    }

    /// Returns the exact physical token text; missing tokens have empty text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.raw.text()
    }

    /// Returns the source-bound token range.
    #[must_use]
    pub fn span(&self) -> FileSpan {
        FileSpan::new(self.file, byte_range(self.raw.text_range()))
    }

    /// Reports whether this is a parser-inserted missing token.
    #[must_use]
    pub fn is_missing(&self) -> bool {
        self.raw.text().is_empty()
    }
}

impl fmt::Debug for SyntaxToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxToken")
            .field("kind", &self.kind())
            .field("span", &self.span())
            .field("text", &self.text())
            .finish()
    }
}

/// One direct syntax-tree child without a dependency on rowan's public API.
#[derive(Clone, Debug)]
pub enum SyntaxElement {
    Node(SyntaxNode),
    Token(SyntaxToken),
}

pub(crate) fn build_tree(lexed: &Lexed<'_>, mut events: Vec<Event>) -> GreenTree {
    let mut builder = GreenNodeBuilder::new();
    let mut physical_index = 0;
    let final_event = events.len().saturating_sub(1);

    for index in 0..events.len() {
        match std::mem::replace(&mut events[index], Event::Tombstone) {
            Event::Tombstone => {}
            Event::Start {
                kind,
                forward_parent,
            } => {
                let mut kinds = vec![kind];
                let mut cursor = index;
                let mut next = forward_parent;
                while let Some(distance) = next {
                    cursor = cursor
                        .checked_add(distance)
                        .expect("forward-parent distance should fit event storage");
                    match std::mem::replace(&mut events[cursor], Event::Tombstone) {
                        Event::Start {
                            kind,
                            forward_parent,
                        } => {
                            kinds.push(kind);
                            next = forward_parent;
                        }
                        _ => panic!("forward parent must reference a start event"),
                    }
                }
                for kind in kinds.into_iter().rev() {
                    builder.start_node(EglurLanguage::kind_to_raw(kind));
                }
            }
            Event::Finish => {
                if index == final_event {
                    emit_through(
                        lexed,
                        &mut builder,
                        &mut physical_index,
                        lexed.tokens().len(),
                    );
                }
                builder.finish_node();
            }
            Event::Token {
                physical_index: target,
            } => emit_through(lexed, &mut builder, &mut physical_index, target + 1),
            Event::Missing { kind } => {
                builder.token(EglurLanguage::kind_to_raw(kind), "");
            }
        }
    }

    GreenTree(builder.finish())
}

pub(crate) fn error_tree_events(lexed: &Lexed<'_>) -> Vec<Event> {
    let mut events = Vec::with_capacity(lexed.tokens().len().saturating_add(4));
    events.push(Event::Start {
        kind: SyntaxKind::Root,
        forward_parent: None,
    });
    events.push(Event::Start {
        kind: SyntaxKind::ErrorNode,
        forward_parent: None,
    });
    events.extend(
        lexed
            .tokens()
            .iter()
            .enumerate()
            .filter(|(_, token)| !token.kind().is_horizontal_trivia())
            .map(|(physical_index, _)| Event::Token { physical_index }),
    );
    events.push(Event::Finish);
    events.push(Event::Finish);
    events
}

fn emit_through(
    lexed: &Lexed<'_>,
    builder: &mut GreenNodeBuilder<'_>,
    physical_index: &mut usize,
    end: usize,
) {
    while *physical_index < end {
        let token = lexed.tokens()[*physical_index];
        let text = lexed
            .text(token)
            .expect("tree sink token should belong to its lexical buffer");
        builder.token(
            EglurLanguage::kind_to_raw(SyntaxKind::from_token(token.kind())),
            text,
        );
        *physical_index += 1;
    }
}

fn byte_range(range: rowan::TextRange) -> ByteRange {
    ByteRange::new(
        ByteOffset::new(u32::from(range.start())),
        ByteOffset::new(u32::from(range.end())),
    )
    .expect("rowan ranges are ordered")
}
