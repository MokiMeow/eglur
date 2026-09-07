# `std::io` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Types

```eglur-catalog
opaque type Path
opaque type PathError
opaque type IoError
sum type PathErrorKind = Empty | ContainsNul
sum type IoErrorKind = NotFound | AlreadyExists | PermissionDenied | ReadOnly | InvalidPath | InvalidData | WrongKind | DirectoryNotEmpty | PolicyRejected | CrossDevice | ResourceBusy | BrokenPipe | Interrupted | TimedOut | StorageFull | ResourceExhausted | Unsupported | Other
sum type TextError = Io(IoError) | InvalidUtf8
sum type FileKind = File | Dir | Symlink | Other
sum type LinkMode = Follow | Reject
sum type DirTraversal = FollowLinks | RejectLinks
sum type DestinationPolicy = RejectExisting | ReplaceCompatible
opaque type PathStat
opaque type ProcessSnapshot
opaque type ByteReader
opaque type ByteWriter
opaque type TextReader
opaque type TextWriter
```

These declarations use the closed [standard-library catalog metalanguage](catalog-signatures.md);
they are not Eglur source declarations.

`ByteReader`, `ByteWriter`, `TextReader`, and `TextWriter` are resource payload
types under `../language/ownership-boundary.md`. Source-visible acquisition APIs
expose them only as `Own[...]` roots or scoped borrows; the raw payload types have
no source constructor or ownership-unwrapping operation.

Equality contracts:

1. `FileKind`, `LinkMode`, `DirTraversal`, and `DestinationPolicy` implement
   `Eq`; two values of any one type are equal iff they are the same variant.
2. `PathErrorKind` and `IoErrorKind` implement `Eq`; two values of either type
   are equal iff they are the same variant.
3. `Path` does not implement `Eq` in v0.1.
4. `PathError`, `IoError`, `TextError`, `PathStat`, `ProcessSnapshot`, and the resource payload types in this module do not implement `Eq` unless a later version explicitly defines their equality identity.

### Operational Error Observation

```eglur-catalog
fn PathError.kind(self: &Self) -> PathErrorKind
fn IoError.kind(self: &Self) -> IoErrorKind
```

Rules:

1. `PathError.kind` and `IoError.kind` are pure and total. They observe only the
   receiver and MUST NOT inspect current process or filesystem state.
2. `PathErrorKind` is closed over the baseline `Path.parse_strict` failures:
   1. empty input maps to `Empty`,
   2. an input containing NUL maps to `ContainsNul`.
3. `IoErrorKind` is a portable recovery category, not a host error-code mirror:

   | Kind | Cause category |
   | --- | --- |
   | `NotFound` | A required path or entry is absent, including a dangling link when the operation requires its target. This does not apply when the operation defines absence as success. |
   | `AlreadyExists` | Create-only or no-replace behavior rejects an existing entry. |
   | `PermissionDenied` | Credentials or an access-control policy deny the operation. |
   | `ReadOnly` | The operation requires mutation on read-only storage. |
   | `InvalidPath` | A path accepted as lexical `Path` data cannot be used under host path rules, including an invalid component or an unresolvable link loop. |
   | `InvalidData` | Host-provided process or filesystem data cannot be represented losslessly by the required Eglur result type. |
   | `WrongKind` | A path resolves to a filesystem kind incompatible with the operation. |
   | `DirectoryNotEmpty` | The operation requires an empty directory but observes a non-empty one. |
   | `PolicyRejected` | An explicit Eglur policy rejects the operation, including selected no-follow behavior or `copy!` of one entry onto itself. |
   | `CrossDevice` | `rename!` cannot cross the relevant filesystem or device boundary. |
   | `ResourceBusy` | A required host resource is temporarily or administratively busy. |
   | `BrokenPipe` | A write or flush observes a closed pipe or equivalent stream endpoint. |
   | `Interrupted` | A host wait or I/O operation is interrupted and the baseline operation does not complete it transparently. |
   | `TimedOut` | A host I/O deadline or timeout expires. |
   | `StorageFull` | Storage space or storage quota is exhausted. |
   | `ResourceExhausted` | Another required finite host resource, such as handles or memory, is exhausted. |
   | `Unsupported` | The operation is unavailable on the implementation target. |
   | `Other` | The cause is implementation-specific, ambiguous, or otherwise has no portable category above. |

4. A condition that matches a named row in Rule 3 MUST use that row's variant.
   Implementations MUST use `Other` only when no named category applies.
5. Implementations MUST NOT expose raw host error numbers, host-localized
   messages, or target-specific error names through `kind()`.
6. `PathError`, `PathErrorKind`, `IoError`, `IoErrorKind`, and `TextError` MUST
   implement `Display`.
7. Kind display is stable and exact:

   | Kind | `Display.to_str` result |
   | --- | --- |
   | `PathErrorKind::Empty` | `"empty"` |
   | `PathErrorKind::ContainsNul` | `"contains NUL"` |
   | `IoErrorKind::NotFound` | `"not found"` |
   | `IoErrorKind::AlreadyExists` | `"already exists"` |
   | `IoErrorKind::PermissionDenied` | `"permission denied"` |
   | `IoErrorKind::ReadOnly` | `"read only"` |
   | `IoErrorKind::InvalidPath` | `"invalid path"` |
   | `IoErrorKind::InvalidData` | `"invalid data"` |
   | `IoErrorKind::WrongKind` | `"wrong entry kind"` |
   | `IoErrorKind::DirectoryNotEmpty` | `"directory not empty"` |
   | `IoErrorKind::PolicyRejected` | `"policy rejected"` |
   | `IoErrorKind::CrossDevice` | `"cross-device operation"` |
   | `IoErrorKind::ResourceBusy` | `"resource busy"` |
   | `IoErrorKind::BrokenPipe` | `"broken pipe"` |
   | `IoErrorKind::Interrupted` | `"interrupted"` |
   | `IoErrorKind::TimedOut` | `"timed out"` |
   | `IoErrorKind::StorageFull` | `"storage full"` |
   | `IoErrorKind::ResourceExhausted` | `"resource exhausted"` |
   | `IoErrorKind::Unsupported` | `"unsupported"` |
   | `IoErrorKind::Other` | `"other"` |

8. `PathError.to_str()` is exactly
   `concat(["path error: ", kind().to_str()])`; `IoError.to_str()` is exactly
   `concat(["I/O error: ", kind().to_str()])`.
9. `TextError::Io(error)` has exactly the same display as `error`.
   `TextError::InvalidUtf8` displays as `"text error: invalid UTF-8"`.
10. Error and kind display MUST NOT append paths, operations, host messages, raw
   error numbers, or implementation-defined context.
11. An `IoErrorKind` describes cause only. It MUST NOT strengthen any operation's
    progress, rollback, retry, ownership, or resource-retirement contract.

### Path Construction and Conversion (pure)

```eglur-catalog
fn Path.from_str(s: Str) -> Path
fn Path.parse_strict(s: Str) -> Res[Path, PathError]
fn Path.to_str(self: &Self) -> Str

fn Path.is_absolute(self: &Self) -> Bool
fn Path.join(self: &Self, child: &Path) -> Path
fn Path.parent(self: &Self) -> Opt[Path]
fn Path.file_name(self: &Self) -> Opt[Str]
fn Path.extension(self: &Self) -> Opt[Str]
```

Rules:

1. `Path.from_str` is total and MUST NOT fail.
2. `Path.from_str` MUST NOT perform filesystem access.
3. `Path.parse_strict` is pure and MUST NOT perform filesystem access.
4. `Path.parse_strict` MUST reject at least:
   1. empty input string,
   2. strings containing NUL (`\u{0000}`).
5. In v0.1 baseline, `Path.parse_strict` MUST accept every other textual path unchanged as lexical data.
   1. repeated `/` separators are accepted as lexical content,
   2. trailing `/` is accepted as lexical content,
   3. `.` and `..` segments are accepted as lexical content,
   4. `\` remains an ordinary character and does not cause rejection,
   5. non-ASCII Unicode path text is accepted unchanged.
6. `Path.parse_strict` failure returns `Err(PathError)`.
7. `Path.to_str` returns the stored textual path representation for diagnostics/logging.
8. Path lexical separator in v0.1 is `/`:
   1. `\` is treated as an ordinary character, not a separator.
9. `Path.is_absolute` returns true iff `Path.to_str()` starts with `/`.
10. `Path.join` is pure lexical composition and MUST NOT access filesystem:
   1. if `child.is_absolute()` is true, result is `child` unchanged,
   2. else if `self.to_str()` is empty, result is `child` unchanged,
   3. else if `self.to_str()` is `/`, result is `Path.from_str(concat(["/", child.to_str()]))`,
   4. else if `self.to_str()` ends with `/`, result is `Path.from_str(concat([self.to_str(), child.to_str()]))`,
   5. otherwise result is `Path.from_str(concat([self.to_str(), "/", child.to_str()]))`,
   6. these rules also apply when `child.to_str()` is empty and MUST preserve
      repeated separators, dot segments, and `\` characters without
      normalization.
11. `Path.parent` is pure lexical parent extraction:
    1. for `/`, result is `None`,
    2. trailing `/` characters are ignored for parent computation except when the whole path is `/`,
    3. if no separator exists after trailing-separator trimming, result is `None`,
    4. if separator-trimmed parent prefix is empty, result is `Some(Path.from_str("/"))`,
    5. otherwise returns `Some(Path.from_str(prefix))`.
12. `Path.file_name` is pure lexical last-segment extraction:
    1. for `/`, result is `None`,
    2. trailing `/` characters are ignored for filename computation except when the whole path is `/`,
    3. result is last segment after `/`,
    4. if no segment remains, result is `None`.
13. `Path.extension` derives from `file_name`:
    1. if `file_name` is `None`, result is `None`,
    2. extension is substring after last `.` in filename only when:
       1. `.` is not first character of filename,
       2. `.` is not last character of filename,
    3. otherwise result is `None`.
14. Filesystem-touching helpers remain the authority for filesystem availability/permission failure:
    1. byte and metadata helpers return `IoError`,
    2. text helpers return `TextError` and wrap filesystem failures as `Io(_)`.
15. v0.1 baseline `Path` is lexical-only:
    1. no implicit lexical normalization (for example removing `.` / `..`) is performed by baseline path helpers,
    2. no filesystem-backed canonicalization is part of baseline `Path` API surface.
16. Lossless host-native path carriers and bridge APIs (for example native-separator or native-name conversion helpers) are deferred non-baseline in v0.1.
17. Implementations MAY provide non-baseline extension APIs for path normalization/canonicalization/native bridge, but REQUIRED conformance MUST NOT depend on those APIs.

### Process Environment Access (bang capture + fallible pure accessors)

```eglur-catalog
fn process_snapshot!() -> ProcessSnapshot

