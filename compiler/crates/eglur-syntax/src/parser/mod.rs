mod declaration;
mod expression;
mod pattern;
mod tensor;
mod types;

use eglur_source::{ByteRange, FileSpan};

use crate::event::{CompletedMarker, Event, Marker};
use crate::tree::error_tree_events;
use crate::{Lexed, ParseResource, SyntaxDiagnostic, SyntaxDiagnosticKind, SyntaxKind, TokenKind};

pub(crate) const MAX_PARSE_DEPTH: u16 = 256;
const MAX_DELIMITER_DEPTH: usize = 256;
pub(crate) const MAX_SYNTAX_DIAGNOSTICS: usize = 32;
// The per-token factor bounds linear growth; the base covers bounded unwind markers.
const EVENTS_PER_SIGNIFICANT_TOKEN: usize = 64;
const BASE_EVENT_BUDGET: usize = 1_024;

#[derive(Clone, Copy, Debug)]
struct ParserLimits {
    recursive_depth: u16,
    delimiter_depth: usize,
    syntax_diagnostics: usize,
    events: usize,
}

impl ParserLimits {
    fn for_tokens(significant_tokens: usize) -> Self {
        Self {
            recursive_depth: MAX_PARSE_DEPTH,
            delimiter_depth: MAX_DELIMITER_DEPTH,
            syntax_diagnostics: MAX_SYNTAX_DIAGNOSTICS,
            events: significant_tokens
                .saturating_mul(EVENTS_PER_SIGNIFICANT_TOKEN)
                .saturating_add(BASE_EVENT_BUDGET),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ResourceViolation {
    resource: ParseResource,
    limit: i64,
    observed_at_least: i64,
    position: usize,
}

#[derive(Debug)]
struct DelimiterIndex {
    matching: Box<[Option<usize>]>,
    trusted_top_level: Box<[bool]>,
    limit_owners: Box<[bool]>,
    violation: Option<ResourceViolation>,
}

impl DelimiterIndex {
    fn build(lexed: &Lexed<'_>, significant: &[usize], delimiter_limit: usize) -> Self {
        let mut matching = vec![None; significant.len()];
        let mut trusted_top_level = vec![false; significant.len()];
        let mut limit_owners = vec![false; significant.len()];
        let mut stack = Vec::with_capacity(delimiter_limit.min(significant.len()));
        let mut trusted = true;
        let mut violation = None;

        for position in 0..significant.len() {
            trusted_top_level[position] = trusted && stack.is_empty();
            let kind = lexed.tokens()[significant[position]].kind();

            if let Some(closing) = closing_delimiter(kind) {
                if trusted && stack.len() >= delimiter_limit {
                    if violation.is_none() {
                        for &(_, opening) in &stack {
                            limit_owners[opening] = true;
                        }
                        limit_owners[position] = true;
                        violation = Some(ResourceViolation {
                            resource: ParseResource::DelimiterDepth,
                            limit: to_i64(delimiter_limit),
                            observed_at_least: to_i64(stack.len().saturating_add(1)),
                            position,
                        });
                    }
                    trusted = false;
                } else if trusted {
                    stack.push((closing, position));
                }
                continue;
            }

            if is_closing_delimiter(kind) && trusted {
                match stack.pop() {
                    Some((expected, opening)) if expected == kind => {
                        matching[opening] = Some(position);
                    }
                    _ => {
                        stack.clear();
                        trusted = false;
                    }
                }
            }
        }

        Self {
            matching: matching.into_boxed_slice(),
            trusted_top_level: trusted_top_level.into_boxed_slice(),
            limit_owners: limit_owners.into_boxed_slice(),
            violation,
        }
    }

    fn matching(&self, opening: usize) -> Option<usize> {
        self.matching.get(opening).copied().flatten()
    }

    fn is_trusted_top_level(&self, position: usize) -> bool {
        self.trusted_top_level
            .get(position)
            .copied()
            .unwrap_or(false)
    }

    fn violation_at(&self, position: usize) -> Option<ResourceViolation> {
        self.violation
            .filter(|violation| violation.position == position)
    }

    fn violation_for_opening(&self, opening: usize) -> Option<ResourceViolation> {
        self.limit_owners
            .get(opening)
            .copied()
            .unwrap_or(false)
            .then_some(self.violation)
            .flatten()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParseEntryPoint {
    CompilationUnit,
    #[cfg(test)]
    Expression,
    #[cfg(test)]
    ExpressionSequence,
    #[cfg(test)]
    ControlHead,
}

pub(crate) fn parse_lexed(
    lexed: &Lexed<'_>,
    entry: ParseEntryPoint,
) -> (Vec<Event>, Box<[SyntaxDiagnostic]>) {
    Parser::new(lexed).parse(entry)
}

pub(super) struct Parser<'source> {
    lexed: &'source Lexed<'source>,
    significant: Box<[usize]>,
    delimiters: DelimiterIndex,
    limits: ParserLimits,
    position: usize,
    events: Vec<Event>,
    diagnostics: Vec<SyntaxDiagnostic>,
    halted: bool,
    region_has_diagnostic: bool,
    terminal: bool,
    depth: u16,
    placeholder_hole: Option<bool>,
}

impl<'source> Parser<'source> {
    fn new(lexed: &'source Lexed<'source>) -> Self {
        let significant = significant_tokens(lexed);
        let limits = ParserLimits::for_tokens(significant.len());
        Self::with_limits_and_significant(lexed, significant, limits)
    }

    #[cfg(test)]
    fn with_limits(lexed: &'source Lexed<'source>, limits: ParserLimits) -> Self {
        Self::with_limits_and_significant(lexed, significant_tokens(lexed), limits)
    }

    fn with_limits_and_significant(
        lexed: &'source Lexed<'source>,
        significant: Box<[usize]>,
        limits: ParserLimits,
    ) -> Self {
        let delimiters = DelimiterIndex::build(lexed, &significant, limits.delimiter_depth);
        Self {
            lexed,
            significant,
            delimiters,
            limits,
            position: 0,
            events: Vec::new(),
            diagnostics: Vec::new(),
            halted: false,
            region_has_diagnostic: false,
            terminal: false,
            depth: 0,
            placeholder_hole: None,
        }
    }

    fn parse(mut self, entry: ParseEntryPoint) -> (Vec<Event>, Box<[SyntaxDiagnostic]>) {
        let root = self.start();
        if self.terminal {
            return self.fallback_output();
        }
        match entry {
            ParseEntryPoint::CompilationUnit => self.parse_compilation_unit(),
            #[cfg(test)]
            ParseEntryPoint::Expression => {
                self.skip_newlines();
                self.parse_expression(false, true);
                self.skip_newlines();
                self.require_eof();
            }
            #[cfg(test)]
            ParseEntryPoint::ExpressionSequence => self.parse_expression_sequence(),
            #[cfg(test)]
            ParseEntryPoint::ControlHead => {
                self.skip_newlines();
                self.parse_control_construct();
                self.skip_newlines();
                self.require_eof();
            }
        }
        if self.terminal {
            return self.fallback_output();
        }
        if self.halted {
            self.drain_error();
        }
        if self.terminal {
            return self.fallback_output();
        }
        self.complete(root, SyntaxKind::Root);
        if self.terminal {
            return self.fallback_output();
        }
        (self.events, self.diagnostics.into_boxed_slice())
    }

    fn fallback_output(self) -> (Vec<Event>, Box<[SyntaxDiagnostic]>) {
        (
            error_tree_events(self.lexed),
            self.diagnostics.into_boxed_slice(),
        )
    }

    #[cfg(test)]
    fn parse_expression_sequence(&mut self) {
        self.skip_newlines();
        while !self.at_eof() && !self.halted {
            self.parse_expression(false, true);
            if self.at_eof() {
                break;
            }
            if !self.at(TokenKind::Newline) {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                break;
            }
            self.skip_newlines();
        }
    }

    pub(super) fn start(&mut self) -> Marker {
        self.emit_event(Event::Tombstone)
            .map_or_else(Marker::inactive, Marker::new)
    }

    pub(super) fn complete(&mut self, marker: Marker, kind: SyntaxKind) -> CompletedMarker {
        let Some(position) = marker.position() else {
            return CompletedMarker::inactive();
        };
        if self.terminal || !matches!(self.events.get(position), Some(Event::Tombstone)) {
            self.exhaust_event_budget();
            return CompletedMarker::inactive();
        }
        if self.emit_event(Event::Finish).is_none() {
            return CompletedMarker::inactive();
        }
        self.events[position] = Event::Start {
            kind,
            forward_parent: None,
        };
        CompletedMarker::new(position)
    }

    pub(super) fn precede(&mut self, completed: CompletedMarker) -> Marker {
        let Some(completed_position) = completed.position() else {
            return Marker::inactive();
        };
        let marker = self.start();
        let Some(marker_position) = marker.position() else {
            return Marker::inactive();
        };
        let Some(distance) = marker_position.checked_sub(completed_position) else {
            self.exhaust_event_budget();
            return Marker::inactive();
        };
        let Some(Event::Start { forward_parent, .. }) = self.events.get_mut(completed_position)
        else {
            self.exhaust_event_budget();
            return Marker::inactive();
        };
        *forward_parent = Some(distance);
        marker
    }

    pub(super) fn change_kind(&mut self, completed: CompletedMarker, kind: SyntaxKind) {
        let Some(position) = completed.position() else {
            return;
        };
        let Some(Event::Start {
            kind: current_kind, ..
        }) = self.events.get_mut(position)
        else {
            self.exhaust_event_budget();
            return;
        };
        *current_kind = kind;
    }

    fn emit_event(&mut self, event: Event) -> Option<usize> {
        if self.terminal {
            return None;
        }
        let observed = self.events.len().saturating_add(1);
        if observed > self.limits.events {
            self.resource_limit_current(
                ParseResource::EventCount,
                to_i64(self.limits.events),
                to_i64(observed),
            );
            return None;
        }
        let position = self.events.len();
        self.events.push(event);
        Some(position)
    }

    fn exhaust_event_budget(&mut self) {
        self.resource_limit_current(
            ParseResource::EventCount,
            to_i64(self.limits.events),
            to_i64(self.events.len().saturating_add(1)),
        );
    }

    pub(super) fn at(&self, kind: TokenKind) -> bool {
        self.current_kind() == Some(kind)
    }

    pub(super) fn nth_at(&self, distance: usize, kind: TokenKind) -> bool {
        self.nth_kind(distance) == Some(kind)
    }

    pub(super) fn current_kind(&self) -> Option<TokenKind> {
        self.nth_kind(0)
    }

    pub(super) fn nth_kind(&self, distance: usize) -> Option<TokenKind> {
        if self.terminal {
            return None;
        }
        let physical = *self.significant.get(self.position.checked_add(distance)?)?;
        Some(self.lexed.tokens()[physical].kind())
    }

    pub(super) fn at_eof(&self) -> bool {
        self.terminal || self.position >= self.significant.len()
    }

    pub(super) fn at_contextual(&self, spelling: &str) -> bool {
        self.at(TokenKind::Identifier) && self.current_text() == Some(spelling)
    }

    pub(super) fn nth_contextual(&self, distance: usize, spelling: &str) -> bool {
        self.nth_at(distance, TokenKind::Identifier) && self.nth_text(distance) == Some(spelling)
    }

    pub(super) fn current_text(&self) -> Option<&'source str> {
        self.nth_text(0)
    }

    pub(super) fn nth_text(&self, distance: usize) -> Option<&'source str> {
        if self.terminal {
            return None;
        }
        let physical = *self.significant.get(self.position.checked_add(distance)?)?;
        self.lexed.text(self.lexed.tokens()[physical])
    }

    pub(super) fn bump(&mut self) {
        if self.terminal {
            return;
        }
        if let Some(violation) = self.delimiters.violation_at(self.position) {
            self.resource_limit(violation);
            return;
        }
        if let Some(&physical_index) = self.significant.get(self.position)
            && self.emit_event(Event::Token { physical_index }).is_some()
        {
            self.position += 1;
        }
    }

    pub(super) fn parse_path(&mut self, kind: SyntaxKind) -> Option<CompletedMarker> {
        if !self.at(TokenKind::Identifier) {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
            return None;
        }
        let path = self.start();
        self.bump();
        while self.at(TokenKind::ColonColon) && self.nth_at(1, TokenKind::Identifier) {
            self.bump();
            self.bump();
        }
        Some(self.complete(path, kind))
    }

    pub(super) fn logical_position(&self) -> usize {
        self.position
    }

    pub(super) fn kind_at_logical(&self, position: usize) -> Option<TokenKind> {
        let physical = *self.significant.get(position)?;
        Some(self.lexed.tokens()[physical].kind())
    }

    fn text_at_logical(&self, position: usize) -> Option<&'source str> {
        let physical = *self.significant.get(position)?;
        self.lexed.text(self.lexed.tokens()[physical])
    }

    pub(super) fn matching_delimiter(&mut self, opening: usize) -> Option<usize> {
        if self.terminal {
            return None;
        }
        if let Some(violation) = self.delimiters.violation_for_opening(opening) {
            self.resource_limit(violation);
            return None;
        }
        self.delimiters.matching(opening)
    }

    pub(super) fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(super) fn eat_contextual(&mut self, spelling: &str) -> bool {
        if self.at_contextual(spelling) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(super) fn expect(&mut self, kind: TokenKind) -> bool {
        if self.eat(kind) {
            true
        } else {
            if self
                .emit_event(Event::Missing {
                    kind: SyntaxKind::from_token(kind),
                })
                .is_some()
            {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
            }
            false
        }
    }

    pub(super) fn expect_identifier(&mut self) -> bool {
        self.expect(TokenKind::Identifier)
    }

    pub(super) fn expect_closing(&mut self, kind: TokenKind) -> bool {
        if self.eat(kind) {
            true
        } else {
            if self
                .emit_event(Event::Missing {
                    kind: SyntaxKind::from_token(kind),
                })
                .is_some()
            {
                self.error_current(SyntaxDiagnosticKind::ParseUnclosedDelimiter);
            }
            false
        }
    }

    pub(super) fn skip_newlines(&mut self) {
        while self.eat(TokenKind::Newline) {}
    }

    pub(super) fn line_end(&mut self) {
        if self.at(TokenKind::Newline) {
            self.skip_newlines();
        } else if !self.at_eof() && !self.at(TokenKind::RightBrace) {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
        }
    }

    pub(super) fn require_eof(&mut self) {
        if !self.at_eof() {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
        }
    }

    pub(super) fn enter_recursive(&mut self) -> bool {
        if self.terminal {
            false
        } else if self.depth >= self.limits.recursive_depth {
            self.resource_limit_current(
                ParseResource::RecursiveDepth,
                i64::from(self.limits.recursive_depth),
                i64::from(self.depth).saturating_add(1),
            );
            false
        } else {
            self.depth += 1;
            true
        }
    }

    pub(super) fn leave_recursive(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    pub(super) fn error_current(&mut self, kind: SyntaxDiagnosticKind) {
        self.error_at(kind, self.current_span());
    }

    pub(super) fn error_at(&mut self, kind: SyntaxDiagnosticKind, span: FileSpan) {
        if self.terminal {
            return;
        }
        if !self.region_has_diagnostic && self.diagnostics.len() < self.limits.syntax_diagnostics {
            self.diagnostics.push(SyntaxDiagnostic::new(kind, span));
            self.region_has_diagnostic = true;
        }
        self.halted = true;
    }

    pub(super) fn current_span(&self) -> FileSpan {
        self.significant
            .get(self.position)
            .map(|&physical| self.lexed.tokens()[physical].span())
            .unwrap_or_else(|| {
                FileSpan::new(
                    self.lexed.source().id(),
                    ByteRange::empty(self.lexed.source().len()),
                )
            })
    }

    pub(super) fn halted(&self) -> bool {
        self.halted
    }

    pub(super) fn terminal(&self) -> bool {
        self.terminal
    }

    pub(super) fn recover_top_level(&mut self, region_start: usize) -> bool {
        if self.terminal
            || self.at_eof()
            || self.diagnostics.len() >= self.limits.syntax_diagnostics
        {
            // Diagnostic-cap truncation leaves the unparsed suffix in this failed region.
            let region_end =
                if !self.terminal && self.diagnostics.len() >= self.limits.syntax_diagnostics {
                    self.significant.len()
                } else {
                    self.position
                };
            self.bind_latest_diagnostic_to_region(region_start, region_end);
            return false;
        }

        if self.position > region_start && self.is_top_level_recovery_boundary(self.position) {
            self.bind_latest_diagnostic_to_region(region_start, self.position);
            self.resume_region();
            return true;
        }

        let skipped = self.start();
        let skipped_start = self.position;
        while !self.at_eof() && !self.terminal {
            if self.position > region_start && self.is_top_level_recovery_boundary(self.position) {
                break;
            }
            let before = self.position;
            self.bump();
            if self.position == before {
                break;
            }
        }

        if self.terminal {
            self.bind_latest_diagnostic_to_region(region_start, self.position);
            return false;
        }
        self.bind_latest_diagnostic_to_region(region_start, self.position);
        if self.position > skipped_start {
            self.complete(skipped, SyntaxKind::ErrorNode);
        }
        if self.terminal || self.at_eof() {
            return false;
        }

        self.resume_region();
        true
    }

    fn bind_latest_diagnostic_to_region(&mut self, region_start: usize, region_end: usize) {
        let Some(diagnostic) = self.diagnostics.last().copied() else {
            return;
        };
        let region_start = self
            .span_at_logical(region_start)
            .range()
            .start()
            .min(diagnostic.span().range().start());
        let boundary = self
            .significant
            .get(region_end)
            .map(|&physical| self.lexed.tokens()[physical].span().range().start())
            .unwrap_or_else(|| self.lexed.source().len());
        let region_end = boundary.max(diagnostic.span().range().end());
        let recovery_span = FileSpan::new(
            self.lexed.source().id(),
            ByteRange::new(region_start, region_end)
                .expect("recovery region must contain its diagnostic"),
        );
        self.diagnostics
            .last_mut()
            .expect("latest diagnostic was observed above")
            .set_recovery_span(recovery_span);
    }

    fn is_top_level_recovery_boundary(&self, position: usize) -> bool {
        if !self.delimiters.is_trusted_top_level(position) {
            return false;
        }
        if position > 0 && self.kind_at_logical(position - 1) != Some(TokenKind::Newline) {
            return false;
        }
        match self.kind_at_logical(position) {
            Some(
                TokenKind::HashLeftBracket
                | TokenKind::PubKw
                | TokenKind::ImportKw
                | TokenKind::OverloadKw
                | TokenKind::FnKw
                | TokenKind::ExternKw
                | TokenKind::TraitKw
                | TokenKind::StructKw
                | TokenKind::EnumKw
                | TokenKind::TypeKw
                | TokenKind::ConstKw
                | TokenKind::ImplKw,
            ) => true,
            Some(TokenKind::Identifier) => self.text_at_logical(position) == Some("closed"),
            _ => false,
        }
    }

    fn resume_region(&mut self) {
        self.depth = 0;
        self.halted = false;
        self.region_has_diagnostic = false;
    }

    fn resource_limit_current(
        &mut self,
        resource: ParseResource,
        limit: i64,
        observed_at_least: i64,
    ) {
        self.resource_limit(ResourceViolation {
            resource,
            limit,
            observed_at_least,
            position: self.position,
        });
    }

    fn resource_limit(&mut self, violation: ResourceViolation) {
        if self.terminal {
            return;
        }
        self.diagnostics
            .push(SyntaxDiagnostic::parse_resource_limit(
                self.span_at_logical(violation.position),
                violation.resource,
                violation.limit,
                violation.observed_at_least,
            ));
        self.halted = true;
        self.terminal = true;
    }

    fn span_at_logical(&self, position: usize) -> FileSpan {
        self.significant
            .get(position)
            .map(|&physical| self.lexed.tokens()[physical].span())
            .unwrap_or_else(|| {
                FileSpan::new(
                    self.lexed.source().id(),
                    ByteRange::empty(self.lexed.source().len()),
                )
            })
    }

    fn drain_error(&mut self) {
        if self.at_eof() || self.terminal {
            return;
        }
        let error = self.start();
        while !self.at_eof() && !self.terminal {
            self.bump();
        }
        if !self.terminal {
            self.complete(error, SyntaxKind::ErrorNode);
        }
    }
}

fn closing_delimiter(kind: TokenKind) -> Option<TokenKind> {
    match kind {
        TokenKind::LeftParen => Some(TokenKind::RightParen),
        TokenKind::LeftBracket | TokenKind::HashLeftBracket => Some(TokenKind::RightBracket),
        TokenKind::LeftBrace => Some(TokenKind::RightBrace),
        _ => None,
    }
}

fn is_closing_delimiter(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::RightParen | TokenKind::RightBracket | TokenKind::RightBrace
    )
}

fn to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn significant_tokens(lexed: &Lexed<'_>) -> Box<[usize]> {
    lexed
        .tokens()
        .iter()
        .enumerate()
        .filter(|(_, token)| !token.kind().is_horizontal_trivia())
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use eglur_source::{FileId, SourceFile};

    use super::{ParseEntryPoint, Parser, ParserLimits, to_i64};
    use crate::tree::build_tree;
    use crate::{ParseResource, SyntaxDiagnosticKind, lex};

    #[test]
    fn delimiter_matching_respects_mixed_nesting() {
        let source = SourceFile::new(FileId::new(0), b"[({})] [(])".to_vec());
        let validated = source.validate().expect("test source should validate");
        let lexed = lex(validated);
        let mut parser = Parser::new(&lexed);

        assert_eq!(parser.matching_delimiter(0), Some(5));
        assert_eq!(parser.matching_delimiter(6), None);
        assert_eq!(parser.matching_delimiter(7), None);
    }

    #[test]
    fn delimiter_depth_limit_is_structured_and_lossless() {
        let source = SourceFile::new(FileId::new(0), b"fn main() -> i64 = (((1)))\n".to_vec());
        let validated = source.validate().expect("test source should validate");
        let lexed = lex(validated);
        let limits = ParserLimits {
            delimiter_depth: 2,
            ..ParserLimits::for_tokens(lexed.tokens().len())
        };
        let (events, diagnostics) =
            Parser::with_limits(&lexed, limits).parse(ParseEntryPoint::CompilationUnit);
        let tree = build_tree(&lexed, events);

        assert_eq!(tree.root(source.id()).text().as_bytes(), source.bytes());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].kind(),
            SyntaxDiagnosticKind::ParseResourceLimitExceeded
        );
        let resource = diagnostics[0]
            .resource_limit()
            .expect("resource diagnostic should carry structured data");
        assert_eq!(resource.resource(), ParseResource::DelimiterDepth);
        assert_eq!(resource.limit(), 2);
        assert_eq!(resource.observed_at_least(), 3);
    }

