# Module Overview

Scope: Normative for v0.1 unless this file or a section states otherwise.

All catalog paths below use the implementation-owned `std` module root. Its
namespace authority, user-source reservation, and resolution precedence are
defined only by the [language module-resolution contract](../language/modules-resolution.md#reserved-standard-library-root).

### REQUIRED modules

1. `std::prelude`.
2. `std::str`.
3. `std::logic`.
4. `std::opt`.
5. `std::res`.
6. `std::seq`.
7. `std::vec`.
8. `std::map`.
9. `std::set`.
10. `std::parse`.
11. `std::json`.
12. `std::io`.
13. `std::net`.
14. `std::time`.
15. `std::tensor`.
16. `std::concurrency`.


### OPTIONAL modules

1. `std::csv` (normative if provided; excluded from required v0.1 conformance).

Catalog rule:

1. `Module Overview` lists only modules with normative contracts in this specification tree.
2. Future candidate modules belong in the [standard-library open-item source](../open-items/stdlib.md) and the [roadmap](../../roadmap/README.md), not in the baseline/optional catalog.
3. `std::ord_map` is not a catalog module in v0.1; it remains an optional extension note under `std::map`.


### Conformance harness module

`std::test` is OPTIONAL for production distributions and REQUIRED for conformance execution harnesses.
