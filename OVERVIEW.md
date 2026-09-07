# Eglur Language Overview

Status: Informative (Non-normative)

## What Is Eglur?

Eglur is a compiled language for programs and pipelines with explicit effects,
recoverable failure, visible ownership handoffs, and left-to-right composition.
The core language is not tensor-specific. Its first-class tensor layer supports
data, scientific, and ML code while keeping axis semantics and materialization
costs visible.

## Design Philosophy

### Surface contracts

The main boundaries are visible in source:

- effects by `!`
- failure by `Res/Opt`
- resource ownership and transfer by `Own`/`move`
- tensor transform cost by operation family (`view` vs `rearrange` vs `inflate`)


These choices require more annotations and fewer implicit coercions. Observable
behavior is deterministic under the declared conformance profile; backend
optimizations must preserve that behavior. Compile-time inference is bounded
by the [compile-complexity contract](docs/spec/language/compile-complexity.md).


### Non-goals

Eglur v0.1 is not trying to be:

- a systems-language replacement
- a dynamic metaprogramming-first language
- a backend-specific DSL whose semantics depend on a single compiler/runtime

Low-level control is available only behind explicit `unsafe`, FFI, and
runtime-internal boundaries.


### Left-to-right composition

Pipeline code should read in data-flow order when that does not hide effects or failure.

```eglur
x.then(f).then(g)
```

The core intrinsic rewrites are:

- `x.then(f)` -> `f(x)`
- `(a, b, c).apply(f)` -> `f(a, b, c)`


### Value vs effect is visible in names

Any operation that may perform externally observable effects uses `!`.

```eglur
let v2 = v.sort()   // pure value-producing form
v.sort!()       // in-place mutation (effect)
print!("ok")    // IO effect
```

### Failure is data, not hidden control flow

Recoverable failure is `Res[T, E]` and absence is `Opt[T]`.

```eglur
result {
  let text = read_text!(path, LinkMode::Follow)?
  let n = parse_int(text)?
  yield Ok(n)
}
```

Potentially faulting scalar integer arithmetic uses a lexical `checked` boundary and returns `Res[..., NumericError]`:

```eglur
fn add(a: i64, b: i64) -> Res[i64, NumericError] =
  checked {
    yield a + b
  }
```


### Resource ownership is explicit

External resources are represented by `Own[...]` roots. Transfers use `move`,
and consuming terminals such as `close!` make discharge visible in source.
An ownership root must be transferred or discharged on every typed path;
scoped resource APIs provide the same exact-close guarantee around a callback.


### Batch pipelines have first-class fail-fast tools

`traverse_res` and `sequence_res` give collection pipelines a standard form for "map + stop on first failure" behavior.

```eglur
let rows = lines.traverse_res { parse_row(_) }?
```

Effectful mapping is explicit:

```eglur
let rows = lines.traverse_res! { parse_row_and_log!(_) }?
```


### Tensor cost and semantics are visible in operation families

The first-class tensor layer uses separate operation families to expose cost
and transform semantics:

- `view{...}`: zero-copy affine view only
- `rearrange{...}`: structural remap with materialization
- `inflate{...}`: explicit duplication
- `reduce{...}`: axis elimination (default `sum`, custom reducers via `.reduce_by(...)`)
- `contract{...}`: fixed einsum-style multiply+sum
- `einop{...}`: composite transform with fixed multiply+sum elimination
- `grad(...)` / `value_and_grad(...)`: pure AD transforms over the closed v0.1
  AD-support subset of Core Tensor IR
- `map_axis(...)`: pure left-to-right axis map followed by stack, with
  deterministic fail-fast semantics

The distinction remains visible in code:

```eglur fixture=XFM-public-overview-tensor-cost-example
import std::tensor::{TransformError, ones}

fn main!() -> Res[Str, TransformError] =
  result {
    let x = ones[f32](2,12,3)
    let y = view{ b (h*w) c -> b h w c }.with_sizes(["h": 3, "w": 4])(x)?
    let z = rearrange{ b h w c -> b (h*w) c }(y)?
    let duplicated = inflate{ b hw c -> b hw (c*2) }(z)?
    yield Ok(to_str(duplicated.shape()))
  }
```


### Tensor axes are executable metadata

Axis names are executable tensor metadata, not comments attached to a shape.
They can be checked, propagated, and used by operations that select axes.

```eglur
let t = ones[f32](2, 3).with_names([#0:'batch, #1:'task])?
let r = sum(t, 'task)?
```

`AxisIndex` and `AxisName` are distinct types. A tensor's names are stored as
`NameOverlay = Map[AxisIndex, AxisName]`; each operation defines how it propagates
them.


## Quick Tour

These examples show how the language features compose. They are fragments
unless they include a complete entry point; names such as `path` and `batch`
come from the surrounding program.

