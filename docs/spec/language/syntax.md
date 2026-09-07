# Syntax and Grammar

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Lexical Grammar

This subsection defines the v0.1 lexical baseline required for parser and diagnostic conformance.

#### Source Text

1. Source files MUST be interpreted as UTF-8.
2. A leading UTF-8 BOM MAY be present and MUST be ignored.
3. The accepted physical line endings are `LF`, `CRLF`, and lone `CR`.
   Implementations MUST treat each as one logical `LF` for lexing and
   diagnostic text-coordinate tracking while retaining the exact physical
   source bytes.
4. Unicode is allowed in comments and string literals.
5. Unicode is not allowed in identifiers in v0.1.
6. Source byte streams that are not valid UTF-8 MUST fail with
   `LEX_INVALID_UTF8` before tokenization. Its primary span MUST identify the
   exact invalid physical byte sequence and MUST omit the derived text range.

#### Tokenization Rules

1. Lexing MUST follow maximal-munch (longest-token) behavior.
2. Outside tuple-index context, a decimal digit sequence followed by `.` forms
   a floating literal only when a decimal digit immediately follows the `.`.
   Thus `1.0` is one token, while `1.` and `.5` are not floating literals. A
   second contiguous `.` followed by a digit after a decimal-point float is a
   malformed numeric literal and MUST fail with `LEX_INVALID_NUMBER`; therefore
   `1.2.3` is not a float, member-access `.`, and integer-literal sequence.
3. After a standalone `.` token, following decimal digits MUST lex as one
   `TUPLE_INDEX` when no logical `LF` intervenes. Horizontal whitespace and
   comments without `LF` preserve this dot-owned context. A tuple index uses
   the `DecimalDigits` rules below, ends before a following `.`, and rejects
   exponent or identifier suffixes with `LEX_INVALID_NUMBER`. Thus `pair.0.1`
   lexes as identifier, `.`, tuple index `0`, `.`, and tuple index `1`.
   This classification is lexical and never consults receiver identity or type
   information; tuple validation remains a later type-checking operation.
4. Outside dot-owned context, an integer before a non-digit `.` ends first:
   1. `0..3` and `0..=3` lex as integer, range token, and integer,
   2. `0...tail` lexes as integer `0` and named axis-pack token `...tail`,
   3. `1.to_str()` and `1.0.to_str()` retain integer/float member access.
5. Compact and horizontally spaced range and ellipsis forms are equivalent.
   Parentheses are not required solely to separate a numeric receiver from
   member access. A standalone `1.` therefore lexes as integer `1` followed by
   `.`, and is rejected later unless the `.` begins a valid postfix.
6. `...` MUST lex as one token and MUST take precedence over `..=` and `..` once
   tokenization reaches the first `.`.
7. `..=` MUST lex as one token and MUST take precedence over `..` once
   tokenization reaches the first `.`.
8. `/.` MUST lex as one token and MUST take precedence over `/` followed by `.`.
9. `#[` MUST lex as one token and MUST take precedence over `#`-prefixed literals.
10. Horizontal whitespace is exactly space (`U+0020`), tab (`U+0009`), and
   form feed (`U+000C`). It and comments containing no logical `LF` separate
   tokens but are otherwise non-semantic. Vertical tab and every other Unicode
   whitespace or line-separator scalar are not source trivia and fail with
   `LEX_INVALID_CHAR` outside comments and string literals. Newlines inside
   comments follow the Comments rules below.
11. `LF` newline boundaries are significant for statement separation in v0.1 block/file grammar.
12. Automatic semicolon insertion is not part of v0.1.
13. `||` always lexes as one token under maximal munch. The parser may interpret
   that token as the empty parameter marker only immediately after the opening
   `{` of an explicit closure; everywhere else it is logical OR.
14. An interpolated string begins only with the contiguous characters `f"`.
    No horizontal whitespace, comment, or newline may separate `f` from the
    opening quote. With intervening trivia, `f` lexes as an ordinary identifier
    and the quoted text lexes as a separate `StrLiteral`; the parser MUST NOT
    fuse those tokens into a `FmtStrLiteral`.

#### Comments

1. Line comments use `//` and run to end-of-line.
2. Block comments use `/* ... */`.
3. Nested block comments are disallowed in v0.1.
4. While lexing a block comment, a new `/*` before the matching `*/` MUST fail with `LEX_NESTED_BLOCK_COMMENT`.
5. The logical `LF` that terminates a line comment is not part of that comment
   and MUST emit one `NL` token.
6. Every logical `LF` between a block comment's `/*` and matching `*/` MUST emit
   one `NL` token in source order. All other block-comment content is discarded.
7. Rule 6 applies at every delimiter depth. The lexer preserves the newline
   fact; the parser determines whether the surrounding production admits that
   `NL` token.
8. A block comment containing no logical `LF` acts as horizontal whitespace and
   MUST NOT emit `NL`.
9. `LEX_NESTED_BLOCK_COMMENT` and `LEX_UNTERMINATED_BLOCK_COMMENT` terminate
   lexing before parser behavior can be observed from any `NL` encountered in
   the malformed comment.

#### Reserved Keywords

The following words are reserved and MUST NOT be used as identifiers:

`module`, `fn`, `overload`, `struct`, `enum`, `trait`, `impl`, `import`, `extern`, `pub`, `as`, `const`, `type`, `let`, `mut`, `var`, `field`, `prop`, `result`, `checked`, `yield`, `return`, `if`, `else`, `unknown`, `match`, `for`, `in`, `while`, `break`, `continue`, `unsafe`, `dyn`, `true`, `false`.

`self` and `super` are contextual keywords in import paths (`self::...`, `super::...`) and otherwise lex as identifiers in v0.1.

`closed` is a contextual keyword only when it immediately precedes `trait` in a trait declaration and otherwise lexes as an identifier in v0.1.

`own`, `move`, and `drop_own` are reserved keywords for ownership syntax and MUST NOT be used as identifiers.

`view`, `rearrange`, `inflate`, `reduce`, `contract`, and `einop` are contextual keywords in tensor-transform literal position and otherwise lex as identifiers in v0.1.

`Self` is a reserved contextual type spelling. In type position it is interpreted before ordinary identifier lookup and is valid only in the owner scopes defined by `calls-references.md`. User type-namespace bindings that introduce `Self` MUST fail with `RESERVED_BUILTIN_NAME`; value-namespace bindings are unaffected. Reservation is validated at the binding introduction before checking uses in that declaration, so a generic parameter named `Self` fails with `RESERVED_BUILTIN_NAME` rather than making its signature uses fail with `SELF_TYPE_INVALID`.

`Fn` is a reserved built-in callable type head in type positions.
User type-namespace bindings that introduce `Fn` MUST fail with `RESERVED_BUILTIN_NAME`; value-namespace bindings are unaffected by this type-name reservation.
In type grammar, `Fn` followed by `(` is always parsed as `FnType`, not as a user `PathType`.

`Union` is a reserved built-in structural type head in type positions.
User type-namespace bindings that introduce `Union` MUST fail with `RESERVED_BUILTIN_NAME`; value-namespace bindings are unaffected by this type-name reservation.
The parser treats `Union[...]` as the ordinary `PathType TypeArgs` surface. Type resolution identifies the unqualified reserved head and applies the arity and position rules in `type-system.md`.

`Never` is a reserved intrinsic type spelling in type positions.
User type-namespace bindings that introduce `Never` MUST fail with
`RESERVED_BUILTIN_NAME`; value-namespace bindings are unaffected by this
type-name reservation. The parser treats `Never` as an ordinary `PathType`.
Type resolution identifies the unqualified reserved spelling before ordinary
type lookup and applies the identity, arity, and position rules in
`type-system.md`. `Never` is not a reserved lexer keyword, and `!` is not a type
spelling.

#### Identifier and Axis Tokens

1. Base identifier grammar is ASCII-only: `[A-Za-z_][A-Za-z0-9_]*`.
   When an unsupported non-ASCII scalar has Unicode 17.0.0 `XID_Start` or
   `XID_Continue` status, the lexer MUST report
   `LEX_INVALID_IDENTIFIER_CHAR`; other unsupported non-ASCII scalars MUST
   report `LEX_INVALID_CHAR`. The primary span identifies that first scalar's
   exact UTF-8 bytes. This classification does not apply inside comments or
   string literals.
