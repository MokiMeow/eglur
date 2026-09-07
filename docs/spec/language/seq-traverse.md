# Sequence, Traverse, and Sequence Conversion

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Required Operations

Core sequence processing MUST define:

1. Pure: `traverse_res`, `sequence_res`, `traverse_opt`, `sequence_opt`.
2. Effectful: `traverse_res!`, `traverse_opt!`.
3. Fold/scan pure family: `fold`, `fold_res`, `scan`, `scan0`.
4. Fold/scan bang family: `fold!`, `fold_res!`, `scan!`, `scan0!`.
5. Fallible `_res` pure family: `find_res`, `find_map_res`, `filter_map_res`, `partition_res`.
6. Fallible `_res` bang family: `find_res!`, `find_map_res!`, `filter_map_res!`, `partition_res!`.

Precise API signatures are defined in `../STDLIB.md`.


### Traverse Evaluation

This section defines sequential `Seq` traversal and fold/scan families. Parallel
batch operations from `std::concurrency` (`par_map!`, `par_traverse_res!`, and
`par_traverse_opt!`) are separate `par_*` APIs; their callback interleaving,
earliest-index result rules, and fail-fast boundary are specified in
`../stdlib/concurrency.md`.

All `traverse_*` operations (pure and bang forms) MUST evaluate elements from left to right.

`traverse_res` MUST fail fast at the first `Err` (`TRAVERSE_RES_FAIL`).

`traverse_opt` MUST fail fast at the first `None`.

`traverse_res!` MUST fail fast at the first `Err` (`TRAVERSE_RES_FAIL`).

`traverse_opt!` MUST fail fast at the first `None`.

For `xs = [x_0, ..., x_{n-1}]`:

\[
traverse\_res(xs, f) =
\begin{cases}
Err(e) & \text{if } \exists i \text{ minimal s.t. } f(x_i)=Err(e) \\
Ok([y_0,\dots,y_{n-1}]) & \text{if } \forall i,\ f(x_i)=Ok(y_i)
\end{cases}
\]


### Diagnostics

On traversal `Res` failure, diagnostics MUST include the 0-based failing element index.

On traversal `Opt` `None` short-circuit, diagnostics are optional in baseline v0.1. If emitted, they MUST include the same 0-based failing index contract (`TRAVERSE_OPT_NONE`).

Diagnostic message shape and code assignments are defined in `../ERRORS.md`.

For `fold_res` and `fold_res!`, diagnostics on first `Err` MUST include failing 0-based element index (`FOLD_RES_FAIL`).

For Seq `_res` combinators (`find_res*`, `find_map_res*`, `filter_map_res*`, `partition_res*`), diagnostics on first `Err` MUST include failing 0-based element index (`TRAVERSE_RES_FAIL`).


### Callback Effect Rules

The rules in this section apply to the sequential operations listed under
Required Operations. They do not constrain callback start/completion/effect
interleaving for `std::concurrency` `par_*` APIs.

`traverse_res` and `traverse_opt` are non-bang APIs and MUST reject effectful callbacks (`PURE_COMBINATOR_EFFECTFUL_CALLBACK`).

`traverse_res!` and `traverse_opt!` are bang APIs and MAY accept effectful callbacks.

`fold`, `fold_res`, `scan`, and `scan0` are non-bang APIs and MUST reject effectful callbacks (`PURE_COMBINATOR_EFFECTFUL_CALLBACK`).

`fold!`, `fold_res!`, `scan!`, and `scan0!` are bang APIs and MAY accept effectful callbacks.

`find_res`, `find_map_res`, `filter_map_res`, and `partition_res` are non-bang APIs and MUST reject effectful callbacks (`PURE_COMBINATOR_EFFECTFUL_CALLBACK`).

`find_res!`, `find_map_res!`, `filter_map_res!`, and `partition_res!` are bang APIs and MAY accept effectful callbacks.

For these sequential bang traversal variants, conforming implementations MUST preserve left-to-right effect order and MUST NOT execute callback effects after the first short-circuit point.

### Fold and Scan Evaluation

`fold`, `fold_res`, `scan`, `scan0`, `fold!`, `fold_res!`, `scan!`, and `scan0!` MUST evaluate callbacks left-to-right over encounter order.

`fold_res` and `fold_res!` MUST short-circuit at first `Err` and MUST NOT evaluate later elements.

Seq `_res` combinators MUST evaluate left-to-right and short-circuit at first `Err` with no callback evaluation for later indices.

`scan`/`scan!` exclude initial seed from produced sequence; `scan0`/`scan0!` include it as first element.

For `scan!` and `scan0!`, callback effects MUST be exactly-once per input element and MUST NOT be replayed by iterating produced sequences.