### Pure + effect composition

```eglur
import std::io::{LinkMode, read_lines!, print!}
import std::parse::parse_int

result {
  let lines = read_lines!(path, LinkMode::Follow)?
  let nums = lines.traverse_res { parse_int(_) }?
  let pos = nums.filter { _ > 0 }
  pos.tap! { |x| print!(to_str(*x)) }
  yield Ok(pos.len())
}
```


### Modules, imports, and visibility

```eglur
// src/app/rules.eglur
module app::rules

pub fn grade(x: f64) -> Str =
  if x >= 0.9 { yield "A" } else { yield "B" }
```

```eglur
// src/app/score.eglur
module app::score

import std::io::print! as log!
import std::parse::{parse_f64, ParseError}
import super::rules::grade

fn run!(raw: Str) -> Res[Str, ParseError] =
  result {
    let x = parse_f64(raw)?
    let g = grade(x)
    log!(g)
    yield Ok(g)
  }
```


### Control flow (`if`, `else if`, `match`, loops)

```eglur
import std::logic::Tri

fn tri_label(t: Tri) -> Str =
  if t {
    yield "ready"
  } else {
    yield "blocked"
  } unknown {
    yield "unknown"
  }

fn level(score: i64) -> Str =
  if score >= 90 {
    yield "gold"
  } else if score >= 80 {
    yield "silver"
  } else {
    yield "bronze"
  }

fn explain(r: Res[i64, Str]) -> Str =
  match r {
    Ok(v) if v > 0 => "positive",
    Ok(_) => "non-positive",
    Err(e) => e,
  }

fn sum_to(n: i64) -> Res[i64, NumericError] =
  checked {
    let mut acc: i64 = 0
    for i in 1 ..= n {
      acc = acc + i
    }
    yield acc
  }

```


### Numeric division

```eglur
fn divisions() -> (i64, f64) =
  (7 / 2, 7 /. 2)
```


### Explicit resource ownership

```eglur
import std::io::{LinkMode, Path, TextError, TextReader, open_text_reader!}

fn check_access!(path: Path) -> Res[Unit, TextError] =
  result {
    let reader: Own[TextReader] = open_text_reader!(path, LinkMode::Reject)?
    yield (move reader).close!()
  }
```


### Tensor transform surface

```eglur fixture=XFM-public-overview-tensor-surface-example
import std::tensor::{AxisError, ContractError, DTypeError, DeterminismError, IndexError, NumericError, ShapeError, Tensor, TransformError, ones}
import std::tensor::index::{get_at, set_at!, IndexItem, ScatterMode}

fn main!() -> Res[Tensor[f32], Union[AxisError, ContractError, DTypeError, DeterminismError, IndexError, NumericError, ShapeError, TransformError]] =
  result {
    let x = ones[f32](2,12,3)
    let y = x.then(view{ b (h*w) c -> b h w c }.with_sizes(["h": 3, "w": 4]))?
    let z = y.then(rearrange{ b h w c -> b (h*w) c })?
    let u = reduce{ b h w c -> b c }(y)?

    let left = ones[f32](2,3)
    let weights = ones[f32](3,4)
    let mm = contract{ i k, k j -> i j }
    let mut r = (left, weights).apply(mm)?

    let picked = get_at(r, [IndexItem::At(0), IndexItem::All].to_vec())?
    set_at!(&mut r, [IndexItem::At(1), IndexItem::All].to_vec(), picked, mode=ScatterMode::Assign)?
    yield Ok(r)
  }
```

`view/rearrange/inflate/reduce/contract/einop` and `index` belong to the
standard tensor surface.


### Program transforms (`map_axis`, `grad`)

```eglur
let vf = map_axis(
  normalize,
  in_axes=[axis(0), None].to_vec(),
  out_axis=axis(0),
)
let g = grad(loss_fn)
let vg = value_and_grad(loss_fn)

result {
  let h = vf(batch, bias)?
  let dx = g(h)?
  let both = vg(h)?
  yield Ok((dx, both))
}
```


## How To Read The Spec Set

This overview is informative. The following documents define normative behavior:

- [Language specification](docs/spec/SPEC.md)
- [Standard library](docs/spec/STDLIB.md)
- [Tensor axis semantics](docs/spec/axes.md)
- [Diagnostics](docs/spec/ERRORS.md)
- [Conformance rules](docs/spec/CONFORMANCE.md)
- [Spec-package release and implementation-conformance policy](docs/spec/RELEASE-GATE-v0.1.md)

Tensor conformance suites are split by operation category in the [suite
manifest](tests/conformance/manifest.yaml).

The repository [README](README.md) routes conformance authors, tooling
maintainers, release operators, and design maintainers to their working entry
points.