2. `BangIdent` is `<Identifier>!` with no intervening whitespace.
3. `AxisName` literal grammar is `'` followed by `<Identifier>` (for example `'batch`).
4. `AxisIndex` literal grammar is `#` immediately followed by `DecimalDigits`
   (for example `#0`, `#12`, or `#1_024`). Except for `#[`, a `#` not followed
   by an ASCII decimal digit fails with `LEX_INVALID_CHAR`. Once the digit
   component starts, malformed separators fail with `LEX_INVALID_NUMBER`.
5. A lone `_` is a reserved token used by wildcard, discard-target, and hole syntax; it is not a regular binding identifier.
6. `@` is a delimiter token only when it follows an `Identifier` on the same
   logical line, as required by `AliasPattern`. Otherwise, sigil-prefixed
   identifiers other than `'` and `#` (for example `@x`, `$x`) are disallowed
   in v0.1 and fail with `LEX_INVALID_CHAR`.
7. `AxisPackAnon` token is `...` and `AxisPackNamed` token is `...<Identifier>` with no intervening whitespace.
8. Axis-pack tokens are semantic-only inside tensor transform pattern contexts (`view/rearrange/inflate/reduce/einop`) and are not general expression/path identifiers.

#### Literal Tokens

1. Decimal digit components use the `DecimalDigit`, `DecimalDigits`, and
   `Exponent` productions in the complete grammar below. A component begins
   and ends with an ASCII decimal digit and permits `_` only as a single
   separator between adjacent digits. An exponent begins with `e` or `E`, may
   contain one following `+` or `-`, and requires a `DecimalDigits` component.
2. Integer literals are `DecimalDigits`.
3. Floating literals are either `DecimalDigits "." DecimalDigits Exponent?`
   or `DecimalDigits Exponent`. Leading and trailing decimal points are not
   valid floating literals.
4. `1_000`, `1_000.25_00`, `1e1_0`, and `1.25E-1_0` are valid. Once a numeric
   token has started, consecutive or trailing separators and separators next
   to `.`, `e`/`E`, or an exponent sign fail with `LEX_INVALID_NUMBER`.
5. Numeric-token rules do not absorb surrounding identifier syntax. `_1` is an
   identifier, and `1._0` is integer `1`, `.`, and identifier `_0`.
6. String literals are double-quoted.
7. Required string escapes in v0.1 are: `\\`, `\"`, `\n`, `\r`, `\t`, and `\u{...}`.
8. A `\u{...}` escape MUST contain one to six ASCII hexadecimal digits and
   denote one Unicode scalar value: `U+0000..U+D7FF` or `U+E000..U+10FFFF`.
   Empty, non-hexadecimal, surrogate, and out-of-range forms MUST fail with
   `LEX_INVALID_ESCAPE`.
9. Unknown escapes are lexical errors and MUST fail with `LEX_INVALID_ESCAPE`.
10. Interpolated string literals use the contiguous `f"..."` surface from
   Tokenization Rule 14.
11. v0.1 interpolated placeholders are named-only: `{name}` and debug-form `{name:?}`.
12. Literal braces in interpolated strings MUST be escaped as `{{` and `}}`.
13. `format(...)` uses that same placeholder and literal-brace family at runtime.
14. Placeholder resolution and placeholder-shape validation are compile-time checks:
   1. unresolved placeholder names fail with `FSTRING_UNRESOLVED_NAME`,
   2. malformed placeholder grammar fails with `FSTRING_INVALID_PLACEHOLDER`.
15. The Unit value literal is exactly `()`; `Unit` remains its type spelling.
16. Duration-suffix numeric literals (for example `2s`, `150ms`) are not part of v0.1 baseline literal grammar:
   1. source forms that append alphabetic unit suffix directly to numeric literals MUST be rejected in baseline mode,
   2. lexical rejection MUST use `LEX_INVALID_NUMBER`.

#### Operators and Delimiters

At minimum, v0.1 lexers MUST recognize:

`->`, `=>`, `::`, `...`, `..`, `..=`, `#[`, `?`, `=`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `+`, `-`, `*`, `/`, `/.`, `%`, `&&`, `||`, `!`, `&`, `|`, `@`, `:`, `,`, `.`, `(`, `)`, `{`, `}`, `[`, `]`.

#### Lexer Diagnostics

Lexical failures MUST use stable compile-time diagnostic codes defined in `../ERRORS.md`:

`LEX_INVALID_UTF8`, `LEX_INVALID_CHAR`, `LEX_INVALID_IDENTIFIER_CHAR`, `LEX_UNTERMINATED_STRING`, `LEX_INVALID_ESCAPE`, `LEX_UNTERMINATED_BLOCK_COMMENT`, `LEX_NESTED_BLOCK_COMMENT`, `LEX_INVALID_NUMBER`.


#### Parser EBNF

The following grammar is normative for v0.1 parser behavior.

