/// Complete physical token family recognized by the v0.1 lexer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TokenKind {
    Bom,
    Whitespace,
    LineComment,
    BlockComment,
    Newline,
    Identifier,
    BangIdentifier,
    Underscore,
    Integer,
    Float,
    TupleIndex,
    String,
    FormatString,
    AxisName,
    AxisIndex,
    AxisPackAnonymous,
    AxisPackNamed,
    ModuleKw,
    FnKw,
    OverloadKw,
    StructKw,
    EnumKw,
    TraitKw,
    ImplKw,
    ImportKw,
    ExternKw,
    PubKw,
    AsKw,
    ConstKw,
    TypeKw,
    LetKw,
    MutKw,
    VarKw,
    FieldKw,
    PropKw,
    ResultKw,
    CheckedKw,
    YieldKw,
    ReturnKw,
    IfKw,
    ElseKw,
    UnknownKw,
    MatchKw,
    ForKw,
    InKw,
    WhileKw,
    BreakKw,
    ContinueKw,
    UnsafeKw,
    DynKw,
    TrueKw,
    FalseKw,
    OwnKw,
    MoveKw,
    DropOwnKw,
    Arrow,
    FatArrow,
    ColonColon,
    Range,
    RangeInclusive,
    HashLeftBracket,
    Question,
    Equal,
    EqualEqual,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Plus,
    Minus,
    Star,
    Slash,
    FractionalDivide,
    Percent,
    LogicalAnd,
    LogicalOr,
    Bang,
    Ampersand,
    Pipe,
    At,
    Colon,
    Comma,
    Dot,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Error,
    Raw,
}

impl TokenKind {
    /// Reports whether this token is omitted by the parser-visible cursor.
    #[must_use]
    pub const fn is_horizontal_trivia(self) -> bool {
        matches!(
            self,
            Self::Bom | Self::Whitespace | Self::LineComment | Self::BlockComment
        )
    }

    /// Reports whether this token carries source text but no grammar meaning.
    #[must_use]
    pub const fn is_trivia(self) -> bool {
        self.is_horizontal_trivia() || matches!(self, Self::Newline)
    }

    /// Returns the reserved-keyword kind for one exact spelling.
    #[must_use]
    pub(crate) fn keyword(text: &str) -> Option<Self> {
        Some(match text {
            "module" => Self::ModuleKw,
            "fn" => Self::FnKw,
            "overload" => Self::OverloadKw,
            "struct" => Self::StructKw,
            "enum" => Self::EnumKw,
            "trait" => Self::TraitKw,
            "impl" => Self::ImplKw,
            "import" => Self::ImportKw,
            "extern" => Self::ExternKw,
            "pub" => Self::PubKw,
            "as" => Self::AsKw,
            "const" => Self::ConstKw,
            "type" => Self::TypeKw,
            "let" => Self::LetKw,
            "mut" => Self::MutKw,
            "var" => Self::VarKw,
            "field" => Self::FieldKw,
            "prop" => Self::PropKw,
            "result" => Self::ResultKw,
            "checked" => Self::CheckedKw,
            "yield" => Self::YieldKw,
            "return" => Self::ReturnKw,
            "if" => Self::IfKw,
            "else" => Self::ElseKw,
            "unknown" => Self::UnknownKw,
            "match" => Self::MatchKw,
            "for" => Self::ForKw,
            "in" => Self::InKw,
            "while" => Self::WhileKw,
            "break" => Self::BreakKw,
            "continue" => Self::ContinueKw,
            "unsafe" => Self::UnsafeKw,
            "dyn" => Self::DynKw,
            "true" => Self::TrueKw,
            "false" => Self::FalseKw,
            "own" => Self::OwnKw,
            "move" => Self::MoveKw,
            "drop_own" => Self::DropOwnKw,
            _ => return None,
        })
    }

    /// Reports whether one exact spelling is a reserved source keyword.
    #[must_use]
    pub fn is_reserved_keyword(text: &str) -> bool {
        Self::keyword(text).is_some()
    }
}
