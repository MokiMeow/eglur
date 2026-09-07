# `std::net` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

All network I/O is effectful and uses value-channel failures via `Res[..., NetError]`.

```eglur-catalog
opaque type NetAddr
opaque type NetError
sum type NetErrorKind = InvalidAddress | AddressInUse | AddressUnavailable | PermissionDenied | ConnectionRefused | ConnectionReset | ConnectionAborted | NotConnected | Unreachable | BrokenPipe | Interrupted | TimedOut | ResourceExhausted | Unsupported | Other
opaque type TcpListener
opaque type TcpStream

fn NetError.kind(self: &Self) -> NetErrorKind
fn parse_net_addr(s: Str) -> Res[NetAddr, NetError]

fn tcp_bind!(addr: NetAddr) -> Res[Own[TcpListener], NetError]
fn tcp_connect!(addr: NetAddr) -> Res[Own[TcpStream], NetError]

fn with_tcp_listener![T](addr: NetAddr, f: callback[scoped-resource](&mut Own[TcpListener]) -> Res[T, NetError]) -> Res[T, ScopedFailure[NetError]]
fn with_tcp_stream![T](addr: NetAddr, f: callback[scoped-resource](&mut Own[TcpStream]) -> Res[T, NetError]) -> Res[T, ScopedFailure[NetError]]

fn Own[TcpListener].local_addr(self: &Self) -> NetAddr
fn Own[TcpListener].accept!(self: &mut Self) -> Res[Own[TcpStream], NetError]
fn Own[TcpListener].accept_with![T](self: &mut Self, f: callback[scoped-resource](&mut Own[TcpStream]) -> Res[T, NetError]) -> Res[T, ScopedFailure[NetError]]
fn Own[TcpListener].close!(self: Self) -> Res[Unit, NetError]

fn Own[TcpStream].peer_addr(self: &Self) -> NetAddr
fn Own[TcpStream].read_chunk!(self: &mut Self, max_bytes: PositiveNat) -> Res[Opt[Vec[u8]], NetError]
fn Own[TcpStream].write_all!(self: &mut Self, bytes: Vec[u8]) -> Res[Unit, NetError]
fn Own[TcpStream].close!(self: Self) -> Res[Unit, NetError]
```

`TcpListener` and `TcpStream` are resource payload types under
`../language/ownership-boundary.md`. The declarations use the closed
[standard-library catalog metalanguage](catalog-signatures.md); their
`callback[scoped-resource]` parameters follow the effectful, non-escaping
contract in `conventions.md`.

### Error Observation

`NetErrorKind` implements `Eq`, and two values are equal iff they are the same
variant. `NetError` does not implement `Eq` in v0.1. Both `NetErrorKind` and
`NetError` MUST implement `Display`.

Rules:

1. `NetError.kind` is pure and total. It observes only the receiver and MUST NOT
   inspect current network or process state.
2. Malformed or out-of-range `parse_net_addr` input maps to `InvalidAddress`.
3. Network host failures use these portable cause categories:

   | Kind | Cause category |
   | --- | --- |
   | `InvalidAddress` | The textual or resolved address is malformed, out of range, or otherwise invalid for the operation. |
   | `AddressInUse` | The requested local address is already bound. |
   | `AddressUnavailable` | The requested local address is not available on the host. |
   | `PermissionDenied` | Credentials or an access-control policy deny the operation. |
   | `ConnectionRefused` | The remote endpoint refuses the connection. |
   | `ConnectionReset` | The peer or network resets an established connection. |
   | `ConnectionAborted` | The host aborts an in-progress or established connection. |
   | `NotConnected` | The operation requires a connected stream but none exists. |
   | `Unreachable` | The host, network, or relevant network subsystem is unreachable or down. |
   | `BrokenPipe` | A write observes a closed stream endpoint. |
   | `Interrupted` | A host wait or network operation is interrupted and the baseline operation does not complete it transparently. |
   | `TimedOut` | A connection or network I/O timeout expires. |
   | `ResourceExhausted` | A required finite host resource, such as sockets or memory, is exhausted. |
   | `Unsupported` | The operation is unavailable on the implementation target. |
   | `Other` | The cause is implementation-specific, ambiguous, or otherwise has no portable category above. |