fn invocation_arg!() -> Res[Opt[Str], IoError]
fn args!() -> Res[Seq[Str], IoError]
fn env_get!(key: Str) -> Res[Opt[Str], IoError]
fn cwd!() -> Res[Path, IoError]

fn ProcessSnapshot.invocation_arg(self: &Self) -> Res[Opt[Str], IoError]
fn ProcessSnapshot.args(self: &Self) -> Res[Seq[Str], IoError]
fn ProcessSnapshot.env_get(self: &Self, key: &Str) -> Res[Opt[Str], IoError]
fn ProcessSnapshot.cwd(self: &Self) -> Res[Path, IoError]
```

Rules:

1. `process_snapshot!` is bang and MUST capture independent observations of the
   host argument vector, environment, and cwd without filesystem mutation. It
   returns the resulting opaque `ProcessSnapshot` directly; source-specific
   acquisition or representation failures are retained for the corresponding
   pure accessor rather than failing the whole capture.
2. The capture interval is the dynamic extent of the `process_snapshot!` call:
   1. every retained observation MUST occur during that interval,
   2. `invocation_arg` and `args` MUST be derived from one host argument-vector
      observation,
   3. environment and cwd observations need not occur at the same instant or in
      a specified order,
   4. the snapshot therefore does not claim one atomically linearizable
      argument/environment/cwd instant,
   5. failure to acquire or represent one source MUST NOT prevent the snapshot
      from retaining independently observable outcomes for the other sources.
3. `ProcessSnapshot` accessors are pure and deterministic for that snapshot:
   1. they MUST NOT inspect current host state or retry a captured operation,
   2. repeated calls with the same receiver and, for `env_get`, the same key MUST
      return the same retained success or failure outcome.
4. Scalar projection of host-provided process data is lossless:
   1. encoding a returned scalar sequence through the corresponding host
      argument, environment, or path boundary MUST recover the same native
      byte/code-unit sequence,
   2. implementations MUST NOT omit data, substitute `U+FFFD`, expose escaped
      diagnostic text as literal data, normalize Unicode, change case, or convert
      path separators,
   3. host data without a lossless scalar representation returns
      `Err(IoError)` with `IoErrorKind::InvalidData` from only the accessor or
      direct helper whose result requires that data.
5. Argument-vector projection is defined as follows:
   1. `invocation_arg` is the optional zeroth invocation element supplied by the
      host process boundary,
   2. it is descriptive invocation text, not an executable path or executable
      identity guarantee,
   3. absence of a host invocation element returns `Ok(None)`,
   4. `args` contains only user arguments after that optional element, in their
      supplied order,
   5. an unrepresentable invocation element fails only `invocation_arg`,
   6. if any user argument is unrepresentable, `args` returns
      `Err(IoError)` whose kind is `IoErrorKind::InvalidData` and MUST NOT expose
      a partial sequence.
6. Environment lookup is exact over the captured native entries:
   1. a native key is an exact match iff its native byte/code-unit sequence
      equals the target encoding of the requested scalar key,
   2. v0.1 performs no key normalization, case folding, or case conversion,
   3. no exact matching entry returns `Ok(None)`,
   4. one exact match with a representable value returns `Ok(Some(value))`,
   5. an unrepresentable matching value returns `Err(IoError)` whose kind is
      `IoErrorKind::InvalidData`,
   6. multiple exact matching entries return `Err(IoError)` whose kind is
      `IoErrorKind::InvalidData`
      because `Opt[Str]` cannot represent that state without discarding data,
   7. an unrepresentable or duplicate entry whose native key does not exactly
      match the requested key MUST NOT affect the result,
   8. if the host environment observation itself cannot be acquired independently
      of entry projection, every lookup on that snapshot returns the retained
      acquisition error.
7. Cwd projection is defined as follows:
   1. success returns the non-empty host-provided spelling of the current working
      directory observed during capture,
   2. the spelling MUST satisfy the lossless scalar projection rule and MUST NOT
      be normalized, canonicalized, case-converted, or separator-converted,
   3. host-rootedness is distinct from the `/`-only lexical predicate
      `Path.is_absolute`; a successful cwd value is not required to make
      `Path.is_absolute()` return `true`,
   4. success does not guarantee that the same spelling will continue to resolve
      to the captured directory after concurrent host namespace or cwd changes,
   5. an empty or unrepresentable host spelling returns
      `Err(IoError)` whose kind is `IoErrorKind::InvalidData`.
8. Each direct helper performs one fresh observation of only its named source:
   1. `invocation_arg!` and `args!` apply the corresponding argument-vector
      projection,
   2. `env_get!` observes only the host environment state required to apply the
      exact lookup and projection contract in Rule 6 to the requested key,
   3. `cwd!` observes and projects only cwd,
   4. direct helpers are not desugarings through `process_snapshot!`, and failure
      in an unrelated process source MUST NOT affect their results.
9. Direct-helper failure returns `Err(IoError)` and MUST NOT terminate via the
   runtime-error channel.
10. Process-environment APIs do not imply stable global process state across
    separate observations:
    1. values from separate direct-helper or `process_snapshot!` calls are not
       required to compare equal,
    2. the host environment or cwd MAY change between observations.
11. v0.1 baseline process-environment surface is read-only:
   1. mutation APIs such as `set_env!`, `remove_env!`, and `set_cwd!` are deferred non-baseline in v0.1,
   2. REQUIRED v0.1 conformance MUST NOT depend on process-environment mutation APIs.

### Byte Streaming I/O (bang, fallible)

```eglur-catalog
fn open_reader!(path: Path, links: LinkMode) -> Res[Own[ByteReader], IoError]
fn open_writer!(path: Path, links: LinkMode) -> Res[Own[ByteWriter], IoError]

fn with_reader![T](path: Path, links: LinkMode, f: callback[scoped-resource](&mut Own[ByteReader]) -> Res[T, IoError]) -> Res[T, ScopedFailure[IoError]]
fn with_writer![T](path: Path, links: LinkMode, f: callback[scoped-resource](&mut Own[ByteWriter]) -> Res[T, IoError]) -> Res[T, ScopedFailure[IoError]]

fn Own[ByteReader].read_chunk!(self: &mut Self, max_bytes: PositiveNat) -> Res[Opt[Vec[u8]], IoError]
fn Own[ByteReader].close!(self: Self) -> Res[Unit, IoError]