```text
CompilationUnit   ::= NL* ModuleDecl? ( Item NL* )* EOF
LineEnd           ::= NL+ | <empty only before "}" or EOF>
ModuleDecl        ::= "module" ModulePath LineEnd

Item              ::= ImportDecl | AttrTopDecl | DeclItem
DeclItem          ::= FnDecl | ExternDecl | TraitDecl | ImplDecl | StructDecl | EnumDecl | TypeDecl | ConstDecl | PubDeclItem
PubDeclItem       ::= "pub" ( FnDecl | ExternDecl | TraitDecl | StructDecl | EnumDecl | TypeDecl | ConstDecl )
AttrTopDecl       ::= AttrList AttrTopTarget
AttrTopTarget     ::= FnDecl | StructDecl | ImplDecl | EnumDecl | "pub" (FnDecl | StructDecl | EnumDecl)

ImportDecl        ::= Visibility? "import" ImportSource LineEnd
Visibility        ::= "pub"
ImportSource      ::= ImportPath "::" ImportSpec | ImportPath ImportAlias?
ImportSpec        ::= ImportLeaf | "{" NL* ImportEntryList? NL* "}"
ImportEntryList   ::= ImportEntry ( NL* "," NL* ImportEntry )* ( NL* "," )?
ImportEntry       ::= ImportLeaf
ImportLeaf        ::= ImportName ImportAlias?
ImportName        ::= Identifier | BangIdent
ImportAlias       ::= "as" ( Identifier | BangIdent )
ImportPath        ::= ModulePath | RelativeImportPath
ModulePath        ::= Path
RelativeImportPath ::= ("self" | "super") ( "::" Identifier )*
Path              ::= Identifier ( "::" Identifier )*
QualifiedPath     ::= Identifier "::" Identifier ( "::" Identifier )*

FnDecl            ::= FnSignature FnBody
FnSignature       ::= "overload"? "fn" FnName GenericParams? "(" NL* ParamList? NL* ")" ReturnType?
ExternDecl        ::= "extern" StrLiteral "fn" FnName "(" NL* ParamList? NL* ")" ReturnType? LineEnd
FnName            ::= Identifier | BangIdent
FnBody            ::= BlockExpr | "=" NL* Expr LineEnd
ParamList         ::= Param ( NL* "," NL* Param )* ( NL* "," )?
Param             ::= Pattern ":" TypeExpr
ReturnType        ::= "->" TypeExpr

TraitDecl         ::= "closed"? "trait" Identifier GenericParams? "{" NL* ( TraitItem NL* )* "}"
TraitItem         ::= TraitFnDecl | AttrTraitFnDecl | AssocTypeDecl | StorageFieldDecl | PropertyDecl
TraitFnDecl       ::= FnSignature ( FnBody | LineEnd )
AssocTypeDecl     ::= "type" Identifier LineEnd
ImplDecl          ::= "impl" GenericParams? ImplTraitRef "for" TypeExpr "{" NL* ( ImplItem NL* )* "}"
ImplTraitRef      ::= "!"? TypeExpr
ImplItem          ::= FnDecl | AttrFnDecl | AssocTypeBinding
AssocTypeBinding  ::= "type" Identifier "=" TypeExpr LineEnd
StructDecl        ::= "struct" Identifier GenericParams? ( ":" TypeExpr )? StructBody
StructBody        ::= "{" NL* ( StructItem NL* )* "}" | "{}"
StructItem        ::= StructStorageFieldDecl | StructPropertyDecl | StructFnDecl | AttrStructFnDecl
StructStorageFieldDecl ::= Visibility? StorageFieldDecl
StructPropertyDecl ::= Visibility? PropertyDecl
StructFnDecl      ::= Visibility? FnDecl
AttrStructFnDecl  ::= AttrList StructFnDecl
StorageFieldDecl  ::= ( "field" "mut"? | "var" ) Identifier ":" TypeExpr ( "=" Expr )? LineEnd
PropertyDecl      ::= "prop" "mut"? Identifier ":" TypeExpr PropertyBody
PropertyBody      ::= "{" NL* ( PropertyAccessor NL* )* "}"
PropertyAccessor  ::= FnDecl
AttrFnDecl        ::= AttrList FnDecl
AttrTraitFnDecl   ::= AttrList TraitFnDecl
EnumDecl          ::= "enum" Identifier GenericParams? EnumBody
EnumBody          ::= "{" NL* ( EnumVariant NL* )* "}" | "{}"
EnumVariant       ::= AttrList? Identifier EnumVariantPayload? LineEnd
EnumVariantPayload ::= "(" NL* TypeExprList? NL* ")" | "{" NL* EnumVariantFieldTypeList NL* "}"
EnumVariantFieldTypeList ::= EnumVariantFieldType ( NL* "," NL* EnumVariantFieldType )* ( NL* "," )?
EnumVariantFieldType ::= Identifier ":" TypeExpr
TypeDecl          ::= "type" Identifier GenericParams? "=" TypeExpr LineEnd
ConstDecl         ::= "const" Identifier ":" TypeExpr "=" Expr LineEnd

GenericParams     ::= "[" NL* GenericParam ( NL* "," NL* GenericParam )* ( NL* "," )? NL* "]"
GenericParam      ::= Identifier ( ":" TraitBoundList )?
TraitBoundList    ::= TraitRef ( "+" TraitRef )*
TraitRef          ::= PathType TypeArgs?
AttrList          ::= Attribute ( NL* Attribute )* NL*
Attribute         ::= "#[" AttrMeta "]"
AttrMeta          ::= AttrPath AttrArgPack?
AttrPath          ::= Identifier ( "::" Identifier )*
AttrArgPack       ::= "(" NL* AttrArgList? NL* ")"
AttrArgList       ::= AttrArg ( NL* "," NL* AttrArg )* ( NL* "," )?
AttrArg           ::= AttrValue | Identifier "=" AttrValue
AttrValue         ::= AttrMeta | IntLiteral | FloatLiteral | StrLiteral | BoolLiteral

BlockExpr         ::= "{" NL* ( Stmt NL* )* YieldStmt? NL* "}"
Stmt              ::= LetStmt | AssignStmt | WhileStmt | ForStmt | BreakStmt | ContinueStmt | ReturnStmt | ExprStmt
LetStmt           ::= ( "let" "mut"? | "var" ) Pattern ( ":" TypeExpr )? ( "=" Expr )? LineEnd
AssignStmt        ::= AssignTarget "=" Expr LineEnd
AssignTarget      ::= "_" | Identifier | PostfixExpr | DerefExpr
ExprStmt          ::= Expr LineEnd
YieldStmt         ::= "yield" Expr LineEnd
WhileStmt         ::= "while" ControlHeadExpr BlockExpr
ForStmt           ::= "for" Identifier "in" ControlHeadExpr BlockExpr
BreakStmt         ::= "break" LineEnd
ContinueStmt      ::= "continue" LineEnd
ReturnStmt        ::= "return" Expr? LineEnd

Expr              ::= LogicOrExpr
GroupedExpr       ::= GroupedLogicOrExpr
ControlHeadExpr   ::= Expr
LogicOrExpr       ::= LogicAndExpr ( "||" NL* LogicAndExpr )*
LogicAndExpr      ::= CompareExpr ( "&&" NL* CompareExpr )*
CompareExpr       ::= RangeExpr ( ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) NL* RangeExpr )*
RangeExpr         ::= AddExpr ( ( ".." | "..=" ) NL* AddExpr )?
AddExpr           ::= MulExpr ( ( "+" | "-" ) NL* MulExpr )*
MulExpr           ::= UnaryExpr ( ( "*" | "/" | "/." | "%" ) NL* UnaryExpr )*
GroupedLogicOrExpr ::= GroupedLogicAndExpr ( NL* "||" NL* GroupedLogicAndExpr )*
GroupedLogicAndExpr ::= GroupedCompareExpr ( NL* "&&" NL* GroupedCompareExpr )*
GroupedCompareExpr ::= GroupedRangeExpr ( NL* ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) NL* GroupedRangeExpr )*
GroupedRangeExpr  ::= GroupedAddExpr ( NL* ( ".." | "..=" ) NL* GroupedAddExpr )?
GroupedAddExpr    ::= GroupedMulExpr ( NL* ( "+" | "-" ) NL* GroupedMulExpr )*
GroupedMulExpr    ::= UnaryExpr ( NL* ( "*" | "/" | "/." | "%" ) NL* UnaryExpr )*
UnaryExpr         ::= DropOwnExpr | OwnExpr | MoveExpr | RefExpr | DerefExpr | ( "!" | "-" ) UnaryExpr | PostfixExpr
RefExpr           ::= "&" "mut"? UnaryExpr
DerefExpr         ::= "*" UnaryExpr
OwnExpr           ::= "own" Identifier
MoveExpr          ::= "move" Identifier
DropOwnExpr       ::= "drop_own" ( MoveExpr | Identifier )
PostfixExpr       ::= PrimaryExpr ( PostfixSuffix | NL+ DotContinuationSuffix )*
PostfixSuffix     ::= DotContinuationSuffix | TypeArgCallSuffix | DTypeBracketCallSuffix | CallSuffix | TrailingClosureArg | SpawnThunkArg | TrySuffix
DotContinuationSuffix ::= ReduceByCandidateSuffix | TupleProjectionSuffix | MemberSuffix
ReduceByCandidateSuffix ::= ".reduce_by" "(" NL* ReduceByArgs NL* ")"
TypeArgCallSuffix ::= "::" TypeArgs CallSuffix
DTypeBracketCallSuffix ::= "[" NL* TypeExpr NL* "]" CallSuffix
CallSuffix        ::= "(" NL* ArgList? NL* ")"
ArgList           ::= Arg ( NL* "," NL* Arg )* ( NL* "," )?
Arg               ::= NamedArg | Expr
NamedArg          ::= Identifier "=" Expr
TupleProjectionSuffix ::= "." TupleIndex
MemberSuffix      ::= "." ( Identifier | BangIdent )
TrySuffix         ::= "?"

PrimaryExpr       ::= Literal
                    | UnitExpr
                    | CatalogOwnerCallExpr
                    | Identifier
                    | BangIdent
                    | HoleExpr
                    | AxisNameLit
                    | AxisIndexLit
                    | LambdaExpr
                    | TupleExpr
                    | ArrayExpr
                    | MapExpr
                    | StructLiteralExpr
                    | VariantConstructorExpr
                    | BlockExpr
                    | IfExpr
                    | MatchExpr
                    | ResultExpr
                    | CheckedExpr
                    | TensorSchemaExpr
                    | UnsafeExpr
                    | GroupExpr
GroupExpr         ::= "(" NL* GroupedExpr NL* ")"
HoleExpr          ::= "_"
UnitExpr          ::= "(" NL* ")"
UnsafeExpr        ::= "unsafe" BlockExpr
TupleExpr         ::= "(" NL* Expr NL* "," NL* ExprList? NL* ")"
ExprList          ::= Expr ( NL* "," NL* Expr )* ( NL* "," )?
ArrayExpr         ::= "[" NL* ExprList? NL* "]"
MapExpr           ::= "[" NL* ":" NL* "]"
                    | "[" NL* MapEntryList NL* "]"
MapEntryList      ::= MapEntry ( NL* "," NL* MapEntry )* ( NL* "," )?
MapEntry          ::= Expr ":" NL* Expr
StructLiteralExpr ::= Path "{" NL* FieldInitList? NL* "}"
VariantConstructorExpr ::= QualifiedPath ( "(" NL* ExprList? NL* ")" | "{" NL* FieldInitList? NL* "}" )?
CatalogOwnerCallExpr ::= Path TypeArgs "." ( Identifier | BangIdent ) CallSuffix
LambdaExpr        ::= ExplicitLambdaExpr | PlaceholderLambdaExpr
ExplicitLambdaExpr ::= "{" ( "||" | "|" "|" | "|" LambdaParamList "|" ) LambdaBody "}"
PlaceholderLambdaExpr ::= "{" PlaceholderLambdaBody "}"
LambdaParamList   ::= LambdaParam ( "," LambdaParam )* ","?
LambdaParam       ::= ( Identifier | "_" ) ( ":" TypeExpr )?
LambdaBody        ::= Expr | NL* ( LambdaStmt NL* )* LambdaTailExpr? NL*
LambdaStmt        ::= LetStmt | AssignStmt | WhileStmt | ForStmt | BreakStmt | ContinueStmt | ReturnStmt | ExprStmt
LambdaTailExpr    ::= Expr
PlaceholderLambdaBody ::= Expr
TrailingClosureArg ::= LambdaExpr
SpawnThunkArg     ::= "{" NL* ( LambdaStmt NL* )* LambdaTailExpr? NL* "}"
FieldInitList     ::= FieldInit ( NL* "," NL* FieldInit )* ( NL* "," )?
FieldInit         ::= Identifier ":" Expr
IfExpr            ::= "if" ControlHeadExpr BlockExpr ( "else" IfElseTail )?
IfElseTail        ::= BlockExpr IfUnknownTail? | IfExpr
IfUnknownTail     ::= "unknown" BlockExpr
MatchExpr         ::= "match" ControlHeadExpr "{" NL* ( MatchArm NL* )* "}"
MatchArm          ::= Pattern GuardOpt? "=>" NL* Expr ( "," NL* | LineEnd )
GuardOpt          ::= "if" Expr
ResultExpr        ::= "result" BlockExpr
CheckedExpr       ::= "checked" BlockExpr

TensorSchemaExpr  ::= ViewSchemaExpr
                    | RearrangeSchemaExpr
                    | InflateSchemaExpr
                    | ReduceSchemaExpr
                    | ContractSchemaExpr
                    | EinopSchemaExpr
ViewSchemaExpr    ::= "view" "{" NL* TransformOperands NL* "->" NL* TransformOutputs NL* "}"
RearrangeSchemaExpr ::= "rearrange" "{" NL* TransformOperands NL* "->" NL* TransformOutputs NL* "}"
InflateSchemaExpr ::= "inflate" "{" NL* TransformOperands NL* "->" NL* TransformOutputs NL* "}"
ReduceSchemaExpr  ::= "reduce" "{" NL* TransformOperand NL* "->" NL* TransformOutput NL* "}"
ContractSchemaExpr ::= "contract" "{" NL* TransformOperands NL* "->" NL* TransformOutput NL* "}"
EinopSchemaExpr   ::= "einop" "{" NL* TransformOperands NL* "->" NL* TransformOutputs NL* "}"

TransformOperand  ::= TransformAxisList
TransformOutput   ::= TransformAxisList
TransformOperands ::= TransformAxisList ( "," TransformAxisList )*
TransformOutputs  ::= TransformAxisList ( "," TransformAxisList )*
TransformAxisList ::= TransformAxisItem*
TransformAxisItem ::= AxisExpr | AxisPackAnon | AxisPackNamed
AxisExpr          ::= AxisTerm ( "+" AxisTerm )*
AxisTerm          ::= AxisFactor ( "*" AxisFactor )*
AxisFactor        ::= Identifier | IntLiteral | "(" AxisExpr ")"
ReduceByArgs      ::= ReduceBySingle | ReduceByPhases
ReduceBySingle    ::= Expr
ReduceByPhases    ::= ReduceByPhase ( NL* "," NL* ReduceByPhase )* ( NL* "," )?
ReduceByPhase     ::= "(" NL* ReduceByPhaseAxes NL* "," NL* Expr NL* ")"
ReduceByPhaseAxes ::= Identifier | "(" Identifier Identifier+ ")"

Pattern           ::= OrPattern
OrPattern         ::= PatternAtom ( "|" PatternAtom )*
PatternAtom       ::= "_"
                    | Identifier
                    | LiteralPattern
                    | UnitPattern
                    | AliasPattern
                    | VariantPattern
                    | TuplePattern
                    | StructPattern
                    | "(" NL* Pattern NL* ")"
AliasPattern      ::= Identifier "@" PatternAtom
UnitPattern       ::= "(" NL* ")"
VariantPattern    ::= QualifiedPath ( "(" NL* PatternList? NL* ")" | "{" NL* FieldPatternList? NL* "}" )?
                    | Identifier "(" NL* PatternList? NL* ")"
TuplePattern      ::= "(" NL* Pattern NL* "," NL* PatternList? NL* ")" | "(" NL* ".." NL* ")"
StructPattern     ::= Path? "{" NL* FieldPatternList? NL* "}"
FieldPatternList  ::= FieldPattern ( NL* "," NL* FieldPattern )* ( NL* "," )?
FieldPattern      ::= Identifier ( ":" Pattern )? | ".."
PatternList       ::= PatternItem ( NL* "," NL* PatternItem )* ( NL* "," )?
PatternItem       ::= Pattern | ".."

TypeExpr          ::= RefType
RefType           ::= "&" "mut"? RefType | PtrType | PrimaryType
PtrType           ::= "*" ( "const" | "mut" ) RefType
PrimaryType       ::= PathType TypeArgs?
                    | DynType
                    | TupleType
                    | FnType
                    | "(" NL* TypeExpr NL* ")"
DynType           ::= "dyn" PathType TypeArgs?
PathType          ::= Identifier ( "::" Identifier )*
TypeArgs          ::= "[" NL* TypeExpr ( NL* "," NL* TypeExpr )* ( NL* "," )? NL* "]"
TupleType         ::= "(" NL* TypeExpr NL* "," NL* TypeExprList? NL* ")"
FnType            ::= "Fn" "(" NL* TypeExprList? NL* ")" "->" TypeExpr
TypeExprList      ::= TypeExpr ( NL* "," NL* TypeExpr )* ( NL* "," )?

Literal           ::= IntLiteral | FloatLiteral | StrLiteral | FmtStrLiteral | BoolLiteral
LiteralPattern    ::= IntLiteral | FloatLiteral | StrLiteral | BoolLiteral
DecimalDigit      ::= "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"
DecimalDigits     ::= DecimalDigit ( DecimalDigit | "_" DecimalDigit )*
Exponent          ::= ( "e" | "E" ) ( "+" | "-" )? DecimalDigits
IntLiteral        ::= DecimalDigits
TupleIndex        ::= <TUPLE_INDEX token whose payload is DecimalDigits>
FloatLiteral      ::= DecimalDigits "." DecimalDigits Exponent? | DecimalDigits Exponent
StrLiteral        ::= <double-quoted string literal>
BoolLiteral       ::= "true" | "false"
FmtStrLiteral     ::= <contiguous "f" followed immediately by StrLiteral>
AxisNameLit       ::= "'" Identifier
AxisIndexLit      ::= "#" DecimalDigits
AxisPackAnon      ::= "..."
AxisPackNamed     ::= "..." Identifier
BangIdent         ::= Identifier "!"
```

