# `std::logic` (REQUIRED)

Scope: Normative for v0.1 unless this file or a section states otherwise.

### Type

`Tri` is the three-valued logic type with variants:

1. `Tri::True`
2. `Tri::False`
3. `Tri::Unknown`


### Core Operations (pure)

```eglur-catalog
fn Tri.not(self: Tri) -> Tri
overload fn Tri.and(self: Tri, rhs: Tri) -> Tri
overload fn Tri.and(self: Tri, rhs: Bool) -> Tri
overload fn Tri.or(self: Tri, rhs: Tri) -> Tri
overload fn Tri.or(self: Tri, rhs: Bool) -> Tri

fn Tri.coalesce(self: Tri, default: Bool) -> Bool
fn Tri.to_opt(self: Tri) -> Opt[Bool]
fn Bool.to_tri(self: Bool) -> Tri
fn Bool.and(self: Bool, rhs: Tri) -> Tri
fn Bool.or(self: Bool, rhs: Tri) -> Tri
```

`Tri` logical operations MUST follow Strong Kleene (K3) truth tables.

`Tri` is a logic-domain type, not a numeric-domain type:

1. `Tri` MUST NOT participate in numeric arithmetic (`+`, `-`, `*`, `/`, `%`).
2. `Tri` MUST NOT be implicitly interpreted as numeric `0/1` in v0.1.
3. Any mixed `Bool`/`Tri` logical operation MUST evaluate by first converting `Bool` to `Tri` (`Bool.to_tri`) and then applying K3 rules.
4. `Tri::Unknown` is not a NaN-style absorbing value; K3 short-information rules apply (for example `Unknown and False = False`, `Unknown or True = True`).
5. The mixed `and`/`or` method overloads above are exactly equivalent to calling `Bool.to_tri()` on each `Bool` operand and then using the corresponding `Tri` operation.
6. v0.1 does not add `Bool.and(Bool)` or `Bool.or(Bool)` methods; ordinary two-valued Boolean composition remains the language-level `&&` / `||` surface.


### K3 Truth Tables (normative)

`not`:

1. `not(Tri::True) = Tri::False`
2. `not(Tri::False) = Tri::True`
3. `not(Tri::Unknown) = Tri::Unknown`

`and`:

1. `Tri::False and x = Tri::False` for any `x`.
2. `Tri::True and Tri::True = Tri::True`.
3. `Tri::True and Tri::Unknown = Tri::Unknown`.
4. `Tri::Unknown and Tri::Unknown = Tri::Unknown`.

`or`:

1. `Tri::True or x = Tri::True` for any `x`.
2. `Tri::False or Tri::False = Tri::False`.
3. `Tri::False or Tri::Unknown = Tri::Unknown`.
4. `Tri::Unknown or Tri::Unknown = Tri::Unknown`.


### Control-Flow Interaction

`Tri` does not implicitly coerce to `Bool`.

The language-level `if` rules for `Tri` are defined in `../language/control-flow.md` and require an explicit `unknown` branch.

`while` remains `Bool`-only in v0.1; `Tri` is not accepted as a loop condition.