fn Own[ByteWriter].write_all!(self: &mut Self, xs: Vec[u8]) -> Res[Unit, IoError]
fn Own[ByteWriter].flush!(self: &mut Self) -> Res[Unit, IoError]
fn Own[ByteWriter].close!(self: Self) -> Res[Unit, IoError]
```

Rules:

1. All byte-streaming APIs are bang and return `Res[..., IoError]`.
2. `open_reader!`:
   1. opens an existing readable regular-file target under the supplied
      `links` policy,
   2. returns `Err(IoError)` on missing/unreadable/non-file targets,
   3. follows the ownership-producing acquisition convention in
      `conventions.md`; an `Err` MUST NOT create, truncate, or write target data
      as an operation-induced effect, but it MAY follow a host-observable access
      attempt.
3. `open_writer!`:
   1. opens a writable regular-file target with create-or-truncate semantics
      under the supplied `links` policy,
   2. returns `Err(IoError)` when path cannot be opened for writing,
   3. follows the ownership-producing acquisition convention in
      `conventions.md`,
   4. a definite link-policy, kind, or path-resolution rejection before creation
      or truncation leaves the selected path state unchanged,
   5. after any later acquisition failure, an initially absent selected target
      MAY remain absent or be a new empty regular file, while an existing
      selected regular file MAY retain its original contents or remain the same
      object truncated to empty,
   6. creation or truncation is direct: the operation MUST NOT remove a newly
      created entry, restore old contents, or leave an auxiliary staging or
      backup entry as compensating rollback.
4. Successful `open_reader!` and `open_writer!` results establish resource roots; callers MUST transfer or close each root before its obligation region exits.
5. `with_reader!` and `with_writer!` are the scoped forms of the corresponding
   open operations, inherit their acquisition and terminal-close effects, and
   follow the scoped resource callback contract in `conventions.md`.
6. The `callback[scoped-resource]` parameters use the effectful, non-escaping contract in `conventions.md`.
7. `Own[ByteReader].read_chunk!`:
   1. `max_bytes` is a `PositiveNat`; signed runtime values MUST be converted with `PositiveNat.from_i64` before use, with failures reported as `DOMAIN_INVALID_VALUE`,
   2. on successful non-EOF read, returns `Ok(Some(chunk))` where `chunk.len()` is at least `1` and at most `max_bytes.to_nat()`,
   3. on EOF, returns `Ok(None)`,
   4. once one call returns `Ok(None)`, every later `read_chunk!` call on that
      reader MUST also return `Ok(None)`,
   5. logical delivery, `Err(IoError)` progress, retry, and continued ownership
      follow the ordered byte-chunk reader convention in `conventions.md`.
8. `Own[ByteWriter].write_all!`:
   1. applies the payload-completing writer convention in `conventions.md` to
      `xs`,
   2. returns `Ok(())` only after all bytes have been accepted, or
      `Err(IoError)` under that convention,
   3. accepts an empty payload and MUST return `Ok(())`.
9. `Own[ByteWriter].flush!` follows the ordered writer-flush convention in
   `conventions.md` and reports failure as `Err(IoError)`.
10. Every byte `close!` follows the terminal consuming-close convention in
    `conventions.md` and returns its result through `Res`.
11. A closed root no longer exists as a source value; repeated close, post-close I/O, and post-close inspection MUST fail statically as ownership use after discharge rather than through `IoError`.
12. `drop_own` on a byte resource root fails with `RESOURCE_DROP_FORBIDDEN`.
13. Byte-streaming APIs do not perform implicit text decoding/encoding; callers requiring text semantics use `read_text!`/`write_text!` or explicit adapters.
14. Random-access byte APIs (for example `seek!`, `read_exact!`, `read_at!`, `write_at!`) are deferred non-baseline in v0.1.
15. REQUIRED v0.1 conformance MUST NOT depend on random-access byte APIs.

### Text Streaming Adapters (bang, fallible, UTF-8 baseline)

```eglur-catalog
fn open_text_reader!(path: Path, links: LinkMode) -> Res[Own[TextReader], TextError]
fn open_text_writer!(path: Path, links: LinkMode) -> Res[Own[TextWriter], TextError]

fn with_text_reader![T](path: Path, links: LinkMode, f: callback[scoped-resource](&mut Own[TextReader]) -> Res[T, TextError]) -> Res[T, ScopedFailure[TextError]]
fn with_text_writer![T](path: Path, links: LinkMode, f: callback[scoped-resource](&mut Own[TextWriter]) -> Res[T, TextError]) -> Res[T, ScopedFailure[TextError]]

fn Own[TextReader].read_line!(self: &mut Self) -> Res[Opt[Str], TextError]
fn Own[TextReader].close!(self: Self) -> Res[Unit, TextError]

fn Own[TextWriter].write_str!(self: &mut Self, s: Str) -> Res[Unit, TextError]
fn Own[TextWriter].write_line!(self: &mut Self, s: Str) -> Res[Unit, TextError]
fn Own[TextWriter].flush!(self: &mut Self) -> Res[Unit, TextError]
fn Own[TextWriter].close!(self: Self) -> Res[Unit, TextError]
```

Rules:

1. Text streaming adapters are REQUIRED in v0.1 baseline.
2. Text streaming encoding is UTF-8 only in v0.1:
   1. adapter APIs MUST NOT accept alternate encoding selectors,
   2. non-UTF-8 decode failure MUST return `Err(InvalidUtf8)`.
3. `open_text_reader!` and `open_text_writer!` use the same file-open and
   `LinkMode` semantics as `open_reader!`/`open_writer!`; open failures MUST
   return `Err(Io(_))`, including the corresponding ownership-producing
   acquisition and operation-induced file-state rules.
4. Successful open results establish resource roots under the same transfer and exact-close rules as byte handles.
5. `with_text_reader!` and `with_text_writer!` are scoped forms, inherit the
   corresponding acquisition and terminal-close effects, and follow
   `conventions.md`, including acquisition/body/close error precedence.
6. `Own[TextReader].read_line!`:
   1. consumes logical lines in source order and returns `Ok(Some(s))` for the
      next complete decoded UTF-8 line,
   2. a logical line ends at `\n` or at EOF; when `\n` is immediately preceded
      by `\r`, the pair is one `\r\n` terminator, while a lone `\r` remains line
      content,
   3. strips one trailing line terminator when present:
      1. `\n`,
      2. `\r\n`,
   4. if EOF follows pending bytes without a line terminator, returns those
      bytes as the final decoded line before returning `Ok(None)` on a later
      call,
   5. at EOF with no pending bytes, returns `Ok(None)`; after that result, every
      later call MUST also return `Ok(None)`,
   6. a complete valid line acquired before a later underlying I/O failure MUST
      be returned before that failure is surfaced,
   7. on `Err(Io(_))`:
      1. bytes accumulated for the next incomplete line MUST be retained and
         MUST NOT be skipped or duplicated,
      2. a later successful call MUST resume that same logical line from the
         retained prefix,
      3. the call does not discharge or statically invalidate the reader root,
         but no later-read success is guaranteed,
   8. when the next logical line is conclusively not valid UTF-8, returns
      `Err(InvalidUtf8)` and enters a terminal decode-error state:
      1. every complete valid line ordered before the malformed input MUST be
         returned first,
      2. every later `read_line!` call on that reader MUST return
         `Err(InvalidUtf8)` without reading or logically consuming additional
         source bytes,
      3. the reader root remains type-valid and subject to its transfer or
         consuming-close obligation,
   9. an incomplete UTF-8 prefix followed by `Err(Io(_))` MUST NOT be classified
      as `InvalidUtf8` solely because it is incomplete; invalidity requires a
      contradictory byte or EOF.
7. `Own[TextWriter].write_str!` applies the payload-completing writer convention
   in `conventions.md` to the UTF-8 bytes of `s` and returns write failures as
   `Err(Io(_))`. Progress is byte-granular, so an externally visible prefix
   after `Err` MAY end inside one encoded Unicode scalar and need not itself be
   valid UTF-8.
8. `Own[TextWriter].write_line!` applies that convention to one payload equal to
   the UTF-8 encoding of `concat([s, "\n"])`. On `Ok(())`, its observable output
   is equivalent to `write_str!(concat([s, "\n"]))`; on `Err`, the appended
   newline byte MAY or MAY NOT have been accepted or made externally visible.
9. `Own[TextWriter].flush!` follows the ordered writer-flush convention in
   `conventions.md` for the accepted UTF-8 bytes and reports failure as
   `Err(Io(_))`.
10. Every text `close!` follows the terminal consuming-close convention in
    `conventions.md`; repeated or post-close use fails statically as ownership
    use after discharge.
11. `drop_own` on a text resource root fails with `RESOURCE_DROP_FORBIDDEN`.
12. Text streaming adapters MUST remain value-channel APIs (`Res`) and MUST NOT report decode/open/write/close failures through runtime-error channel.

### Standard Text Stream Handles (bang, fallible)

```eglur-catalog
fn stdin!() -> Res[Own[TextReader], TextError]
fn stdout!() -> Res[Own[TextWriter], TextError]
fn stderr!() -> Res[Own[TextWriter], TextError]

fn with_stdin![T](f: callback[scoped-resource](&mut Own[TextReader]) -> Res[T, TextError]) -> Res[T, ScopedFailure[TextError]]
fn with_stdout![T](f: callback[scoped-resource](&mut Own[TextWriter]) -> Res[T, TextError]) -> Res[T, ScopedFailure[TextError]]
fn with_stderr![T](f: callback[scoped-resource](&mut Own[TextWriter]) -> Res[T, TextError]) -> Res[T, ScopedFailure[TextError]]
```

Rules:

1. Standard text stream handles are REQUIRED in v0.1 baseline.
2. `stdin!`, `stdout!`, and `stderr!` acquire wrappers over the current process
   standard text streams and return `Err(Io(_))` when acquisition fails. They
   follow the ownership-producing acquisition convention in `conventions.md`;
   terminal retirement of a provisional wrapper MUST NOT close or invalidate
   the underlying process standard stream.
3. Standard text stream handles use the same UTF-8 decoding/encoding and `TextReader` / `TextWriter` semantics as text streaming adapters.
4. `close!` on a standard text stream handle follows the terminal
   consuming-close convention and is wrapper-local only:
   1. closing one handle MUST NOT close or invalidate the underlying process standard stream,
   2. later acquisition of the same standard stream kind MUST remain valid when the underlying process stream itself is still available.
5. `with_stdin!`, `with_stdout!`, and `with_stderr!` are scoped forms following
   `conventions.md`; they inherit the corresponding wrapper-acquisition effects,
   and their consuming close releases only the acquired wrapper.
6. `stdout!` and `stderr!` use the same `write_str!`, `write_line!`, `flush!`, and consuming `close!` rules as other `Own[TextWriter]` roots.
7. `stdin!` uses the same `read_line!` and consuming `close!` rules as other `Own[TextReader]` roots.


### Console I/O (bang)

```eglur-catalog
fn print!(s: Str) -> Res[Unit, TextError]
fn eprint!(s: Str) -> Res[Unit, TextError]
```

Rules:

1. `print!` and `eprint!` are bang convenience functions for console text output.
2. `print!` targets the current process stdout text stream; `eprint!` targets the current process stderr text stream.
3. Standard text stream handles do not replace `print!` / `eprint!`; both surfaces remain part of the v0.1 baseline.
4. Each function applies the payload-completing writer convention in
   `conventions.md` to the UTF-8 contents of `s` with the corresponding standard
   `TextWriter.write_str!` semantics and does not append a line terminator.
5. Each call follows the library-owned managed-resource convention in
   `conventions.md`, with the text write as its fixed body and the internal
   standard-stream wrapper release as its close.
6. A sole acquisition, text-write, or wrapper-release failure MUST return
   `Err(Io(_))`, including broken-pipe write failures. If the text write and
   wrapper release both fail, the write error remains the outward `Err` and the
   later release error is not represented separately.
7. A sole console failure MUST NOT be discarded or reported through the fatal
   runtime-error channel. A wrapper-release failure MAY therefore return
   `Err(Io(_))` after the complete payload was made externally visible; it does
   not strengthen the common progress or retry contract.


### File I/O (bang, fallible)

```eglur-catalog
fn open_appender!(path: Path, links: LinkMode) -> Res[Own[ByteWriter], IoError]
fn open_text_appender!(path: Path, links: LinkMode) -> Res[Own[TextWriter], TextError]