Delimited newline rules:

1. Newlines are not discarded globally. A comma list enclosed by `(...)`,
   `[...]`, or list-owned `{...}` owns `NL*` only where its production shows it:
   immediately after the opening delimiter, immediately before the closing
   delimiter, and on either side of a comma. This policy covers import groups;
   declaration, call, and attribute argument lists; generic and type arguments;
   tuple, array, constructor, and pattern lists; callable and tuple types; and
   `reduce_by` phase lists.
2. A newline never substitutes for a comma. Adjacent entries without a comma
   MUST fail with `PARSE_UNEXPECTED_TOKEN`, whether the entries are on one line
   or separate lines. A trailing comma remains optional in both single-line and
   multiline forms.
3. Parenthesized expression grouping selects `GroupedExpr` and owns its boundary
   `NL*`; parenthesized type and pattern grouping owns leading and trailing
   `NL*`. Empty Unit expressions and patterns, zero-argument calls,
   zero-parameter declarations, and zero-parameter `Fn` types do likewise. The
   dedicated single-item `DTypeBracketCallSuffix` and the outer
   `ReduceByCandidateSuffix` delimiters also own boundary `NL*` as shown.
4. An `NL` emitted from inside a block comment is indistinguishable at the
   parser boundary from an `NL` emitted from source text and is accepted only
   at the same explicitly owned positions.