    #[test]
    fn event_limit_discards_partial_events_for_a_lossless_fallback_tree() {
        let source = SourceFile::new(FileId::new(0), b"fn main() -> Unit {}\n".to_vec());
        let validated = source.validate().expect("test source should validate");
        let lexed = lex(validated);
        let limits = ParserLimits {
            events: 3,
            ..ParserLimits::for_tokens(lexed.tokens().len())
        };
        let (events, diagnostics) =
            Parser::with_limits(&lexed, limits).parse(ParseEntryPoint::CompilationUnit);
        let tree = build_tree(&lexed, events);

        assert_eq!(tree.root(source.id()).text().as_bytes(), source.bytes());
        assert_eq!(diagnostics.len(), 1);
        let resource = diagnostics[0]
            .resource_limit()
            .expect("resource diagnostic should carry structured data");
        assert_eq!(resource.resource(), ParseResource::EventCount);
        assert_eq!(resource.limit(), 3);
        assert_eq!(resource.observed_at_least(), 4);
    }

    #[test]
    fn every_intermediate_event_budget_has_one_controlled_outcome() {
        let source = SourceFile::new(
            FileId::new(0),
            b"fn main(value: Value) -> Value = value.member.member.member\n".to_vec(),
        );
        let validated = source.validate().expect("test source should validate");
        let lexed = lex(validated);

        for event_limit in 0..=128 {
            let limits = ParserLimits {
                events: event_limit,
                ..ParserLimits::for_tokens(lexed.tokens().len())
            };
            let (events, diagnostics) =
                Parser::with_limits(&lexed, limits).parse(ParseEntryPoint::CompilationUnit);
            let tree = build_tree(&lexed, events);
            assert_eq!(
                tree.root(source.id()).text().as_bytes(),
                source.bytes(),
                "event limit {event_limit}"
            );

            if let Some(diagnostic) = diagnostics.first() {
                assert_eq!(diagnostics.len(), 1, "event limit {event_limit}");
                let resource = diagnostic
                    .resource_limit()
                    .expect("only an event resource can fail this valid fixture");
                assert_eq!(resource.resource(), ParseResource::EventCount);
                assert_eq!(resource.limit(), to_i64(event_limit));
                assert_eq!(
                    resource.observed_at_least(),
                    to_i64(event_limit.saturating_add(1))
                );
            }
        }
    }