4. A condition that matches a named row in Rule 3 MUST use that row's variant.
   Implementations MUST use `Other` only when no named category applies.
5. Implementations MUST NOT expose raw host error numbers, host-localized
   messages, or target-specific error names through `kind()`.
6. Kind display is stable and exact:

   | Kind | `Display.to_str` result |
   | --- | --- |
   | `NetErrorKind::InvalidAddress` | `"invalid address"` |
   | `NetErrorKind::AddressInUse` | `"address in use"` |
   | `NetErrorKind::AddressUnavailable` | `"address unavailable"` |
   | `NetErrorKind::PermissionDenied` | `"permission denied"` |
   | `NetErrorKind::ConnectionRefused` | `"connection refused"` |
   | `NetErrorKind::ConnectionReset` | `"connection reset"` |
   | `NetErrorKind::ConnectionAborted` | `"connection aborted"` |
   | `NetErrorKind::NotConnected` | `"not connected"` |
   | `NetErrorKind::Unreachable` | `"unreachable"` |
   | `NetErrorKind::BrokenPipe` | `"broken pipe"` |
   | `NetErrorKind::Interrupted` | `"interrupted"` |
   | `NetErrorKind::TimedOut` | `"timed out"` |
   | `NetErrorKind::ResourceExhausted` | `"resource exhausted"` |
   | `NetErrorKind::Unsupported` | `"unsupported"` |
   | `NetErrorKind::Other` | `"other"` |

7. `NetError.to_str()` is exactly
   `concat(["network error: ", kind().to_str()])`.
8. Error and kind display MUST NOT append addresses, operations, host messages,
   raw error numbers, or implementation-defined context.
9. A `NetErrorKind` describes cause only. It MUST NOT strengthen any operation's
   connection progress, byte progress, peer observation, retry, ownership, or
   resource-retirement contract.

Normative rules:

1. `parse_net_addr` is pure and deterministic.
2. v0.1 textual address baseline is IPv4 dotted-decimal with decimal port:
   1. accepted form: `<a>.<b>.<c>.<d>:<port>`,
   2. each octet is `0..255`,
   3. port is `0..65535`,
   4. malformed or out-of-range inputs MUST return `Err(NetError)`.
3. `tcp_bind!` opens a TCP listening endpoint.
4. Binding with port `0` requests an implementation-chosen ephemeral local port.
5. `Own[TcpListener].local_addr` returns the bound local address metadata associated with that listener root.
6. `tcp_connect!` opens a TCP stream to the target address.
7. Network operation failures (bind/connect/accept/read/write/close) MUST use value channel (`Res::Err`) and MUST NOT be reported as `runtime_error` in baseline behavior.
8. Successful bind, connect, and accept operations return resource roots that MUST be transferred or closed before their obligation regions exit.
9. `tcp_bind!`, `tcp_connect!`, and `Own[TcpListener].accept!` follow the
   ownership-producing acquisition convention in `conventions.md`:
   1. `tcp_bind!` `Err` returns no listener root but MAY follow a transient bound
      or listening endpoint observable to local processes or peers; the
      provisional local endpoint MUST be terminally retired before return,
   2. `tcp_connect!` `Err` returns no stream root but MAY follow a
      peer-observable connection attempt, a completed handshake, or an
      established connection that was then terminally retired; connection
      acquisition itself MUST NOT send application payload bytes,
   3. `accept!` `Err` leaves the borrowed listener root owned and type-valid but
      MAY have removed one pending connection from the listener queue; any
      provisional accepted endpoint MUST be terminally retired and no accepted
      stream root is returned,
   4. a caller retry is a fresh bind, connect, or accept against current network
      state. In particular, retrying `accept!` does not resume or recover a
      connection removed by the failed call.