5. Pipe-delimited lambda parameters, tensor schema operand/output lists and
   their whitespace-separated axis lists, and `ReduceByPhaseAxes` do not
   inherit comma-list newline behavior. They admit newlines only where their
   dedicated productions explicitly show `NL*`.
6. `MapEntryList` follows the same delimiter and comma ownership as other
   bracketed comma lists. Its `:` additionally owns `NL*` immediately after
   the delimiter. `[:]` is the sole empty-map spelling; `[]` remains the empty
   array expression.

Expression continuation rules:

1. Outside a delimiter-owned position, an `NL` remains a statement separator
   unless an expression production explicitly owns it. Indentation does not
   affect this classification.
2. Every binary operator in ordinary `Expr` and `GroupedExpr` owns `NL*`
   immediately after the operator. A trailing binary operator therefore
   continues its right operand on a later line.
3. No binary operator in ordinary `Expr` owns a preceding newline. At a
   statement-capable boundary, a complete expression followed by `NL` ends
   before every line-leading binary operator, independent of operator spelling.
4. `GroupExpr` selects the separate `GroupedExpr` precedence family. At that
   group's current delimiter depth, every binary operator owns preceding `NL*`.
   This permission does not propagate into a nested call argument, tuple or
   array entry, field initializer, lambda or block body, tensor schema, type, or
   pattern; such a nested expression requires its own `GroupExpr`.
5. Consequently, `left` followed on the next line by `-right` or `*pointer`
   outside `GroupExpr` forms two statements. Inside `GroupExpr`, the same tokens
   continue binary subtraction or multiplication when a left operand precedes
   the owned newline.
6. A preceding `NL+` may attach only the dot-led postfix forms `.member`,
   `.member!`, `.0`, and `.reduce_by(...)`. The member and projection spellings
   stand for their complete lexical families, not only the displayed examples.
7. `?`, `::[...]`, call suffixes `(...)`, dtype-bracket call suffixes `[...]`,
   and trailing closure or spawn braces remain attached to the preceding
   expression only when no `NL` intervenes.
8. An `NL` emitted by a block comment follows the same continuation rules as a
   source newline. Leading spaces neither authorize a continuation nor prevent
   one authorized by `GroupedExpr`, a trailing operator, or a dot-led suffix.
9. These rules apply only to the general expression precedence and postfix
   productions shown above. Tensor schema axis expressions, type expressions,
   patterns, and pipe-delimited lambda parameters do not inherit them.
10. The `=` delimiter in an expression-body function owns `NL*` immediately
    after it. This permission belongs only to `FnBody`; it does not make a
    newline after a local initializer or assignment `=` into continuation.
11. The `=>` delimiter in a `MatchArm` owns `NL*` immediately after it. The
    arm result expression may therefore begin on a later line without grouping.

Container newline rules:

1. File, declaration-body, block, closure-body, and match-body productions own
   the `NL*` between their entries. A self-delimiting entry therefore does not
   need its own trailing-newline alternative merely to admit the next entry.
2. Every line-oriented leaf ends in the shared `LineEnd` production.
   `LineEnd` consumes one or more newlines, or is empty only when the next token
   closes the current braced container or is `EOF`.
3. `AttrList` owns newlines between attributes and after the final attribute,
   so an attributed target may begin on a later line, including after a line
   comment.
4. Leading and trailing blank lines are accepted only where the containing
   production shows `NL*`.
5. Consequently, an expression-body declaration may end immediately before
   `}` or `EOF`, but two line-oriented declarations or statements on one line
   require an explicit grammar delimiter; whitespace alone is not a separator.
6. In `LambdaBody` and `SpawnThunkArg`, a final `Expr` immediately before the
   closing `}` is always `LambdaTailExpr`, even though an empty `LineEnd` could
   otherwise let it parse as `ExprStmt`. The tail-expression interpretation
   takes precedence and determines the closure or child-task result.

Type expression semantic note:

1. `PtrType` is parser surface for raw pointer types needed by extern declarations.
2. In v0.1 baseline, semantic acceptance of a source `*const T` / `*mut T` type expression is limited to extern declaration signature positions as defined in `unsafe-ffi.md`.
3. Raw pointer type expressions in ordinary function, method, local binding, field, property, alias, trait, generic argument, or other non-extern type positions MUST fail with `RAW_POINTER_TYPE_UNSUPPORTED`.
4. Compiler inference of a confined raw-pointer relay local under `unsafe-ffi.md` does not parse or admit a `PtrType` in any additional source type position.
5. `DynType` is parse-valid future surface for explicit dynamic trait objects, but v0.1 baseline rejects semantic use of `dyn Trait` as defined in `traits.md` and `type-system.md`.

Collection literal semantic notes:

1. `ArrayExpr` (`[e1, ..., en]`) and `MapExpr` (`[k1: v1, ..., kn: vn]` or
   `[:]`) are distinct parser surfaces selected entirely by their bracket
   contents. A top-level `:` after the first expression selects `MapExpr`;
   expected type and name resolution do not participate in parsing.
2. Nested delimiters own their internal colons before outer collection
   classification. Struct literals used as keys or values therefore retain
   ordinary struct-literal parsing.
3. v0.1 literal container binding is specified in `type-system.md`: baseline
   array container is `Seq[T]`, and baseline map container is `Map[K,V]`.
4. Bare-brace key-value expressions are not map literals. `{}` and other
   unowned braced expressions retain block or closure parsing.

Axis literal semantic note:

1. `AxisNameLit` and `AxisIndexLit` are language-owned tensor-layer literals.
2. `'name` constructs the canonical `std::tensor::AxisName` value and `#i`
   constructs the canonical `std::tensor::AxisIndex` value.
3. Their type identity does not use ordinary name lookup, expected-type
   inference, imports, aliases, or same-spelled local/module declarations.
4. These literals activate the bounded tensor source layer defined in
   `implementation-boundary.md`; they do not define a user-extensible literal
   protocol.

Tuple and grouping disambiguation:

1. Parenthesized expression and type forms without a comma are grouping forms.
2. `GroupExpr` is semantically transparent: it preserves the enclosed
   expression's value, type, and semantic category while providing the explicit
   newline-continuation boundary defined above.
3. Parenthesized pattern forms without a comma are grouping forms when their contents parse as `Pattern`.
4. Parenthesized expression, type, and pattern forms with a comma are tuple forms.
5. Unary tuple forms are written with a trailing comma: `(expr,)`, `(Type,)`, and `(pattern,)`.
6. The rest-only tuple pattern `(..)` is a tuple pattern because `..` is not a grouped pattern.
7. In expression context, `()` is `UnitExpr`; in pattern context, it is
   `UnitPattern`. Neither form is a tuple.
8. The empty parenthesized type form `()` is invalid in v0.1; write the named
   type `Unit`.

Constructor expression notes:

1. `VariantConstructorExpr` admits source-visible variants owned by either a
   user enum or a catalog sum (`Owner::Variant`, `Owner::Variant(...)`, and
   `Owner::Variant { ... }`).
2. `::` is the variant-qualification surface. `Owner.Variant` remains ordinary
   member-access syntax and is never reclassified as a variant reference.