fn with_appender![T](path: Path, links: LinkMode, f: callback[scoped-resource](&mut Own[ByteWriter]) -> Res[T, IoError]) -> Res[T, ScopedFailure[IoError]]
fn with_text_appender![T](path: Path, links: LinkMode, f: callback[scoped-resource](&mut Own[TextWriter]) -> Res[T, TextError]) -> Res[T, ScopedFailure[TextError]]

fn read_bytes!(path: Path, links: LinkMode) -> Res[Vec[u8], IoError]
fn write_bytes!(path: Path, xs: Vec[u8], links: LinkMode) -> Res[Unit, IoError]

fn read_text!(path: Path, links: LinkMode) -> Res[Str, TextError]
fn write_text!(path: Path, s: Str, links: LinkMode) -> Res[Unit, TextError]

fn read_lines!(path: Path, links: LinkMode) -> Res[Seq[Str], TextError]
fn write_lines!(path: Path, xs: Seq[Str], links: LinkMode) -> Res[Unit, TextError]

fn list_paths!(path: Path) -> Res[Seq[Path], IoError]
fn list_files!(path: Path) -> Res[Seq[Path], IoError]
fn list_dirs!(path: Path) -> Res[Seq[Path], IoError]
```

### Filesystem Queries and Mutations (bang, fallible)

```eglur-catalog
fn exists!(path: Path) -> Res[Bool, IoError]
fn stat!(path: Path) -> Res[PathStat, IoError]
fn entry_exists!(path: Path) -> Res[Bool, IoError]
fn entry_stat!(path: Path) -> Res[PathStat, IoError]

fn PathStat.kind(self: &Self) -> FileKind
fn PathStat.size_bytes(self: &Self) -> u64

fn create_dir!(path: Path, traversal: DirTraversal) -> Res[Unit, IoError]
fn create_dir_all!(path: Path, traversal: DirTraversal) -> Res[Unit, IoError]

fn remove_entry!(path: Path) -> Res[Unit, IoError]
fn remove_dir!(path: Path) -> Res[Unit, IoError]
fn remove_dir_all!(path: Path) -> Res[Unit, IoError]

