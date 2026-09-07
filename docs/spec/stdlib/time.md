# `std::time` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

Host-observing time operations are effects and MUST be bang.
Pure `Duration` value constructors, parsers, accessors, and arithmetic helpers are non-bang.
`Nat` domain failures occur at the domain scalar conversion or contextual literal boundary; after arguments cross that boundary, `Duration` constructors, parsers, and arithmetic helpers use value-channel errors.

```eglur-catalog
opaque type Instant
opaque type Duration
opaque type TimeError
sum type TimeErrorKind = InvalidSyntax | UnsupportedUnit | Overflow | NegativeResult | InvalidFactor | ReversedInstant

fn now!() -> Instant
fn sleep!(d: Duration) -> Unit
fn TimeError.kind(self: &Self) -> TimeErrorKind
fn Instant.duration_since(self: &Self, earlier: &Self) -> Res[Duration, TimeError]

fn Duration.zero() -> Duration
fn Duration.nanos(n: Nat) -> Res[Duration, TimeError]
fn Duration.millis(n: Nat) -> Res[Duration, TimeError]
fn Duration.seconds(n: Nat) -> Res[Duration, TimeError]
fn Duration.parse(s: Str) -> Res[Duration, TimeError]

fn Duration.as_nanos(self: &Self) -> Nat
fn Duration.as_millis(self: &Self) -> Nat
fn Duration.as_seconds_f64(self: &Self) -> f64

fn Duration.add(self: Duration, other: Duration) -> Res[Duration, TimeError]
fn Duration.sub(self: Duration, other: Duration) -> Res[Duration, TimeError]
fn Duration.mul(self: Duration, k: i64) -> Res[Duration, TimeError]
fn Duration.div(self: Duration, k: i64) -> Res[Duration, TimeError]
fn Duration.scale(self: Duration, k: f64) -> Res[Duration, TimeError]
```

### Error Observation

`TimeErrorKind` implements `Eq`, and two values are equal iff they are the same
variant. `TimeError` does not implement `Eq` in v0.1. Both `TimeErrorKind` and
`TimeError` MUST implement `Display`.

Rules:

1. `TimeError.kind` is pure and total. It observes only the receiver and MUST
   NOT sample a clock or inspect scheduler state.
2. Time failures map exactly as follows:
   1. malformed `Duration.parse` grammar maps to `InvalidSyntax`,
   2. a syntactically formed but unsupported duration unit maps to
      `UnsupportedUnit`,
   3. construction, arithmetic, scaling, or instant-difference overflow maps to
      `Overflow`,
   4. `Duration.sub` whose mathematical result is negative maps to
      `NegativeResult`,
   5. an invalid multiplier, divisor, or floating scale factor maps to
      `InvalidFactor`,
   6. reversed instant order maps to `ReversedInstant`.
3. Kind display is stable and exact:

   | Kind | `Display.to_str` result |
   | --- | --- |
   | `TimeErrorKind::InvalidSyntax` | `"invalid syntax"` |
   | `TimeErrorKind::UnsupportedUnit` | `"unsupported unit"` |
   | `TimeErrorKind::Overflow` | `"overflow"` |
   | `TimeErrorKind::NegativeResult` | `"negative result"` |
   | `TimeErrorKind::InvalidFactor` | `"invalid factor"` |
   | `TimeErrorKind::ReversedInstant` | `"reversed instant"` |

4. `TimeError.to_str()` is exactly
   `concat(["time error: ", kind().to_str()])`.
5. Error and kind display MUST NOT append host clock details, scheduler details,
   raw host errors, or implementation-defined context.

Normative instant rules:

1. `now!` samples the implementation's execution-local monotonic clock and
   returns an opaque snapshot in that clock domain.
2. The monotonic clock MUST NOT move backward within one execution. Consecutive
   samples MAY denote the same instant when no observable clock tick separates
   them.
3. Whether host-system suspension advances the monotonic clock is
   implementation- and target-defined. For a fixed implementation and target,
   `now!`, `Instant.duration_since`, and `sleep!` MUST use one consistent choice,
   and the implementation MUST document that choice.
4. `Instant` has no baseline source constructor, parser, or serialization
   surface and does not implement `Eq`, `Ord`, or `Display` in v0.1.
5. `Instant.duration_since` is pure. It observes only its two snapshot values
   and MUST NOT read the current clock.