3. If a qualified braced expression could parse as either `StructLiteralExpr`
   or named-payload `VariantConstructorExpr`, name resolution determines which
   declaration kind is referenced.
4. Semantic validation of variant constructor expressions is defined in
   `algebraic-data.md`, `modules-resolution.md`, and `type-system.md`.

Literal pattern notes:

1. Literal patterns admit only integer, floating, non-interpolated string, and
   boolean literals.
2. `FmtStrLiteral` remains expression-only. An interpolated string at a pattern
   site MUST fail with `PARSE_EXPECTED_PATTERN`.

Variant pattern notes:

1. Source enum and catalog-sum patterns use the same owner-qualified unit,
   positional, and named-payload forms as their constructors.
2. The unqualified `Identifier(...)` alternative exists only as parse surface
   for the built-in residual constructors `Some(...)`, `Ok(...)`, and
   `Err(...)`; semantic resolution rejects other unqualified constructor heads.
3. Bare `None` parses as `Identifier` and is classified as the built-in
   residual constructor during pattern resolution.
4. A qualified braced pattern is classified as a named-payload variant pattern
   or a struct pattern from the resolved owner declaration.
5. Semantic validation does not normalize between bare, parenthesized, and
   braced variant patterns; each form must match the resolved declaration kind
   under `control-flow.md`.

Tensor transform grammar notes:

1. A complete token sequence matching one of the six `TensorSchemaExpr`
   productions is classified as that schema literal before ordinary identifier
   lookup. No import is required, and a same-spelled local, module declaration,
   import, or alias does not shadow the complete literal form.
2. The spellings `view`, `rearrange`, `inflate`, `reduce`, `contract`, and
   `einop` remain ordinary identifiers outside a complete matching
   `TensorSchemaExpr`. For example, `view(x)` and `view { |x| x }` use ordinary
   lookup and call typing.
3. If a same-spelled identifier followed by a braced form does not match a
   complete `TensorSchemaExpr` but does match `TrailingClosureArg`, the ordinary
   identifier-plus-call parse is used. Otherwise normal parser failure applies.
4. `TransformAxisList` items are whitespace-separated in source.
5. `ReduceByPhaseAxes` parenthesized form (`(h w)`) is a whitespace-separated axis set form.
6. `ReduceByCandidateSuffix` is a contextual parse candidate and MUST NOT
   reserve the method name `reduce_by`:
   1. when the tokens also form an ordinary `MemberSuffix` followed by
      `CallSuffix`, the parser MUST retain that ordinary candidate,
   2. when the receiver has the canonical catalog `ReduceSchema` identity, the
      special candidate is mandatory and its arguments MUST match
      `ReduceByArgs`; the ordinary candidate is not a fallback,
   3. a canonical `ReduceSchema.reduce_by` argument list that does not match
      `ReduceByArgs` MUST fail with `REDUCE_BY_ARGUMENTS_INVALID` after
      receiver identity resolution; this is not an immediate parser failure,
   4. every other receiver uses the ordinary member-call candidate; user types
      MAY declare and call a method named `reduce_by`,
   5. if a non-`ReduceSchema` receiver uses phase-only tokens that have no
      ordinary `ArgList` parse, receiver identity resolution MUST fail with
      `REDUCE_BY_ARGUMENTS_INVALID`,
   6. phase-axis coverage/disjointness checks are semantic rules defined in
      `tensor-model.md`.
7. For a canonical `ReduceSchema` receiver, when one `ReduceByArgs` token
   sequence matches both `ReduceBySingle` and a one-entry `ReduceByPhases`, the
   phase parse takes precedence. Thus `.reduce_by((h, f))` and
   `.reduce_by(((h w), f))` are ordered phase forms on that receiver. On every
   other receiver, an ordinary tuple argument retains ordinary call semantics.

#### Call Surface Notes

1. Named arguments are part of baseline call grammar (`name = expr` in `ArgList`).
2. Positional arguments MUST appear before named arguments in v0.1.
3. Explicit call-site type arguments are part of baseline call grammar using `callee::[T0, ...](...)` surface:
   1. allowed on path/member-style callees,
   2. immediate-call only (`callee::[T]` as a standalone value is not baseline syntax),
   3. explicit lists are all-or-none for the callee generic arity.
4. `CatalogOwnerCallExpr` admits catalog-associated calls such as
   `Vec[i64].new()`, `Map[Str, i64].new()`, and `Set[Str].new()`:
   1. the path before `TypeArgs` MUST resolve in the type namespace to a
      language or catalog owner,
   2. the member MUST resolve to an associated callable supplied by the closed
      catalog,
   3. the call suffix is mandatory; `Vec[i64]` and `Vec[i64].new` are not value
      expressions in this surface,
   4. this parse never falls back to value indexing.
5. `DTypeBracketCallSuffix` is reserved for the dedicated tensor constructor dtype-bracket forms defined in `../stdlib/tensor.md`.
   1. Eligibility is determined from the resolved canonical callable identity,
      not the callee's source spelling. Imports, import aliases, and re-exports
      of an eligible constructor preserve that identity; a local binding or
      declaration that shadows the imported name does not.
   2. Name resolution MUST reject its use with an ordinary callee as `TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN`.
   3. Dtype-argument normalization and rejection are specified by
      `../stdlib/tensor.md`.
   4. General expression indexing such as `expr[index]` is not part of the v0.1 grammar.
6. A trailing closure block directly after a callee/member chain binds as call argument syntax:
   1. `TrailingClosureArg`: any `LambdaExpr`, including explicit `{ |...| ... }` and placeholder `{ ... _ ... }` forms.
7. Immediately after the opening `{` of `ExplicitLambdaExpr`, the already
   tokenized `||` token denotes an empty parameter list. The spaced form
   `{ | | body }` is equivalent; `{ |params| body }` remains the nonempty form.
   This parser interpretation does not retokenize `||` and does not affect
   logical OR elsewhere.
8. `SpawnThunkArg` is a context-specific surface for resolved `std::concurrency::spawn!` calls only; after any other callee it is not a valid call argument surface. For resolved `spawn!`, this context-specific form takes precedence over placeholder-closure recognition for trailing `{ Expr }` blocks without an explicit parameter list.
9. Call-argument validation (unknown/duplicate names, positional-after-named) is semantic and specified in `calls-references.md`.
10. Tuple projection uses a numeric postfix suffix:
   1. `expr.0`, `expr.1`, and left-associated chains such as `expr.0.1` are parse-valid tuple projection forms,
   2. tuple projection typing and diagnostics are specified in `type-system.md`,
   3. `expr[0]` is not tuple projection and is not v0.1 syntax.
11. Empty call or declaration delimiters denote zero arguments or parameters:
    `f()` passes no value, while `f(())` passes one Unit value. A Unit parameter
    uses a named binding such as `fn f(value: Unit)` because Unit parameter
    patterns are not accepted in v0.1.

#### Nominal Constructor Type-Argument Surface

1. `StructLiteralExpr` and `VariantConstructorExpr` deliberately name their
   nominal owner without `TypeArgs`.
2. Generic and non-generic owners therefore use the same constructor surface:
   `Cell { value: 1 }`, `Choice::Left(1)`, and `Choice::End`.
3. Constructor-head forms such as `Cell[i64] { value: 1 }`,
   `Choice[i64]::Left(1)`, and `Choice::[i64]::Left(1)` are not part of the v0.1
   grammar.
4. A type annotation or other independently established expected type supplies
   explicit generic-constructor context. Omitted constructor type-argument
   inference is defined in `type-system.md`.
5. This rule does not change immediate generic-call syntax
   (`callee::[T](...)`) or catalog-associated owner calls such as
   `Vec[i64].new()`.

#### Function Body Surface and Tail Policy