    #[test]
    fn delimiter_and_recursive_budget_boundaries_are_exact() {
        let delimited = SourceFile::new(
            FileId::new(0),
            b"fn main() -> i64 = ((((((((1))))))))\n".to_vec(),
        );
        let validated = delimited
            .validate()
            .expect("delimiter source should validate");
        let lexed = lex(validated);
        for delimiter_limit in 0..=8 {
            let limits = ParserLimits {
                delimiter_depth: delimiter_limit,
                ..ParserLimits::for_tokens(lexed.tokens().len())
            };
            let (events, diagnostics) =
                Parser::with_limits(&lexed, limits).parse(ParseEntryPoint::CompilationUnit);
            let tree = build_tree(&lexed, events);
            assert_eq!(
                tree.root(delimited.id()).text().as_bytes(),
                delimited.bytes(),
                "delimiter limit {delimiter_limit}"
            );
            if delimiter_limit < 8 {
                let resource = diagnostics[0]
                    .resource_limit()
                    .expect("delimiter limit should carry resource data");
                assert_eq!(resource.resource(), ParseResource::DelimiterDepth);
                assert_eq!(resource.limit(), to_i64(delimiter_limit));
                assert_eq!(resource.observed_at_least(), to_i64(delimiter_limit + 1));
            } else {
                assert!(diagnostics.is_empty(), "delimiter limit {delimiter_limit}");
            }
        }

        let recursive = SourceFile::new(
            FileId::new(1),
            b"fn main() -> Bool = !!!!!!!!true\n".to_vec(),
        );
        let validated = recursive
            .validate()
            .expect("recursive source should validate");
        let lexed = lex(validated);
        for recursive_limit in 0_u16..=9 {
            let limits = ParserLimits {
                recursive_depth: recursive_limit,
                ..ParserLimits::for_tokens(lexed.tokens().len())
            };
            let (events, diagnostics) =
                Parser::with_limits(&lexed, limits).parse(ParseEntryPoint::CompilationUnit);
            let tree = build_tree(&lexed, events);
            assert_eq!(
                tree.root(recursive.id()).text().as_bytes(),
                recursive.bytes(),
                "recursive limit {recursive_limit}"
            );
            if recursive_limit < 9 {
                let resource = diagnostics[0]
                    .resource_limit()
                    .expect("recursive limit should carry resource data");
                assert_eq!(resource.resource(), ParseResource::RecursiveDepth);
                assert_eq!(resource.limit(), i64::from(recursive_limit));
                assert_eq!(resource.observed_at_least(), i64::from(recursive_limit) + 1);
            } else {
                assert!(diagnostics.is_empty(), "recursive limit {recursive_limit}");
            }
        }
    }
}