10. `Own[TcpListener].accept!` returns only the accepted `Own[TcpStream]` root.
   The accepted peer address is available through `Own[TcpStream].peer_addr`; v0.1 does not place an ownership root inside an accepted-result tuple.
11. `with_tcp_listener!`, `with_tcp_stream!`, and `Own[TcpListener].accept_with!` are scoped forms following `conventions.md`:
   1. when acquisition succeeds, `accept_with!` accepts exactly one connection
      before callback entry; acquisition failure does not enter the callback,
   2. its callback receives a non-escaping writable reference to the accepted stream root,
   3. acquisition/body/close error precedence is the standard scoped resource precedence,
   4. each form inherits the acquisition effects above and the terminal
      consuming-close effects below.
12. `Own[TcpStream].read_chunk!`:
   1. `max_bytes` is a `PositiveNat`; a literal zero is rejected before I/O,
      and signed runtime values MUST be converted with
      `PositiveNat.from_i64` before use, with failures reported as
      `DOMAIN_INVALID_VALUE`,
   2. before EOF, a successful non-EOF read returns `Ok(Some(bytes))` with
      `1 <= bytes.len() <= max_bytes.to_nat()`; it MUST NOT return `Some`
      containing an empty vector,
   3. TCP is a byte stream: one call MAY return only part of one peer write or
      bytes spanning multiple peer writes, subject to `max_bytes`; callers MUST
      NOT infer message boundaries from chunk boundaries,
   4. it returns `Ok(None)` only after peer EOF is observed,
   5. EOF is sticky: after one call returns `Ok(None)`, every later
      `read_chunk!` on that stream MUST also return `Ok(None)`,
   6. logical delivery, `Err(NetError)` progress, retry, and continued ownership
      follow the ordered byte-chunk reader convention in `conventions.md`; in
      particular, bytes received before a later transport failure are delivered
      before that failure is surfaced.
13. `Own[TcpStream].write_all!` follows the payload-completing writer convention
    in `conventions.md`:
   1. `Ok(())` means every provided byte was accepted by the local stream
      implementation,
   2. local acceptance does not establish peer receipt or application
      processing; neither `Ok` nor `Err` reports how many bytes the peer
      observed,
   3. `Err` progress, replay safety, and continued ownership of the stream root
      are exactly the common convention and receive no stronger TCP guarantee.
14. Every network `close!` follows the terminal consuming-close convention in
    `conventions.md`:
   1. listener close terminally retires that local listening endpoint and does
      not close or invalidate stream roots accepted from it,
   2. stream close terminally retires the local endpoint after attempting
      transfer of pending ordered output; `Ok(())` does not establish peer
      receipt, application processing, or a time by which the peer observes EOF,
   3. stream close `Err` MAY follow peer observation of no payload, any payload
      prefix, or the full payload, and the peer MAY observe orderly EOF, reset,
      or no terminal event,
   4. kernel protocol aftermath, port-reuse delay, and peer-observed bytes,
      EOF, or reset are external effects and do not constitute a surviving local
      Eglur resource root.
15. Repeated close, post-close I/O, and post-close metadata access MUST fail statically as ownership use after discharge.
16. `drop_own` on a network resource root MUST fail with `RESOURCE_DROP_FORBIDDEN`.
17. `local_addr` and `peer_addr` are pure while their roots remain owned, return stable metadata, and MUST NOT perform network I/O or fail in baseline behavior.
18. TCP remains byte-oriented in v0.1:
    1. no baseline network operation implicitly decodes or encodes text,
    2. `read_chunk!` boundaries MUST NOT be treated as UTF-8 code-point, line,
       or application-message boundaries,
    3. a stateful text adapter is deferred under `STDLIB-OI-019` and MUST NOT be
       required by baseline conformance.