1. `FnBody` has exactly two forms in v0.1: expression-body (`= Expr`) and block-body (`BlockExpr`).
2. Expression-body form (`fn f(...) -> T = expr`) is a value-producing shorthand.
3. Block-body form does not use implicit tail-expression return in v0.1.
4. In block-body form, trailing expressions without `yield` parse as `ExprStmt` and do not define the block result value.
5. Block result-value semantics are defined in `control-flow.md` (`Block Expressions and \`yield\``).
6. Closure literals are separate syntax and may use closure-tail expression semantics defined in `calls-references.md`.
7. `IfExpr` branch bodies are ordinary `BlockExpr` forms. They do not receive branch-local tail-expression special casing; a value-producing branch must use `yield`.
8. `MatchArm` bodies are `Expr` forms. If an arm expression is a `BlockExpr`, that block must use `yield` to contribute a non-`Unit` arm value.
9. Bodyless `TraitFnDecl` is parse-valid only in trait bodies; top-level, struct, impl, property accessor, and extern function declarations use `FnDecl` / `ExternDecl`.

#### Hole and Placeholder Closure Surface

1. `HoleExpr` (`_`) is parse-valid expression surface for call-position partial application and placeholder closure shorthand.
2. `PlaceholderLambdaExpr` (`{ _.field }`, `{ f(_) }`) is a closure literal shorthand whose parameter set is inferred from hole occurrences.
3. `_` in wildcard pattern position or discard assignment target position is not a `HoleExpr`.
4. Hole binding order, valid hole contexts, and invalid-context diagnostics are defined in `calls-references.md`.

#### Assignment Target Surface

1. `AssignTarget` is intentionally parse-wide enough to allow precise semantic diagnostics.
2. The v0.1 assignable subset and `ASSIGN_TARGET_NOT_ASSIGNABLE` diagnostic are defined in `calls-references.md`.
3. Parser acceptance of `PostfixExpr` as `AssignTarget` does not make calls, indexing, `?`, or other value-producing postfix forms assignable.

#### Control-Head Brace Boundary

`ControlHeadExpr` is the condition, iterable, or scrutinee expression directly
followed by a braced control body in `if`, `while`, `for`, and `match`.

Rules:

1. `ControlHeadExpr` uses the ordinary `Expr` grammar subject only to the
   outer-depth brace boundary below.
2. Delimiter depth is measured from the start of the control head. An
   expression inside an already-open `(...)`, `[...]`, call argument list, or
   other delimited subexpression uses ordinary `Expr` parsing.
3. At the control head's outer delimiter depth, after a complete expression
   prefix has been parsed, the next ordinary `{` MUST terminate the
   `ControlHeadExpr` and open the required control body or `match` arm block.
4. That outer-depth brace MUST NOT instead be consumed as:
   1. the field-initializer body of `StructLiteralExpr`,
   2. the named payload of `VariantConstructorExpr`,
   3. an ordinary `TrailingClosureArg`.
5. A source expression that needs one of those forms at the control head's
   outer depth MUST place it inside parentheses or another explicit delimiter.
   For example:

   ```eglur
   if (Point{ x: 1 }.is_valid()) { ... }
   if check(Point{ x: 1 }) { ... }
   while (run { |x| is_ready(x) }) { ... }
   for x in (build { |v| v }) { ... }
   match (Token::Number { value: 1 }) { ... }
   ```

6. A brace encountered before any control-head expression prefix has been
   parsed MAY begin a brace-led primary such as `BlockExpr` or `LambdaExpr`.
   Thus `if { yield true } { ... }` has a block expression as its condition and
   the second brace opens the branch body.
7. Complete `TensorSchemaExpr` forms and an eligible `SpawnThunkArg` retain
   their context-owned classifications from the tensor-transform and call
   surface rules above. An ordinary trailing closure on a same-spelled
   identifier remains subject to Rule 4.
8. The parser MUST NOT use ordinary name or type resolution, greedy suffix
   consumption, or AST backtracking to choose between an ordinary
   control-body brace and any form prohibited by Rule 4.
9. A token sequence that attempts a Rule 4 form at the control head's outer
   depth before the required control block MUST fail with
   `PARSE_CONTROL_HEAD_BRACE_REQUIRES_GROUPING`. Implementations MAY inspect
   balanced tokens solely to select this diagnostic, but MUST NOT construct or
   type-check an alternate AST from that probe.

#### Conditional Expression Surface

1. `if <cond> { ... }` without `else` is parse-valid.
2. Using an `if` expression without `else` where a value is required is a semantic error (`IF_MISSING_ELSE_VALUE`) defined in `control-flow.md`.

#### Range Expression Surface

1. `RangeExpr` is parser surface for the `..` / `..=` tokens and for precise range diagnostics.
2. In v0.1, range syntax is semantically valid only as the iterable expression of `for <name> in <range>`, as defined in `control-flow.md`.
3. Range syntax in any other expression position MUST fail with `RANGE_EXPR_UNSUPPORTED`.
4. v0.1 defines no first-class `Range` value type and no implicit range-to-`Seq` materialization.
5. Missing range bounds or chained range syntax remain parser diagnostics (`PARSE_INVALID_RANGE_SYNTAX`).

#### Unsafe Surface Notes

1. v0.1 baseline unsafe surface includes:
   1. `ExternDecl`: `extern "<abi>" fn ...`,
   2. `UnsafeExpr`: `unsafe { ... }`.
2. `unsafe fn` and `unsafe impl/trait` are not part of v0.1 baseline grammar.
3. `pub extern ...` is parse-valid but semantically forbidden in v0.1 (`EXTERN_VISIBILITY_FORBIDDEN`).
4. `extern "<abi>" ...` is parse-valid for any string literal, but v0.1 semantic ABI subset enforcement rejects unsupported selectors (`EXTERN_ABI_UNSUPPORTED`).
5. `ExternDecl` keeps `ReturnType?` in parser grammar so omitted return types can receive `FN_RETURN_TYPE_REQUIRED`; v0.1 does not infer omitted extern returns as `Unit`.
6. Unsafe boundary semantics and diagnostics are specified in `unsafe-ffi.md`.

Ownership-profile note:

1. In v0.1 conformance baseline, `OwnExpr`, `MoveExpr`, and `DropOwnExpr` are required syntax.
2. Implementations MAY expose a non-baseline ownership-disabled mode.
3. In ownership-disabled mode:
   1. use of ownership syntax (`own`, `move`, `drop_own`) MUST fail with `OWNERSHIP_PROFILE_DISABLED`,
   2. use of `Own[...]` in type positions MUST fail with `OWNERSHIP_PROFILE_DISABLED`.
4. When ownership surface is enabled, `drop_own Identifier` is parse-valid only as the semantic-error surface for a missing explicit `move` marker; it MUST fail with `DROP_OWN_REQUIRES_MOVE`.
5. Function-call-like forms `own(<expr>)`, `move(<expr>)`, and `drop_own(<expr>)` are not part of v0.1 ownership syntax.

#### Expression Precedence

From highest to lowest:

1. Postfix (calls, `.member`, tuple projection, explicit or dtype-bracket call
   suffixes, trailing closures or spawn thunks, and `?`).
2. Unary (`drop_own`, `own`, `move`, `!`, unary `-`, references).
3. Multiplicative (`*`, `/`, `/.`, `%`).
4. Additive (`+`, `-`).
5. Range (`..`, `..=`) as non-associative binary.
6. Comparison (`==`, `!=`, `<`, `<=`, `>`, `>=`).
7. Logical `&&`.
8. Logical `||`.

Structured forms such as `if`, `match`, `result`, blocks, and lambdas are
`PrimaryExpr` operands. They are not a precedence tier below logical OR and may
receive any postfix suffix allowed by `PostfixExpr`.

#### Pattern Rest Restrictions

1. `..` in tuple/variant positional patterns MUST appear at most once.
2. Positional `..` MUST appear only in tail position in v0.1.
3. In struct and named-payload variant patterns, `..` MAY appear at most once
   and has no positional requirement.

Violations MUST fail with `PARSE_INVALID_REST_POSITION`.
For structurally valid patterns, `..` omission semantics and exact arity/field
rules are defined in `control-flow.md`.

#### Pattern Use-Site Restrictions

1. The full `Pattern` grammar is the baseline `match` pattern surface.
2. `Param` and `LetStmt` reuse `Pattern` as parse-valid diagnostic surface, but v0.1 semantic acceptance is narrower than the grammar.
3. Function, method, trait, and extern parameters MUST use identifier binding patterns only (`PARAM_PATTERN_UNSUPPORTED` otherwise).
4. Local `let`/`let mut` binding patterns MAY use identifiers, wildcard `_`,
   Unit pattern `()`, or tuple patterns recursively composed only of accepted
   local binding patterns.