fn rename!(
  from: Path,
  to: Path,
  named destination: DestinationPolicy = DestinationPolicy::RejectExisting
) -> Res[Unit, IoError]
fn copy!(
  from: Path,
  to: Path,
  named source_links: LinkMode,
  named destination: DestinationPolicy = DestinationPolicy::RejectExisting
) -> Res[u64, IoError]
```

Destination collision policy:

1. `rename!` and `copy!` share the trailing named-only `destination` parameter:
   1. omitting it selects `DestinationPolicy::RejectExisting`,
   2. replacement requires the source-visible named argument
      `destination=DestinationPolicy::ReplaceCompatible`,
   3. `DestinationPolicy` governs only the final destination entry; it does not
      select source-link traversal or permit cross-filesystem fallback.
2. Destination collision handling is final-entry based:
   1. neither policy follows a symbolic link in the final destination component,
   2. an existing final symbolic link is the destination entry, whether it is
      dangling or resolves to a file or directory,
   3. ordinary resolution of preceding destination components is unchanged.
3. Under `DestinationPolicy::RejectExisting`:
   1. an absent final destination permits the operation to proceed,
   2. any existing final destination entry returns `Err(IoError)` unless an
      operation-specific same-identity rule below declares the call a no-op,
   3. the rejected entry and any symbolic-link target remain unmodified.
4. `DestinationPolicy::RejectExisting` is a race-safe no-replace contract:
   1. the destination absence check and namespace commit MUST be one conditional
      host operation or use an equivalent race-resistant guarantee,
   2. `entry_exists!` or `entry_stat!` preflight followed by an ordinary replacing
      rename, create, or truncate operation does not conform,
   3. an entry that wins a concurrent destination-creation race MUST be rejected
      and MUST NOT be followed or replaced.
5. Under `DestinationPolicy::ReplaceCompatible`:
   1. an absent final destination permits the operation to proceed,
   2. an existing final destination is replaced only when the operation-specific
      kind rules below declare it compatible,
   3. a compatible symbolic-link destination is replaced as a link entry without
      following or modifying its target,
   4. an incompatible entry that wins a concurrent replacement race returns
      `Err(IoError)` and MUST NOT be followed.

Normative filesystem rules:

1. Filesystem-touching APIs are bang and return explicit value-channel errors:
   1. byte and metadata APIs return `Res[..., IoError]`,
   2. whole-file text convenience helpers return `Res[..., TextError]`.
2. Every path-based regular-file acquisition requires one explicit `LinkMode`
   argument:
   1. this requirement applies to byte and text readers, writers, appenders,
      their scoped forms, the whole-file byte, text, and line helpers, and the
      source side of `copy!`,
   2. v0.1 defines no default or ambient `LinkMode`,
   3. `LinkMode::Follow` uses ordinary final-component symbolic-link
      resolution:
      1. a final symbolic-link chain that resolves to a regular file accesses
         that target,
      2. a reader returns `Err` when the chain is dangling,
      3. a create-or-truncate writer or create-or-append appender follows a
         dangling final link and creates the missing target when ordinary path
         resolution and parent-directory requirements permit it,
      4. a link loop, excessive-link failure, permission failure, or other
         resolution failure returns `Err`,
   4. `LinkMode::Reject` MUST NOT follow a symbolic link in the final path
      component:
      1. an existing final symbolic link returns `Err` whether it is dangling
         or resolves to a file, directory, or other kind,
      2. a reader returns `Err` when the final entry is missing,
      3. a create-or-truncate writer or create-or-append appender creates a
         missing final entry directly when ordinary parent-directory and
         permission requirements permit it, but MUST return `Err` rather than
         follow a symbolic link that wins a concurrent replacement race,
   5. under either mode, a final directory or other non-regular-file target
      returns `Err`,
   6. both modes retain ordinary resolution for every preceding path
      component; `LinkMode::Reject` is a final-entry policy, not a rooted path
      containment or sandbox guarantee,
   7. `LinkMode::Reject` MUST be enforced by the file-open operation or an
      equivalent atomic host guarantee; an `entry_stat!` check followed by an
      ordinary target-following open does not conform,
   8. `LinkMode` governs symbolic links only; a hard link is a regular-file
      directory entry and is accessed under either mode,
   9. scoped and whole-file helpers inherit the exact mode and acquisition
      result of their corresponding direct open operation.
3. `exists!` and `stat!` are target-following observations:
   1. both functions follow a symbolic link in the final path component,
   2. `exists!` returns `Ok(true)` when final-component resolution reaches an
      existing target,
   3. `exists!` returns `Ok(false)` when no final entry exists or when final-link
      resolution terminates at a missing target,
   4. `exists!` returns `Err(IoError)` when existence can be neither confirmed
      nor denied, including permission failures and symbolic-link resolution
      errors such as loops,
   5. `stat!` returns `Ok(PathStat)` describing the resolved target when its
      metadata can be obtained,
   6. a successfully resolved final link therefore reports the target's
      `FileKind`, not `FileKind::Symlink`,
   7. `stat!` returns `Err(IoError)` when no final entry exists, a final link is
      dangling, link resolution fails, or target metadata cannot be obtained.
4. `entry_exists!` and `entry_stat!` are final-entry observations:
   1. both functions MUST NOT follow a symbolic link in the final path component,
   2. `entry_exists!` returns `Ok(true)` when the final directory entry exists,
      including when that entry is a dangling symbolic link,
   3. `entry_exists!` returns `Ok(false)` when no final directory entry exists,
   4. `entry_exists!` returns `Err(IoError)` when entry existence can be neither
      confirmed nor denied,
   5. `entry_stat!` returns `Ok(PathStat)` describing the final directory entry,
      including `FileKind::Symlink` for a dangling or non-dangling symbolic link,
   6. `entry_stat!` returns `Err(IoError)` when no final directory entry exists
      or its own metadata cannot be obtained,
   7. no-follow behavior applies only to the final component; ordinary
      resolution of preceding components is unchanged.
5. `PathStat.kind` returns one of `FileKind` variants.
6. `PathStat.size_bytes`:
   1. for regular files, returns byte length,
   2. for non-file kinds, returns `0`,
   3. returns a host-observed `u64` byte count rather than `Nat` because filesystem byte measurements may exceed the v0.1 `Nat` range and this API is not an input boundary.
7. Every call to `create_dir!` or `create_dir_all!` requires one explicit
   `DirTraversal` argument:
   1. v0.1 defines no default or ambient `DirTraversal`,
   2. the policy governs symbolic-link traversal in every existing path
      component that the operation must resolve, including the final component
      when `create_dir_all!` checks whether its target already exists,
   3. unlike final-entry-only `LinkMode`, `DirTraversal` is an all-components
      policy for directory creation,
   4. `DirTraversal` governs symbolic links only; an ordinary directly addressed
      directory remains a directory even when the host may expose other aliases
      to its storage.
8. `create_dir!(path, traversal)` creates exactly one final directory component:
   1. every preceding component MUST already resolve to a directory permitted by
      `traversal`,
   2. the final directory entry MUST be absent; an existing final entry of any
      kind, including any symbolic link, returns `Err(IoError)` under either
      policy,
   3. under `DirTraversal::FollowLinks`, symbolic links in preceding components
      are resolved normally; a dangling link, link loop, non-directory target,
      permission failure, or other resolution failure returns `Err(IoError)`,
   4. under `DirTraversal::RejectLinks`, a symbolic link in any preceding
      component returns `Err(IoError)` whether it is dangling or resolves to a
      directory, file, or other kind,
   5. when the final entry is missing, the operation creates a directory entry
      directly at that location; if another entry wins a concurrent creation
      race, the operation returns `Err(IoError)` and MUST NOT follow it,
   6. an `Err(IoError)` detected before invoking the one final-entry host
      creation operation MUST induce no namespace change,
   7. after that host operation is invoked, an outward `Err(IoError)` permits
      exactly two operation-induced namespace outcomes, disregarding independent
      concurrent mutation: the requested entry remains absent, or the complete
      requested directory entry exists,
   8. a directory that exists after such an `Err(IoError)` MUST satisfy the same
      traversal and race-resistant no-follow rules as a successful creation; the
      operation MUST NOT leave a partial directory entry, staging entry, backup
      entry, compensating deletion, or unrelated namespace change,
   9. `IoError` does not identify which permitted outcome occurred or whether
      the host creation operation was invoked,
   10. the compiler, runtime, and standard library MUST NOT automatically replay
       the call;
       a caller retry is a fresh operation against the current namespace under
       the supplied `traversal`, not a continuation or replay against the first
       call's state. In a quiescent namespace, if the first call completed the
       directory before returning `Err(IoError)`, retry observes an existing
       final entry and returns `Err(IoError)` without certifying the first call's
       outcome.
9. `create_dir_all!(path, traversal)` recursively creates missing directory
   components and is idempotent only under the selected traversal policy:
   1. under `DirTraversal::FollowLinks`, an existing symbolic link in any
      component is resolved normally,
   2. under `DirTraversal::FollowLinks`, an existing directly addressed target
      directory or a final symbolic link that resolves to a directory returns
      `Ok(())`,
   3. under `DirTraversal::FollowLinks`, a component that resolves to a
      non-directory, dangling link, link loop, or other resolution failure
      returns `Err(IoError)`,
   4. under `DirTraversal::RejectLinks`, any existing symbolic link in any
      component, including the final component, returns `Err(IoError)` whether
      it is dangling or resolves to a directory, file, or other kind,
   5. under `DirTraversal::RejectLinks`, existing directly addressed directory
      components are accepted, missing components are created directly, and an
      existing directly addressed target directory returns `Ok(())`,
   6. under either policy, an existing non-directory component returns
      `Err(IoError)`,
   7. missing components are created directly and in traversal order,
   8. an outward `Err(IoError)` may follow zero through all successful component
      creations. Disregarding independent concurrent mutation, the call's
      namespace changes MUST consist only of newly created directories that form
      a prefix of the missing-component sequence reached while resolving `path`
      under `traversal`,
   9. the operation MUST NOT remove or replace a pre-existing entry, alter the
      payload of a pre-existing non-directory entry, roll back or compensate by
      removing a directory that it created, or leave staging entries, backup
      entries, or unrelated namespace changes. Existing directory metadata and
      contents MAY reflect the permitted new child entries,
   10. `IoError` does not report a created-component count, last-created path,
       failure phase, or whether all requested directories were created,
   11. the compiler, runtime, and standard library MUST NOT automatically replay
       the call;
       a caller retry resolves `path` again under `traversal` against the current
       namespace. In a quiescent namespace, retry accepts any directories left
       by the first call and continues from the first still-missing component,
       so it may return `Ok(())`,
   12. concurrent namespace mutation may change what retry observes or acts on;
       retry is not replay against the first call's namespace snapshot,
   13. `create_dir!` and `create_dir_all!` are direct, nontransactional APIs.
       Atomic or transactional directory creation is outside their contract and,
       if introduced, requires a distinct source-visible API.
10. `DirTraversal::RejectLinks` is a race-resistant no-follow contract:
    1. the implementation MUST enforce no-follow behavior while walking and
       creating individual components, or through an equivalent host guarantee,
    2. a separate `entry_stat!` or lexical preflight followed by an ordinary
       target-following creation routine does not conform,
    3. the operation MUST NOT follow a symbolic link that wins a concurrent
       replacement race at any component,
    4. this policy rejects symbolic-link traversal but is not a rooted path
       containment or sandbox guarantee; absolute paths, lexical `..`, mount
       topology, and other host namespace behavior remain outside this contract.
11. `remove_entry!` removes exactly one non-directory final entry:
    1. the operation MUST NOT follow a symbolic link in the final path component,
    2. a directly addressed `FileKind::File`, `FileKind::Symlink`, or
       `FileKind::Other` entry is removed; a file link, directory link, and
       dangling link are all removed as symbolic-link entries without modifying
       their targets,
    3. a hard link has `FileKind::File`, and removing it removes only the named
       directory entry,
    4. a directly addressed `FileKind::Dir` or a missing final entry returns
       `Err(IoError)`,
    5. ordinary resolution of preceding components is unchanged; this final-entry
       rule is not a rooted path containment or sandbox guarantee,
    6. final-entry no-follow behavior MUST be enforced by the removal operation or
       an equivalent atomic host guarantee; an `entry_stat!` preflight followed
       by a target-following removal routine does not conform,
    7. prior `FileKind` observations do not add a compare-and-remove identity
       guarantee: if one non-directory entry is concurrently replaced by another,
       a successful call may remove the replacement entry, but MUST NOT follow it.
12. `remove_dir!` removes exactly one directly addressed empty directory:
    1. the operation MUST NOT follow a symbolic link in the final path component,
    2. a missing final entry, non-empty directory, `FileKind::File`,
       `FileKind::Symlink`, or `FileKind::Other` returns `Err(IoError)`,
    3. a final symbolic link remains a link and its target is not modified,
    4. ordinary resolution of preceding components is unchanged,
    5. final-entry kind enforcement MUST be part of the removal operation or an
       equivalent atomic host guarantee; a final symbolic link that wins a
       concurrent replacement race MUST NOT be followed,
    6. prior `FileKind` observations do not add a compare-and-remove identity
       guarantee: if one empty directory is concurrently replaced by another,
       a successful call may remove the replacement directory, but a replacement
       of any other kind remains subject to the accepted-kind rules above.
13. `remove_entry!` and `remove_dir!` share one strict single-entry failure and
    retry contract:
    1. `Ok(())` denotes the complete removal defined by Rule 11 or Rule 12,
    2. validation, path-resolution, or policy rejection established before
       invoking the final-entry host removal operation MUST induce no namespace
       change,
    3. after that host operation has been invoked, an outward `Err(IoError)`
       permits exactly two operation-induced namespace outcomes:
       1. unchanged: the call contributes no namespace change,
       2. completed: the call has the same complete namespace effect as
          `Ok(())` under the applicable operation-specific rule, including its
          accepted-kind, final-entry no-follow, hard-link, and replacement-race
          requirements,
    4. the completed-after-`Err` alternative exists for hosts where removal may
       commit before its success acknowledgement is lost or replaced by a later
       failure report,
    5. `IoError` does not identify which permitted outcome occurred or whether
       the host removal operation was invoked,
    6. neither permitted `Err` outcome may create, recreate, move, or replace an
       entry; modify an entry payload or symbolic-link target; remove another
       final name; leave a staging or backup entry; or perform compensating
       rollback,
    7. these postconditions describe this call's contribution and do not
       constrain independent concurrent namespace mutation,
    8. the compiler, runtime, and standard library MUST NOT automatically invoke
       a second removal after the source-level call returns `Err`,
    9. a caller-initiated retry is a fresh operation that resolves `path` again
       and applies the selected function's kind and no-follow rules to the current
       final entry; it is not continuation or replay against the first call's
       namespace state,
    10. in a quiescent namespace, if the first call completed the removal before
        returning `Err`, retry observes a missing final entry and returns
        `Err(IoError)` without certifying the first call's outcome,
    11. under concurrent recreation or replacement, retry may remove a different
        current entry when its kind is accepted by the selected function; it
        MUST NOT follow a final symbolic link or bypass the operation-specific
        kind rules,
    12. callers MUST NOT treat `Err` as proof that retry is safe or idempotent
        without an independent identity or application protocol,
    13. these functions are direct, nontransactional APIs. Missing-tolerant,
        identity-conditional, or transactional removal is outside their contract
        and, if introduced, requires a distinct source-visible API.
14. `remove_dir_all!` recursively removes one directly addressed directory tree:
    1. a missing final entry returns `Ok(())`, including on repeated calls,
    2. the final entry MUST be `FileKind::Dir`; a final `FileKind::File`,
       `FileKind::Symlink`, or `FileKind::Other` returns `Err(IoError)`, and a
       final symbolic link and its target remain unmodified,
    3. directly addressed nested directories are traversed recursively, while
       nested `FileKind::File`, `FileKind::Symlink`, and `FileKind::Other`
       entries are removed as entries,
    4. a nested symbolic link is never traversed, whether it is dangling or
       resolves to a file or directory; removing it does not modify its target,
    5. recursive traversal MUST enforce no-follow behavior while opening and
       removing individual descendants, or through an equivalent race-resistant
       host guarantee,
    6. metadata preflight followed by an ordinary target-following recursive
       removal routine does not conform, and a symbolic link that wins a
       concurrent replacement race MUST NOT be followed,
    7. ordinary resolution of path components preceding the final directory is
       unchanged; this recursive no-follow rule is not a rooted path containment
       or sandbox guarantee,
    8. the operation is not an all-or-nothing tree transaction: before returning
       `Err(IoError)`, it may have removed zero or more descendants and may have
       removed the final directory itself,
    9. an `Err(IoError)` result does not identify which entries were removed, and
       the implementation MUST NOT recreate or roll back already removed entries,
    10. partial progress does not weaken the no-follow contract: every removed
        symbolic link is removed only as an entry, no symbolic-link target is
        modified through that link, and no surviving entry is rewritten as
        compensating rollback; surviving ancestor-directory metadata may reflect
        the entries already removed,
    11. retry behavior is current-namespace based rather than snapshot based:
        1. a later call resolves and processes the path again under the same
           final-directory and recursive no-follow rules,
        2. when the namespace is quiescent, a later call may continue removing
           the remaining tree, and a call made after the final entry is gone
           returns `Ok(())`,
        3. with concurrent creation or replacement, a later call may remove
           entries that were not present during the failed call, so callers MUST
           NOT treat retry as replay against the original tree snapshot,
        4. the compiler, runtime, and standard library MUST NOT automatically
           replay the complete `remove_dir_all!` call after it returns `Err`.
15. `rename!` moves one final source entry to the final destination name:
    1. the operation MUST NOT follow a symbolic link in the final source
       component; `FileKind::File`, `FileKind::Dir`, `FileKind::Symlink`, and
       `FileKind::Other` are all valid source entries,
    2. a dangling source symbolic link is moved as a link, and no source-link
       target is modified,
    3. a missing final source entry returns `Err(IoError)`,
    4. if source and destination refer to the same underlying filesystem object,
       the call returns `Ok(())` without changing either name; this rule applies
       before destination collision policy and includes distinct hard links to
       one regular file,
    5. under `DestinationPolicy::ReplaceCompatible`:
       1. a non-directory source may replace a destination
          `FileKind::File`, `FileKind::Symlink`, or `FileKind::Other`,
       2. a directory source may replace one empty directly addressed destination
          directory,
       3. a directory/non-directory kind mismatch or a non-empty destination
          directory returns `Err(IoError)`,
    6. moving a directory to a path within its own descendant tree returns
       `Err(IoError)`,
    7. source and destination MUST belong to one filesystem rename domain;
       cross-filesystem movement returns `Err(IoError)` and MUST NOT fall back to
       copy-and-delete behavior,
    8. ordinary resolution of path components preceding either final entry is
       unchanged; this operation is not a rooted path containment guarantee,
    9. implementations MUST use one host rename namespace operation or an
       equivalent operation that enforces the selected destination policy; this
       forbids user-space copy-and-delete fallback,
    10. validation or policy rejection established before invoking that host
        namespace operation MUST leave both final names and their entries
        unchanged by this call,
    11. `Ok(())` is the only success value and denotes a complete rename;
        `Err(IoError)` carries no partial success value, but value-channel
        failure does not by itself establish namespace rollback,
    12. after the host namespace operation has been invoked, `Err(IoError)`
        permits exactly two operation-induced namespace outcomes:
        1. unchanged: the call contributes no source or destination namespace
           change,
        2. completed: the rename has the same complete namespace effect as
           `Ok(())` under Rules 1-8 and the selected `DestinationPolicy`,
    13. the completed-after-`Err` alternative exists for hosts where a rename
        may commit before its success acknowledgement is lost or replaced by a
        later failure report; `IoError` does not distinguish it from unchanged,
    14. neither permitted `Err` outcome may expose a copy-and-delete fallback,
        partial file contents, an extra namespace entry beyond the complete
        outcome, or compensating rollback; these postconditions describe this
        call's contribution and do not constrain independent concurrent
        namespace mutations,
    15. retry after `Err` is not replay against the failed call's initial
        namespace:
        1. the compiler, runtime, and standard library MUST NOT automatically
           invoke a second `rename!` after returning `Err`,
        2. a caller-initiated retry resolves both names again and applies the
           selected destination policy to their current entries,
        3. if the first call completed, a retry may report a missing source
           rather than confirming that completion,
        4. under concurrent source recreation or destination replacement, a
           retry may act on entries different from those seen by the failed
           call; `ReplaceCompatible` may replace the current compatible
           destination,
        5. callers MUST NOT treat `Err` as proof that retry is safe or
           idempotent without an independent identity or application protocol.
16. `copy!` copies the contents of one source regular file to one directly
    addressed destination file:
    1. every call MUST supply the named argument
       `source_links=LinkMode::Follow` or `source_links=LinkMode::Reject`; there
       is no default or ambient source-link policy,
    2. `source_links` governs only the final source component:
       1. `LinkMode::Follow` resolves a final symbolic-link chain normally and
          copies the target only when resolution reaches a regular file,
       2. under `LinkMode::Follow`, a dangling link, link loop, directory target,
          `FileKind::Other` target, permission failure, or other resolution
          failure returns `Err(IoError)`,
       3. `LinkMode::Reject` accepts a directly addressed `FileKind::File` but
          returns `Err(IoError)` for a missing final entry, `FileKind::Dir`,
          `FileKind::Symlink`, or `FileKind::Other`; a rejected link is not
          followed whether it is dangling or resolves to any target kind,
       4. a hard link has `FileKind::File` and is an accepted source under either
          mode,
       5. ordinary resolution of preceding source components is unchanged,
       6. `LinkMode::Reject` MUST be enforced by the source-open operation or an
          equivalent race-resistant guarantee; an `entry_stat!` preflight
          followed by an ordinary target-following open does not conform, and a
          final link that wins a concurrent replacement race MUST NOT be followed,
    3. `copy!` never recreates a source symbolic-link entry and does not copy
       directory trees; link-entry and tree copying are outside the v0.1
       `copy!` surface, and a directly addressed directory source returns
       `Err(IoError)`,
    4. self-copy is rejected before destination truncation or replacement:
       1. if `from` and `to` identify the same final directory entry before final
          source-link following, the call returns `Err(IoError)`,
       2. after applying `source_links`, an acquired source regular file and an
          existing final destination entry that are the same underlying
          filesystem object also return `Err(IoError)`; this includes distinct
          hard links to one regular file and does not follow a destination link,
       3. either rejection MUST leave the source file and source entry unmodified,
    5. under `DestinationPolicy::ReplaceCompatible`, a final destination
       `FileKind::File`, `FileKind::Symlink`, or `FileKind::Other` is compatible;
       on success the final destination is the resulting regular-file entry,
       while a final `FileKind::Dir` returns `Err(IoError)`,
    6. successful copy returns the copied byte count in `Ok(u64)`,
    7. the copied byte count is the same host-observed `u64` measurement domain as
       `PathStat.size_bytes`; copy failures return `Err(IoError)`,
    8. once final-component resolution and source acceptance succeed, the call
       fixes that acquired underlying regular-file object as its source; a later
       unlink, rename, or replacement of `from` MUST NOT retarget the call to a
       different object,
    9. successful copy MUST be observationally equivalent to reading bytes from
       offset zero in increasing offset order on that same object until the call
       observes its end of file, then writing exactly that observed sequence to
       the destination; an implementation MAY use an optimized host copy primitive
       only when its observable source-consistency behavior conforms to this
       model,
    10. `copy!` does not promise a point-in-time source snapshot and MUST NOT bound
        the transfer to a source size sampled before copying; concurrent overwrite,
        truncation, or growth may change the observed sequence and copied byte
        count, and continued growth may delay or prevent observation of end of
        file,
    11. on `Ok(n)`, `n` is exactly the length of the observed byte sequence and the
        resulting destination regular file contains exactly that sequence; `n`
        need not equal a `PathStat.size_bytes` value observed for the source before
        or after the call,
    12. after source acquisition, an implementation MUST NOT reopen `from` or
        concatenate data observed from a different underlying object into this
        call's transfer,
    13. `copy!` is one nested
        [library-owned managed-resource operation](conventions.md#library-owned-managed-resource-operations):
        1. logical source and destination acquisition each follow the
           [ownership-producing resource-acquisition convention](conventions.md#ownership-producing-resource-acquisition),
           including terminal retirement of provisional resources before an
           acquisition returns `Err`,
        2. source acquisition is the outer managed-resource acquisition; its
           fixed body is one inner managed-resource operation comprising
           destination acceptance and acquisition, byte transfer, and
           destination close,
        3. destination acceptance includes every post-source check that can
           reject the call before destination modification, including collision,
           compatible-kind, and same-underlying-object checks,
        4. if source acquisition succeeds but destination acceptance or
           acquisition fails, the source MUST receive one consuming close before
           the call returns; a later source-close failure does not replace that
           earlier destination error,
        5. after destination acquisition succeeds, either typed transfer result
           MUST be followed by one consuming destination close and then one
           consuming source close; source close MUST still be attempted when
           destination close returns `Err`,
        6. the complete logical phase order is source acquisition, destination
           acceptance/acquisition, transfer, destination close, then source
           close,
        7. the outward `Res[u64, IoError]` contains the first `Err` produced by
           an executed phase in that order; later close errors are not separately
           represented and MUST NOT replace the first error,
        8. `Ok(n)` requires successful completion of every phase, with `n` equal
           to the transfer count defined by Rules 6-11,
        9. both closes follow the
           [terminal consuming-close convention](conventions.md#terminal-consuming-close);
           after either outward result becomes observable, no usable handle or
           reference to either acquired resource may remain and no hidden later
           cleanup may be scheduled,
        10. an optimized host copy mechanism MAY coalesce logical phases or avoid
            distinct host handles only when it preserves the source- and
            destination-identity guarantees, outward result, destination-state
            envelope, phase precedence, and terminal-retirement behavior; this
            contract does not require a particular host call trace,
    14. `copy!` provides direct, non-transactional destination semantics:
        1. it does not promise staging, rollback, or all-or-nothing visibility,
        2. an `Err(IoError)` returned before this call modifies or removes the
           final destination MUST leave that destination unchanged by this call,
        3. implementations MAY use optimized host mechanisms, but their
           observable destination state after return MUST conform to Rules 15-17,
    15. after destination work has begun, `Err(IoError)` permits these
        operation-induced final-destination states:
        1. if the final destination was absent when destination work began under
           either policy, it may remain absent or be a directly addressed regular
           file containing a prefix of the increasing-offset source sequence
           observed by this call,
        2. if a compatible final destination existed under
           `DestinationPolicy::ReplaceCompatible`, the original entry may remain
           unchanged, be absent, or be replaced or modified into such a regular
           prefix file,
        3. the prefix may be empty, partial, or the complete sequence through an
           observed end of file; complete content with `Err` is permitted when a
           later write, close, or success-acknowledgement step fails,
        4. a regular prefix file MUST NOT retain a stale suffix from an earlier
           destination value, contain bytes observed from a different source
           object, or result from following a final destination symbolic link,
        5. the call MUST NOT leave an auxiliary staging or backup entry after it
           returns,
        6. `IoError` carries no copied-byte count or phase discriminator and does
           not identify which permitted state occurred,
        7. these postconditions describe this call's contribution and do not
           constrain independent concurrent destination mutations,
    16. `ReplaceCompatible` does not promise the underlying identity of an
        existing regular-file destination:
        1. an implementation may truncate and write the same object or install a
           different regular-file object at the final name,
        2. another hard link to the old object may therefore observe the direct
           modification or retain the old content; portable programs MUST NOT
           depend on either identity strategy,
    17. retry after `Err(IoError)` is a new copy attempt, not continuation or
        replay of the failed attempt:
        1. the compiler, runtime, and standard library MUST NOT automatically
           invoke `copy!` again after it returns `Err`,
        2. a caller-initiated retry reacquires `from` under `source_links`, starts
           at offset zero, and applies `destination` to the current final entry,
        3. under `RejectExisting`, a regular prefix file left by the failed call
           causes the retry to reject that existing entry, while an absent final
           destination permits a new attempt,
        4. under `ReplaceCompatible`, a retry may replace the current compatible
           entry and restart; it does not resume after a known copied prefix,
        5. source-path or destination-path replacement between attempts may make
           the retry act on different underlying objects,
        6. callers MUST NOT treat `Err` as proof that retry is safe or idempotent
           without an independent identity and recovery protocol.
17. `list_paths!`, `list_files!`, and `list_dirs!` share one deterministic
    directory-enumeration contract:
    1. Each call performs one directory acquisition after ordinary resolution
       of `path`. Successful acquisition fixes one underlying directory object
       for the complete call; the implementation MUST NOT re-resolve `path`,
       reopen by path, or switch to a replacement directory during that call.
    2. Each call enumerates direct children of that acquired directory only and
       MUST NOT recurse. Entries `.` and `..` MUST NOT be included.
    3. The enumeration body ends at the first successful end-of-directory
       observation. Each native basename record emitted before that observation
       contributes one observed candidate.
    4. Exact duplicate native basename code-unit sequences contribute one
       candidate. Implementations MUST NOT merge distinct code-unit sequences
       through Unicode normalization, canonical equivalence, case conversion,
       or host comparison rules.
    5. Every distinct observed candidate basename MUST be converted losslessly
       to one `Str` sequence of Unicode scalar values.
    6. Lossless conversion means that encoding the resulting scalar sequence
       through the implementation's corresponding host path boundary recovers
       the same native basename code-unit sequence.
    7. If any candidate basename has no such representation, the entire call
       MUST return `Err(IoError)` and MUST NOT expose a partial sequence.
    8. Implementations MUST NOT omit an unrepresentable candidate, substitute
       `U+FFFD`, or expose an escaped rendering as if it were literal path text.
    9. After lossless host decoding, implementations MUST preserve the scalar
       spelling exposed by enumeration and MUST NOT apply additional Unicode
       normalization, canonical-equivalence replacement, or case conversion.
    10. For each represented one-segment candidate basename `name`, the result
        path value MUST equal `path.join(&Path.from_str(name))`, where `path` is
        the call argument.
    11. Rule 10 MUST preserve the argument's lexical spelling under the
        `Path.join` rules above; implementations MUST NOT return a basename-only
        `Path` or implicitly normalize or canonicalize the argument.
    12. Complete enumeration, exact-native-name deduplication, and lossless
        conversion occur before any kind filter is applied.
    13. `list_paths!` returns one result path for every represented candidate,
        regardless of whether its own kind is `FileKind::File`,
        `FileKind::Dir`, `FileKind::Symlink`, or `FileKind::Other`. A candidate
        need not remain present after its basename was observed.
    14. For a filtered call, every represented candidate is classified exactly
        once in a logical classification phase after enumeration. Classification
        resolves that basename relative to the directory object fixed by Rule 1,
        not by resolving the joined result path from the filesystem root.
    15. Each classification is a final-entry, no-follow observation equivalent
        to `entry_stat!`: it inspects the then-current entry bound to that
        basename and MUST NOT follow a symbolic-link target.
    16. Enumeration does not establish entry identity for later classification.
        If a basename is replaced before its classification observation, the
        replacement entry's own kind determines the filter result. No result
        claims that the classified entry is the same underlying object whose
        basename record was enumerated.
    17. If a candidate basename is absent at its classification observation, or
        its own kind otherwise cannot be classified, the filtered call MUST
        return `Err(IoError)` without exposing a partial sequence.
    18. `list_files!` returns exactly the represented candidates classified as
        `FileKind::File`; `list_dirs!` returns exactly those classified as
        `FileKind::Dir`. `FileKind::Symlink` and `FileKind::Other` are excluded
        from both filtered results.
    19. Each successful result MUST contain no duplicate `Path` value and MUST
        be sorted by ascending `Path.to_str()` under the
        [canonical lexical text order](../language/invariants.md#canonical-lexical-text-order).
    20. If the acquired directory's direct-child bindings and their own kinds
        remain unchanged throughout enumeration and classification, every
        candidate basename is representable, and no lifecycle phase fails, each
        helper returns exactly the applicable direct-child set. An empty
        directory under those conditions returns `Ok` containing an empty
        sequence for all three helpers.
    21. Concurrent namespace mutation does not establish a snapshot:
        1. a basename added, removed, or renamed during enumeration may be
           observed or omitted according to the acquired directory enumeration,
        2. both the old and new basenames of a rename may be observed, either one
           may be observed, or neither may be observed,
        3. exact duplicate basename records remain collapsed under Rule 4,
        4. `list_paths!` may return a path whose observed basename is absent or
           bound to another entry by the time the call returns,
        5. a filtered result records each accepted basename's kind only at its
           classification observation; later removal, rename, or replacement
           does not revise the result,
        6. classifications occur independently; this contract does not specify
           their observation order, the order need not match enumeration or
           result order, and the observations MUST NOT be presented as atomic,
        7. separate helper invocations perform independent acquisitions and do
           not share a candidate set or classification observations.
    22. Each helper is one
        [library-owned managed-resource operation](conventions.md#library-owned-managed-resource-operations)
        whose fixed body consists of enumeration, conversion, filtering, and
        sorting:
        1. acquisition failure returns `Err(IoError)` without beginning the body,
        2. an enumeration, conversion, or classification failure returns
           `Err(IoError)` without exposing observed or accepted candidates,
        3. after successful acquisition, the internal enumeration resource MUST
           receive one consuming close after either typed body result,
        4. a sole close failure replaces body success with `Err(IoError)`, while
           a body failure remains the outward error when close also fails,
        5. terminal retirement remains required on every typed post-acquisition
           path even when its close failure is not separately represented.
    23. The compiler, runtime, and standard library MUST NOT automatically replay
        a complete helper call after it returns `Err`. A caller-initiated retry
        performs a new acquisition of the then-current `path` and starts a new
        enumeration; it is not continuation against the prior directory object,
        candidate set, or classification observations.
18. Whole-file convenience helpers:
    1. Whole-file read result mapping:
        1. `read_bytes!` opens an existing readable regular-file target and
           returns the byte sequence selected by Rules 2-4 in `Ok(Vec[u8])`,
        2. `read_bytes!` returns `Err(IoError)` on a missing, unreadable, or
           non-file target,
        3. `read_text!` obtains its byte sequence through the same whole-file
           read process in Rules 2-4 and, after the byte read succeeds, decodes
           that complete sequence as UTF-8,
        4. `read_text!` returns `Err(Io(_))` on an acquisition or read failure
           and `Err(InvalidUtf8)` when the successfully read byte sequence is
           not valid UTF-8,
        5. `read_lines!(path, links)` MUST be equivalent to
           `read_text!(path, links)?.lines()`,
    2. Each whole-file read owns one acquisition/read/release lifecycle:
        1. it performs one regular-file acquisition under the supplied `links`
           policy and the ownership-producing acquisition convention in
           `conventions.md`, using the same target-selection rules as the
           corresponding reader opener,
        2. successful acquisition fixes one underlying regular-file object for
           the call and positions its logical read cursor at byte offset zero,
        3. the implementation MUST NOT re-resolve `path`, reopen by path, or
           switch to another underlying object during the read body,
        4. the read body observes bytes at monotonically increasing offsets
           until its first observed end of file or a typed read failure,
        5. after either typed body result, the helper MUST invoke one consuming
           close and apply the library-owned managed-resource precedence in
           `conventions.md`, treating byte reading plus any required UTF-8
           decoding as the fixed body,
    3. On a successful whole-file read:
        1. the byte sequence is exactly the ordered concatenation of bytes
           observed before the first end-of-file result, without omission,
           duplication, or reordering,
        2. the first observed end of file terminates the read body; the helper
           MUST NOT perform a later read to include bytes that become available
           after that result,
        3. a file size obtained before or during the call MAY be used to choose
           buffer capacity but MUST NOT determine or cap the returned sequence,
        4. buffering or another implementation strategy is permitted only when
           its observable result and failures satisfy Rules 2-5,
    4. Concurrent namespace and content mutation have this envelope:
        1. the helpers do not provide a point-in-time snapshot, stable content
           version, mutation-exclusion or locking protocol, or
           implementation-strategy-specific filesystem semantics,
        2. replacement or removal of the selected path after acquisition MUST
           NOT switch the call away from its acquired object,
        3. overwrite, truncation, or growth of that same object MAY affect bytes
           at offsets not yet observed; a successful sequence therefore need
           not equal the object's complete contents at any single instant,
        4. truncation MAY cause an earlier end of file, growth observed before
           the first end of file MAY extend the result, and bytes made available
           only after that end-of-file result are excluded,
        5. continuing growth MAY delay completion indefinitely; v0.1 provides no
           completion-time or input-size bound for these helpers,
        6. concurrent mutation alone MUST NOT cause a synthesized typed failure;
           a real acquisition, read, close, or finite-resource failure is
           reported under the existing error-kind rules,
    5. Whole-file read failure and error precedence:
        1. the result is all-or-error: an `Err` MUST NOT carry or otherwise
           expose a partial byte sequence, decoded text, line sequence, progress
           count, lifecycle phase, or acquired-object identity,
        2. a read failure after internal byte progress remains the body error;
           those bytes are not returned,
        3. a sole close failure replaces an otherwise successful body result,
           while a body failure remains the outward error when close also
           fails; terminal retirement is still required,
        4. `read_text!` MUST finish the byte-read body through its first end of
           file before attempting UTF-8 decoding, so a prior read failure takes
           precedence over invalid bytes already observed,
        5. after a complete byte read, `InvalidUtf8` is the body error and
           remains the outward error if close also fails,
    6. Retry after a whole-file read `Err` is a new whole-file read:
        1. the compiler, runtime, and standard library MUST NOT automatically
           replay the complete helper call,
        2. a caller-initiated retry resolves `path` under `links` again, acquires
           the then-current selected object, and starts at byte offset zero,
        3. a retry is neither continuation from an unread suffix nor evidence of
           snapshot consistency or general idempotence; it MAY observe a
           different object or contents,
    7. Whole-file write result and payload mapping:
        1. `write_bytes!` uses create-or-truncate semantics and the supplied
           `LinkMode` matching `open_writer!`,
        2. `write_bytes!` writes the full payload in `xs` or returns
           `Err(IoError)`,
        3. empty payload writes are valid and follow the same acquisition and
           close contract as non-empty writes; on success they leave the
           acquired target empty and return `Ok(())`,
        4. `write_text!` uses create-or-truncate semantics and the supplied
           `LinkMode` matching `open_text_writer!`, and writes the UTF-8 bytes of
           `s` or returns `Err(Io(_))`,
        5. `write_lines!(path, xs, links)` MUST use the supplied `links` mode and
           be equivalent in observable file contents to writing each input line
           in order followed by `\n`, with failures returned as `Err(Io(_))`,
        6. the logical payload of each whole-file write helper is fixed before
           applying its progress rules:
            1. `write_bytes!` uses the bytes of `xs` in order,
            2. `write_text!` uses the UTF-8 encoding of `s`,
            3. `write_lines!` uses the concatenation, in sequence order, of
               each input line's UTF-8 encoding followed by one byte `0x0A`,
    8. UTF-8 encoding of a `Str` value is total because `Str` contains Unicode
        scalar values; `write_text!` and `write_lines!` MUST NOT return
        `TextError::InvalidUtf8`, and every typed failure from either writer is
        `Err(Io(_))`,
    9. Each whole-file write owns one acquisition/write/release lifecycle:
        1. it performs one create-or-truncate acquisition under `links` and the
           ownership-producing acquisition convention in `conventions.md`,
        2. after successful acquisition, it performs one logical
           payload-completing write and MUST invoke one consuming close after
           either typed result from that write,
        3. its outward result uses the library-owned managed-resource
           precedence from `conventions.md`, treating the logical payload write
           as the fixed body,
        4. an implementation-required flush and the consuming close follow the
           ordered-flush and terminal-close conventions; a flush failure is an
           I/O failure,
        5. `Ok(())` requires complete payload acceptance and successful close,
        6. `Ok(())` does not imply `fsync`-style stable-storage durability or
           survival across process, kernel, device, or power failure,
    10. Whole-file writes provide direct, non-transactional target semantics:
        1. they do not promise staging, rollback, atomic replacement, or
           all-or-nothing visibility,
        2. truncating an existing regular-file target preserves that underlying
           object identity; other hard links to the object may observe the
           truncation and subsequent write progress,
        3. under `LinkMode::Follow`, a final symbolic-link entry remains a link
           and the resolved regular-file target is created or truncated,
        4. under `LinkMode::Reject`, the final component must directly name a
           regular file or be missing; a final symbolic link is rejected
           without modifying its target,
        5. the call MUST NOT leave an auxiliary staging or backup entry after it
           returns,
    11. `Err(IoError)` or `Err(Io(_))` permits only these operation-induced
        target states:
        1. a definite link-policy, kind, or path-resolution rejection before
           creation or truncation leaves the selected path state unchanged,
        2. when the selected target was initially absent and creation was
           permitted, it may remain absent or be a new regular file containing
           an empty, partial, or complete prefix of the logical payload,
        3. when an existing regular-file target was selected, it may retain its
           original contents or remain the same object truncated to an empty,
           partial, or complete payload prefix,
        4. once the call creates or truncates a target, it MUST NOT remove the new
           entry or restore old contents as compensating rollback,
        5. complete payload content with `Err` is permitted when a later flush,
           close, or success-acknowledgement step fails,
        6. a prefix target MUST NOT retain a stale suffix from its old contents
           or contain bytes outside the logical payload,
        7. text and line prefixes are byte-granular and MAY end inside one UTF-8
           scalar encoding or immediately before or after an appended line-feed
           byte,
        8. the error carries no accepted- or visible-byte count and does not
           identify the failing lifecycle phase or permitted target state,
        9. these postconditions describe this call's contribution and do not
           constrain independent concurrent path or content mutations,
    12. Retry after a whole-file write `Err` is a new whole-file write:
        1. the compiler, runtime, and standard library MUST NOT automatically
           replay the complete helper call after returning `Err`,
        2. a caller-initiated retry resolves `path` under `links` again, performs
           create-or-truncate acquisition, and restarts the complete logical
           payload at byte zero rather than resuming after a prefix,
        3. a successful retry against the same quiescent target replaces a prior
           prefix with the complete payload, but path replacement, changed link
           resolution, concurrent observers, and repeated metadata effects mean
           that `Err` alone does not establish general retry idempotence.
19. Append helpers:
   1. `open_appender!` opens a writable byte stream with create-or-append
      semantics under the supplied `LinkMode`,
   2. `open_text_appender!` opens a writable UTF-8 text stream with
      create-or-append semantics under the supplied `LinkMode`,
   3. when target does not exist, append helpers MUST create it,
   4. when target already exists, writes performed through the returned writer MUST be appended after existing contents,
   5. returned writer roots reuse existing `Own[ByteWriter]` / `Own[TextWriter]`
      method semantics for `write_all!`, `write_str!`, `write_line!`, `flush!`,
      and consuming `close!`, including the payload-completing progress and
      retry contract for the three write methods,
   6. appender acquisition follows the ownership-producing acquisition
      convention in `conventions.md`:
      1. a definite link-policy, kind, or path-resolution rejection before
         creation leaves the selected path state unchanged,
      2. after any later acquisition failure, an initially absent selected target
         MAY remain absent or be a new empty regular file,
      3. acquisition itself MUST NOT change the data bytes of an existing
         selected regular file,
      4. the operation MUST NOT remove a newly created entry or leave an
         auxiliary staging or backup entry as compensating rollback,
   7. `with_appender!` and `with_text_appender!` are the corresponding scoped
      forms, inherit the acquisition and terminal-close effects above, and follow
      the callback and error-precedence rules in `conventions.md`,
   8. append helpers do not imply random-access or in-place update capability; seek/update APIs remain deferred non-baseline in v0.1.