6. If `self` does not precede `earlier`, `duration_since` returns the exact
   non-negative elapsed duration represented by the clock snapshots.
7. If `self` precedes `earlier`, or if their difference cannot be represented
   by finite `Duration`, `duration_since` returns `Err(TimeError)`. It MUST NOT
   saturate to zero or a maximum duration.

Normative duration rules:

1. `Duration` represents non-negative finite elapsed time with nanosecond precision.
2. `Duration` implements `Eq`; two values are equal iff they represent the same nanosecond count.
3. `Duration.zero()` returns the additive identity.
4. Constructor inputs (`n`) are `Nat` values:
   1. integer literals in constructor argument position are contextually typed as `Nat`,
   2. signed runtime values MUST be converted with `Nat.from_i64` before use, with failures reported as `DOMAIN_INVALID_VALUE`,
   3. `Duration.nanos` uses the `Nat` input as a nanosecond count,
   4. `Duration.millis` and `Duration.seconds` convert by multiplying the input by `1_000_000` and `1_000_000_000` nanoseconds respectively,
   5. unit conversion overflow, including a `Nat` value that cannot be represented in the finite `Duration` range after scaling, returns `Err(TimeError)`.
5. `Duration.parse` baseline grammar is:
   1. `<int><unit>` where `unit` is one of `ns`, `ms`, `s`,
   2. `<int>` is base-10 digits with optional interior `_`,
   3. no leading sign is allowed,
   4. leading/trailing whitespace is not allowed.
6. `Duration.parse` semantic mapping:
   1. `ns` uses nanoseconds directly,
   2. `ms` converts as `value * 1_000_000` nanoseconds,
   3. `s` converts as `value * 1_000_000_000` nanoseconds.
7. `Duration.parse` failures:
   1. malformed grammar returns `Err(TimeError)`,
   2. unsupported unit returns `Err(TimeError)`,
   3. conversion overflow returns `Err(TimeError)`.
8. Accessor semantics:
   1. `as_nanos` returns the total non-negative nanosecond count as `Nat`,
   2. `as_millis` returns the total whole non-negative millisecond count as `Nat`, truncating any fractional millisecond remainder toward zero.
9. Arithmetic validity:
   1. `add` fails with `Err(TimeError)` on overflow,
   2. `sub` fails with `Err(TimeError)` when result would be negative,
   3. `mul` fails with `Err(TimeError)` when `k < 0` or on overflow,
   4. `div` fails with `Err(TimeError)` when `k <= 0`,
   5. `scale` fails with `Err(TimeError)` when `k` is NaN, infinite, negative, or when the truncated nanosecond result would overflow the finite representable `Duration` range.
10. `div` and `scale` convert to nanosecond result by truncation toward zero.
11. Successful operations MUST be deterministic and pure.

Normative sleep rules:

1. `sleep!` is a scheduler/time effect and retains the signature
   `fn sleep!(d: Duration) -> Unit` in v0.1.
2. `sleep!(Duration.zero())` is a no-op and returns immediately.
3. For positive `d`, a normal return MUST occur only after at least `d` has
   elapsed from function entry in the same monotonic clock domain used by
   `now!` and `Instant.duration_since`.
4. `sleep!` MAY return later than requested because of clock resolution,
   scheduling, or host behavior. v0.1 defines no maximum overshoot.
5. A spurious wakeup or transient host interruption before the minimum elapsed
   duration MUST NOT cause normal return. The implementation MUST continue
   waiting for the remaining duration, using multiple host waits when needed.
6. v0.1 defines no sleep cancellation value channel. Process termination and
   unrecoverable runtime termination are not normal returns and MUST NOT be
   converted into `TimeError` by `sleep!`.
7. `sleep!` guarantees no scheduler fairness or progress for other tasks beyond
   the independent requirements of `std::concurrency`.
8. The exact elapsed duration and overshoot are nondeterministic observations.
   Tensor numeric determinism profiles do not alter the minimum-delay contract
   and MUST NOT require equal elapsed durations across executions.

Duration literal suffixes, richer civil-time types, additional clock domains,
deadline sleep, and cancellation-aware sleep are not part of v0.1. Required
v0.1 conformance MUST NOT require those surfaces. Follow-up design is tracked by
`STDLIB-OI-014` in `../open-items/stdlib.md`.