5. Struct, variant, literal, alias, or-pattern, and rest `..` patterns in local binding position are not part of v0.1 (`LET_PATTERN_UNSUPPORTED` otherwise).
6. The semantic rules for accepted local binding patterns are defined in `type-system.md`.

#### Enum Variant Visibility Restriction

1. Variant-level visibility modifiers are not part of v0.1 grammar.
2. `pub` before enum variants (for example `enum E { pub A }`) MUST fail with `ENUM_VARIANT_VISIBILITY_FORBIDDEN`.

#### Enum Variant Payload Surface

1. Enum variants support:
   1. unit form (`V`),
   2. positional payload form (`V(T1, T2, ...)`),
   3. named payload form (`V { f1: T1, f2: T2, ... }`).
2. Named payload declarations require at least one typed field.

#### Range Syntax Restrictions

1. Range expressions are binary forms: `<start> .. <end>` or `<start> ..= <end>`.
2. Missing start or end bounds are invalid in v0.1.
3. Chained range operators without parentheses are invalid in v0.1.

Violations MUST fail with `PARSE_INVALID_RANGE_SYNTAX`.

#### Parser Resource Limits and Recovery

1. The v0.1 portable parser profile has an active recursive-grammar budget of
   256. A prefix-unary expression with 255 operators before its primary remains
   within that budget. Attempting the next recursive descent, including the
   chain with 256 prefix-unary operators, MUST fail with
   `PARSE_RESOURCE_LIMIT_EXCEEDED` and data
   `resource=recursive_depth`, `limit=256`, and
   `observed_at_least=257`.
2. An implementation MAY impose deterministic delimiter-index or combined
   parser-event/marker limits in addition to Rule 1. A limit MUST be derived
   from source or token structure, not elapsed wall time.
3. Exhausting a parser-owned structural limit MUST fail with
   `PARSE_RESOURCE_LIMIT_EXCEEDED`, not a grammar-invalid code. `resource` MUST
   identify the exhausted resource; `limit` MUST report the active limit; and
   `observed_at_least` MUST report the smallest value established beyond it.
4. The stable `resource` values in v0.1 are `recursive_depth`,
   `delimiter_depth`, and `event_count`; `event_count` includes marker-owned
   event slots. Additional values require a diagnostic contract revision.
5. A bounded delimiter probe needed to classify an enclosing grammar form MAY
   establish its resource failure before that unresolved form receives a
   grammar diagnostic. An independently established earlier root remains
   ordered under `../errors/ordering.md`.
6. Recovery scanning MUST either advance over the finite token stream or stop.
   Recovery breadth, deterministic diagnostic truncation, and poisoned facts
   otherwise follow `../errors/ordering.md`. Conformance of one primary
   diagnostic does not establish multi-error recovery quality.

#### Parser Diagnostics

Parser failures MUST use stable compile-time diagnostic codes defined in `../ERRORS.md`:

`PARSE_UNEXPECTED_TOKEN`, `PARSE_UNCLOSED_DELIMITER`, `PARSE_EXPECTED_EXPRESSION`, `PARSE_EXPECTED_PATTERN`, `PARSE_INVALID_REST_POSITION`, `PARSE_INVALID_RANGE_SYNTAX`, `PARSE_CONTROL_HEAD_BRACE_REQUIRES_GROUPING`, `PARSE_RESOURCE_LIMIT_EXCEEDED`, `STRUCT_FIELD_DEFAULT_FORBIDDEN`, `ENUM_VARIANT_VISIBILITY_FORBIDDEN`.

#### Module and Resolution Diagnostics

Module loading, visibility, and name resolution failures MUST use stable compile-time diagnostic codes defined in `../ERRORS.md`:

`MODULE_PATH_MISMATCH`, `MODULE_PATH_COMPONENT_INVALID`, `MODULE_ROOT_RESERVED`,
`IMPORT_NOT_FOUND`, `IMPORT_SUPER_AT_ROOT`, `IMPORT_BANG_ALIAS_MUST_BANG`,
`IMPORT_ROOT_CONFLICT`, `NAME_AMBIGUOUS`, `NAME_NOT_FOUND`,
`VISIBILITY_VIOLATION`, `EXTERN_VISIBILITY_FORBIDDEN`, `TYPE_ALIAS_CYCLE`,
`PACKAGE_MANIFEST_MISSING`, `PACKAGE_SRC_ROOT_MISSING`,
`PACKAGE_MANIFEST_PACKAGE_TABLE_MISSING`, `PACKAGE_MANIFEST_NAME_MISSING`,
`PACKAGE_MANIFEST_NAME_INVALID`, `PACKAGE_MANIFEST_VERSION_MISSING`,
`PACKAGE_MANIFEST_VERSION_INVALID`, `PACKAGE_DEP_KEY_INVALID`,
`PACKAGE_VERSION_RANGE_UNSUPPORTED`, `PACKAGE_DEP_SOURCE_CONFLICT`,
`PACKAGE_REGISTRY_ALIAS_INVALID`, `PACKAGE_REGISTRY_SELECTOR_INVALID`,
`PACKAGE_DEP_VERSION_INVALID`, `PACKAGE_DEP_PATH_INVALID`,
`PACKAGE_DEP_PATH_UNRESOLVED`, `PACKAGE_DEP_DUPLICATE`, `PACKAGE_DEP_CYCLE`,
`PACKAGE_VERSION_CONFLICT`, `PACKAGE_NAME_CONFLICT`, `PACKAGE_LOCK_MISSING`,
`PACKAGE_LOCK_INVALID`, `PACKAGE_LOCK_OUT_OF_SYNC`,
`PACKAGE_REGISTRY_RELEASE_MISSING`, `PACKAGE_REGISTRY_TRANSPORT_FAILED`,
`PACKAGE_REGISTRY_LIMIT_EXCEEDED`, `PACKAGE_REGISTRY_INTEGRITY_MISMATCH`,
`PACKAGE_REGISTRY_TREE_INVALID`.

#### Attribute-Lite Parse Scope

v0.1 parses Rust-like attribute surface as a constrained, forward-compatible grammar feature.

Rules:

1. Attribute blocks use `#[...]` surface and MAY appear as a contiguous prefix list.
2. In top-level items, attributes are parse-valid only on:
   1. `fn`,
   2. `struct`,
   3. `impl`,
   4. `enum`,
   5. `pub fn`,
   6. `pub struct`,
   7. `pub enum`.
3. For top-level declarations with visibility, attributes MUST appear before `pub`.
4. `TraitItem` parse-valid attribute targets are `TraitFnDecl` only.
5. `StructItem` parse-valid attribute targets are `StructFnDecl` only. When a struct method has visibility, attributes MUST appear before `pub`.
6. `ImplItem` parse-valid attribute targets are `FnDecl` only.
7. Enum variant declarations are parse-valid attribute targets.
8. Attribute name grammar is general (`AttrPath`) for forward compatibility.
9. Attribute argument grammar is intentionally general in v0.1 parser:
   1. bare meta item (`Path` or `Path(...)`),
   2. key-value item (`name = value`),
   3. literal payloads inside nested meta arguments.
10. `derive(...)` argument entries MUST parse under this grammar. Path-only
    derive entries and derive target restrictions are specified in
    `algebraic-data.md` under "Derive Attribute Semantics".
11. v0.1 semantic support remains narrower than parse support. Attribute target
    applicability and supported attribute semantics are specified in
    `algebraic-data.md` under "Attribute Target Applicability", "Derive
    Attribute Semantics", and "Test Function Attributes".
12. Post-v0.1 attribute extension slots (custom derive providers, `fn`/`impl` attributes, tensor-device metadata attributes, and representation/FFI attributes) are tracked by `LANG-OI-019` in `../open-items/language.md`, with candidate designs in `../../roadmap/language/attributes.md`.
13. Slots listed in Rule 12 are outside the baseline and MUST NOT be required for v0.1 conformance.
