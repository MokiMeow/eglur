# Package and Dependency Model

Scope: Normative for v0.1 unless this file or a section states otherwise.

This section defines package-level compilation boundaries used by module resolution and coherence rules.

## Package Root

An Eglur package build target is a directory containing:

1. `Eglur.toml` package manifest at package root.
2. `src/` directory as the only package source root.
3. `.eglur` as the canonical baseline source file extension.

Package mode activation in v0.1 is deterministic:

1. If package root contains top-level `Eglur.toml`, implementation MUST activate package mode by default.
2. Implementations MAY expose an explicit file-set override mode for tooling workflows.
3. Without such explicit override, a present top-level `Eglur.toml` MUST select package mode.

Module canonical path mapping for package mode is defined by `modules-resolution.md` using `src/`-relative paths.

Package root validation failures MUST use stable compile-channel codes:

1. Missing `Eglur.toml` in an explicitly requested package-mode build MUST fail with `PACKAGE_MANIFEST_MISSING`.
2. Missing `src/` root in package mode MUST fail with `PACKAGE_SRC_ROOT_MISSING`.
3. Both diagnostics MUST report `package_role = root` for the build root or
   `package_role = dependency` for a selected dependency root.
4. The dependency form MUST additionally report `dependency` and `source_kind`.
   A path dependency MUST report its exact declared `path`; a registry
   dependency MUST instead report its canonical `registry_package`.

An executable package invocation MUST designate exactly one mapped `.eglur`
source under the validated `src/` root as its entry source. That source selects
the program `main!` under `program-model.md`; package mode does not infer a
fixed filename such as `src/main.eglur`. Package root, manifest, dependency,
lockfile, and module validation precede program-entry validation.

## Manifest Shape

`Eglur.toml` baseline fields:

```toml
[package]
name = "my_pkg"
version = "0.1.0"

[registries]
corp = "packages.example.com"

[dependencies]
dep_a = { package = "packages.example.com/acme/dep-a", version = "1.2.3" }
dep_b = { registry = "corp", package = "acme/dep-b", version = "2.0.0" }
dep_c = { path = "../dep_c" }
```

Rules:

1. `[package]` table is required.
2. `package.name` is required.
3. A `package.name` value is a `PackageName` only when it is a TOML string whose
   complete decoded text satisfies `[A-Za-z0-9][A-Za-z0-9_-]*`.
4. `PackageName` comparison is case-sensitive scalar-for-scalar equality.
   Implementations MUST NOT case-fold, normalize Unicode, or otherwise rewrite
   a package name. Empty strings, non-ASCII text, whitespace, separators, and
   punctuation other than `-` and `_` are invalid.
5. An invalid present `package.name` MUST fail with
   `PACKAGE_MANIFEST_NAME_INVALID`. Diagnostic `reason` is `non_string`, `empty`,
   or `invalid_character`; the latter two forms MUST also report the exact
   decoded `name`.
6. `package.version` is required.
7. `[registries]` and `[dependencies]` are optional (empty maps allowed).
8. `[registries]` is a flat map from `RegistryAlias` keys to
   `RegistryAuthority` string values. Inline records and
   `[registries.<alias>]` subtables are not v0.1 forms.
9. Missing `[package]` table MUST fail with
   `PACKAGE_MANIFEST_PACKAGE_TABLE_MISSING`.
10. Missing `package.name` MUST fail with `PACKAGE_MANIFEST_NAME_MISSING`.
11. Missing `package.version` MUST fail with `PACKAGE_MANIFEST_VERSION_MISSING`.
12. `package.version` MUST be an exact semantic version literal. Values that are
    not exact semantic version literals MUST fail with
    `PACKAGE_MANIFEST_VERSION_INVALID`.
13. If multiple package-metadata failures exist, Rule 9 takes precedence, then
    Rule 10, then Rule 5, then Rule 11, then Rule 12.

## Registry Locators

Registry source spelling is normalized once at manifest ingress into a
`RegistryPackageId`. It is never inferred from a dependency key or package
name.

### Registry Authority and Alias

1. `RegistryAuthority` is lowercase ASCII DNS-style text containing at least
   two labels separated by `.`.
2. Each authority label is 1-63 characters and satisfies
   `[a-z0-9](?:[a-z0-9-]*[a-z0-9])?`.
3. A `RegistryAlias` satisfies
   `[a-z](?:[a-z0-9_-]*[a-z0-9])?`.
4. Alias comparison uses exact decoded scalar text. Implementations MUST NOT
   case-fold, Unicode-normalize, or otherwise rewrite an alias.
5. Empty, uppercase, non-ASCII, leading/trailing `-` or `_`, whitespace, and
   other punctuation are invalid aliases.
6. A `[registries]` value is a `RegistryAuthority`, not a URL or transport
   endpoint. Endpoint, credential, mirror, proxy, and retry configuration is
   outside `Eglur.toml`.
7. Invalid `[registries]` shape, alias keys, value types, or authority values
   MUST fail with `PACKAGE_REGISTRY_ALIAS_INVALID`.

### Registry Package ID

A `RegistryPackageId` has this shape:

```text
RegistryAuthority "/" RegistryPathSegment ("/" RegistryPathSegment)*
```

Each `RegistryPathSegment` satisfies
`[a-z0-9](?:[a-z0-9_-]*[a-z0-9])?`.

Rules:

1. The complete ID is lowercase ASCII and is already canonical.
2. A scheme, user information, port, query, fragment, percent encoding,
   backslash, empty segment, `.` segment, `..` segment, uppercase character, or
   non-ASCII character is invalid. Implementations reject rather than normalize
   those forms.
3. Punycode authorities are representable through their canonical lowercase
   ASCII form; no Unicode-to-punycode conversion occurs at manifest ingress.
4. A qualified registry dependency supplies a complete `RegistryPackageId` in
   `package` and omits `registry`.
5. An alias registry dependency supplies one `RegistryAlias` in `registry` and
   one or more registry path segments in `package`. The alias MUST exist in the
   same manifest and expands exactly once by prefixing its authority.
6. A qualified package with `registry`, an unqualified package without
   `registry`, an unknown alias, an invalid field type, or malformed package
   text MUST fail with `PACKAGE_REGISTRY_SELECTOR_INVALID`.
7. Equality, graph construction, lock comparison, and registry acquisition use
   only the expanded `RegistryPackageId`. Alias spellings never enter package
   identity or `Eglur.lock`.
8. Changing an alias used by a dependency can make its expanded selector
   disagree with the lock. An unused alias is semantically inert.

## Dependency Import-Root Keys

A dependency key is the decoded immediate TOML key directly under
`[dependencies]`. It is the source-visible import root for that direct
dependency; it is not the dependency package's `PackageName` or graph identity.

Rules:

1. `Eglur.toml` and `Eglur.lock` MUST use the same dependency-key domain.
2. A decoded dependency key is valid only when its complete text satisfies the
   [source-addressable canonical module-path](modules-resolution.md#source-addressable-canonical-module-paths)
   domain for the first segment of an absolute `ImportPath` and is not the
   reserved `std` root.
3. Consequently, a dependency key:
   1. MUST satisfy the ASCII-only base identifier grammar
      `[A-Za-z_][A-Za-z0-9_]*`,
   2. MUST NOT be the lone `_` token,
   3. MUST NOT be a reserved source keyword,
   4. MUST NOT be either import-context root `self` or `super`, and
   5. MUST NOT be `std`, which is reserved for the standard-library module root.
4. TOML key syntax does not create a second identity. Bare, basic-quoted, and
   literal-quoted key spellings that decode to the same valid text denote the
   same dependency key. For example, `dep_math`, `"dep_math"`, and `'dep_math'`
   each decode to `dep_math` when used separately.
5. Implementations MUST validate after TOML key decoding and MUST NOT case-fold,
   replace punctuation, apply Unicode normalization, or otherwise mangle a key
   into a source identifier.
6. An invalid decoded dependency key in either artifact MUST fail with
   `PACKAGE_DEP_KEY_INVALID` during resolve-phase package validation.
7. When one artifact contains multiple invalid dependency keys, the key first
   under the [canonical lexical text order](invariants.md#canonical-lexical-text-order)
   MUST determine the primary diagnostic, independent of TOML declaration order.
8. Manifest and lockfile key-set equality is evaluated on decoded valid keys.
   Different TOML spellings that decode to the same valid key therefore match.
9. This key grammar does not constrain `PackageName`. For example, the package
   named `linear-algebra` may be bound locally by dependency key `linalg`.

## Dependency Sources and Versions

Dependency version requirements are exact-only in v0.1:

1. A dependency entry has exactly one of these closed forms:
   1. path: `{ path = "<relative-path>" }`,
   2. qualified registry:
      `{ package = "<RegistryPackageId>", version = "<x.y.z>" }`, or
   3. alias registry:
      `{ registry = "<RegistryAlias>", package = "<registry-path>", version = "<x.y.z>" }`.
2. A non-table entry, unsupported field, simultaneous path and registry-source
   indication, or absent source indication MUST fail with
   `PACKAGE_DEP_SOURCE_CONFLICT`.
3. Any of `package`, `registry`, or `version` establishes registry-source intent
   when `path` is absent. Missing or malformed registry fields then use their
   narrower registry/version diagnostic instead of source conflict.
4. A registry dependency requires an exact semantic version literal. A missing,
   non-string, or malformed exact version MUST fail with
   `PACKAGE_DEP_VERSION_INVALID`.
5. A syntactically recognized range form such as `^1.2`, `~1.2`, or `>=1.0`
   MUST fail with `PACKAGE_VERSION_RANGE_UNSUPPORTED`.
6. Path dependencies use `path = "<relative-path>"` under the path-resolution
   contract below and do not carry a version selector.

v0.1 has no manifest-level public-dependency field. A package that exposes a
dependency-owned type or trait in a public contract uses the contract-module-local
`pub import` bridge defined in `modules-resolution.md`; adding fields such as
`public = true` is an unsupported dependency-entry field under Rule 2.

For v0.1 package validation, an exact semantic version literal is the canonical
SemVer core form `major.minor.patch`. Each component is exactly `0` or an ASCII
digit from `1` through `9` followed by zero or more ASCII decimal digits.
Leading zeroes, omitted components, pre-release identifiers, and build metadata
are invalid. Implementations reject rather than normalize noncanonical text.

Manifest validation normalizes only after selecting a primary failure:

1. validate package metadata,
2. validate the complete optional `[registries]` table,
3. validate decoded dependency keys,
4. inspect dependency entries in canonical dependency-key order, and
5. within one entry, validate closed shape/source cardinality, then source
   locator, then exact version.

TOML declaration order MUST NOT affect the selected primary diagnostic.

Internal resolver algorithm details are implementation-defined, but resolver observables and failure boundaries in this document are normative.

## Package Identity

The resolver keeps source spelling, human-readable naming, and graph identity
as separate values:

1. `DependencyKey` is the local import binding declared by one package.
2. `PackageName` is the validated human-readable manifest value above.
3. `RegistryPackageId` is the canonical registry locator produced at manifest
   ingress.
4. `RegistryTreeIntegrity` identifies one canonical registry package tree and
   has the spelling `tree-sha256:` followed by 64 lowercase hexadecimal digits.
5. `RegistrySourceCoordinate` is the tuple of `RegistryPackageId`, exact
   version, and `RegistryTreeIntegrity`.
6. `VerifiedTreeSnapshot` is an immutable canonical package tree bound to one
   complete `RegistrySourceCoordinate` after acquisition, tree-integrity
   comparison, and target package validation have all succeeded.
7. `ResolvedPackageRoot` is the host-local key used to intern path-source
   package nodes after complete symbolic-link resolution.
8. `LockedNodeId` is a portable lock-local graph reference with no identity
   outside one `Eglur.lock`.
9. `PackageInstanceId` is an opaque identity assigned to one resolved package
   node for one dependency-graph construction.

Rules:

1. `PackageInstanceId`, not `PackageName`, owns graph traversal state, cycle
   detection, module and declaration ownership, nominal type identity, and trait
   locality/coherence comparisons.
2. A package-mode module identity is `(PackageInstanceId, ModulePath)`. A
   declaration identity contains that package-scoped module identity in addition
   to its ordinary declaration key.
3. `PackageInstanceId` is graph-local and MUST NOT be serialized or compared as
   a stable cross-build package identifier.
4. `ResolvedPackageRoot` and `PackageInstanceId` are neither source syntax nor
   persistent identity. They MUST NOT be written to `Eglur.lock` or exposed in
   portable diagnostics.
5. The root package's `ResolvedPackageRoot` MUST intern the initial
   `PackageInstanceId` before dependency traversal. Path selectors that reach
   that root or another already interned root, including through different
   dot-segment spellings or symbolic-link aliases, MUST reuse its
   `PackageInstanceId`. Distinct resolved package roots MUST intern distinct
   package instances.
6. Registry references with equal complete `RegistrySourceCoordinate` values
   MUST reuse one `PackageInstanceId`. Registry and path sources MUST NOT
   implicitly converge.
7. One v0.1 graph selects at most one exact version for each
   `RegistryPackageId`. A later reference to the same registry ID at another
   exact version MUST fail with `PACKAGE_VERSION_CONFLICT` before package-name
   conflict checking.
8. Two lock nodes with one registry ID and exact version but different tree
   integrity values make the lock internally contradictory and MUST fail with
   `PACKAGE_LOCK_INVALID` before acquisition.
9. Within one declaring package, the direct dependency map from `DependencyKey`
   to `PackageInstanceId` MUST be injective. If a later dependency key under the
   canonical traversal order reaches an instance already bound by an earlier
   key, compilation MUST fail with `PACKAGE_DEP_DUPLICATE` and report:
   1. `package`: the declaring `PackageName`,
   2. `dependency`: the later key, and
   3. `existing_dependency`: the earlier key.
10. Different declaring packages MAY use different dependency keys for one
   shared package instance. Transitive graph convergence on one instance is
   valid; the resolver visits that instance once after its first complete visit.
11. v0.1 additionally requires the resolved graph to map each `PackageName` to
   exactly one `PackageInstanceId`, including the root package. Two distinct
   instances with one name MUST fail with `PACKAGE_NAME_CONFLICT`.
12. `PACKAGE_NAME_CONFLICT` diagnostic data MUST contain `package`,
   `existing_dependency`, `dependency`, `existing_version`, `version`,
   `existing_source_kind`, `source_kind`, and `reason`. The root package is
   represented by dependency label `<root>` and source kind `root`; dependency
   instances use their selected key and source kind `path` or `registry`.
13. `reason` is `source` when the exact versions are equal and
    `version_and_source` when they differ. A version-only package-name conflict
    is unreachable because two distinct package instances necessarily have
    distinct source identities.
14. A conflict diagnostic MUST NOT disclose `ResolvedPackageRoot` or
    `PackageInstanceId`. Dependency labels, versions, source kinds, and the
    conflicting `PackageName` provide its portable evidence.

The graph-wide name restriction is a v0.1 coexistence policy, not an equation
between `PackageName` and package identity.

## Registry Acquisition Boundary

Registry acquisition is a transactional provider boundary downstream of lock
selection and upstream of package-tree validation.

### Provider Request and Outcome

1. `RegistryProvider` receives exactly the canonical `RegistryPackageId` and
   exact version selected by the structurally validated lock. It MUST NOT select,
   rewrite, normalize, or substitute package identity.
2. The resolver supplies a bounded private sink. A provider adapter MAY emit
   logical entries into that sink while fetching or decoding, but neither the
   partial entries nor their storage are visible as a package root, cache entry,
   or sealed post-acquisition source for tree, package, or source compilation.
   The provider attempt itself remains part of the acquisition invocation.
3. Unless the sink has already terminated, the provider adapter ends its attempt
   with exactly one signal: `complete`, `missing_release`, or
   `transport_failure`.
4. The sink MAY instead terminate the attempt with `limit_exceeded` while
   accepting an entry. The provider adapter MUST then stop emitting and MUST NOT
   signal a later outcome.
5. Acquisition exposes exactly one closed outcome:
   `complete(RegistryEntrySource)`, `missing_release`, `transport_failure`, or
   `limit_exceeded`.
6. Only `complete` seals the sink as one immutable `RegistryEntrySource`.
   Sealing asserts that fetching and provider-protocol decoding have completed;
   iteration over the sealed source cannot later produce a provider failure.
7. Every non-complete outcome discards the entire partial sink. Partial entries
   MUST NOT participate in tree validation, hashing, package validation, cache
   publication, or diagnostics as if they formed a candidate tree.
8. Sink ownership MAY transfer without copying. This contract requires one
   private transaction, not separate provider and compiler materializations.
9. `missing_release`, `transport_failure`, and `limit_exceeded` fail respectively
   with `PACKAGE_REGISTRY_RELEASE_MISSING`,
   `PACKAGE_REGISTRY_TRANSPORT_FAILED`, and
   `PACKAGE_REGISTRY_LIMIT_EXCEEDED`.

### Logical Entry Source

A complete `RegistryEntrySource` is a multiset of package-root-relative logical
entries. It has no transport wrapper directory and uses these closed variants:

| Entry kind | Exact payload |
| --- | --- |
| `file` | raw path bytes and exact content bytes |
| `directory` | raw path bytes |
| `symlink`, `hardlink` | raw path bytes and raw target bytes |
| `block_device`, `character_device`, `fifo`, `socket`, `other_special` | raw path bytes |

1. A provider-protocol adapter MUST preserve the complete logical multiset,
   including exact duplicate multiplicity, raw bytes, entry kinds, link targets,
   and file contents. It MUST NOT overwrite, merge, resolve, or silently drop a
   logical entry.
2. Entry declaration and delivery order are not semantic. An implementation MAY
   canonicalize a complete multiset after sealing, but MUST preserve every entry
   and its multiplicity.
3. An explicit `directory` entry is path-validated and resource-accounted, then
   discarded before canonical tree formation. It is never materialized or
   hashed; verified directories arise only as parents of accepted regular files.
4. Every link and special entry is rejected as `unsupported_entry_kind` after
   its path and acquisition resources have been validated. Link targets are
   preserved and accounted but never resolved.
5. Only accepted `file` entries become candidate tree members. Exact duplicate,
   case-collision, and file/ancestor checks therefore apply to the complete
   regular-file multiset after directory removal and unsupported-kind rejection.

### Portable Logical Limits

Every conforming v0.1 implementation uses the same logical acquisition limits:

| Resource | Maximum |
| --- | ---: |
| logical entry count | 65,536 |
| raw path bytes per entry | 4,096 |
| raw link-target bytes per link | 4,096 |
| total raw path and link-target bytes | 16,777,216 |
| regular-file content bytes per entry | 134,217,728 |
| total regular-file content bytes | 536,870,912 |
| raw path component positions per entry | 128 |

Accounting is normative:

1. The sink accounts raw logical data before UTF-8 decoding, path validation,
   entry-kind rejection, duplicate detection, or hashing. Invalid, directory,
   duplicate, link, and special entries therefore consume the applicable
   budgets.
2. Entry count includes every emitted logical entry. Path bytes include every
   entry path. Link-target bytes include `symlink` and `hardlink` targets. File
   content bytes include every `file` payload.
3. The raw path component-position count is zero for an empty path and otherwise
   one plus the number of `/` bytes. Empty components still count before the path
   is rejected.
4. Totals use checked arithmetic. An implementation MUST fail before overflow
   and MUST NOT substitute wrapping or saturating arithmetic.
5. For one entry emission, checks occur in this order: entry count, path bytes,
   path component positions, link-target bytes, total path-and-target bytes,
   regular-file bytes, then total regular-file bytes. The first exceeded check
   terminates the transaction.
6. `PACKAGE_REGISTRY_LIMIT_EXCEEDED` reports `resource`, its normative `limit`,
   and `observed_at_least = limit + 1`. It does not expose partial entry data.
7. Wire bytes, compression ratios, provider buffering, request limits, and other
   protocol resources remain provider-protocol concerns. Such limits MUST NOT
   weaken the mandatory logical acceptance profile or be represented as package
   identity.

## Canonical Registry Package Trees

Registry integrity commits to the package tree consumed by package validation
and compilation, not to a transport container. A complete acquisition first
produces one sealed logical entry source rooted at the package root; transport
decoding and fetching do not participate in package identity.

### Tree Domain

1. A candidate tree member is one regular file identified by a non-empty
   canonical relative POSIX path.
2. Every path is valid Unicode scalar text encoded as UTF-8. `/` is the only
   separator; empty, `.`, and `..` components, a leading `/`, backslash, and NUL
   are forbidden. Path identity, record ordering, and hashing do not case-fold or
   Unicode-normalize this text.
3. Duplicate exact paths and file/ancestor conflicts are invalid.
4. Symbolic links, hard links, devices, sockets, FIFOs, and other special file
   kinds are invalid. Directory entries are implicit and do not enter the hash;
   empty directories therefore carry no package-tree identity.
5. The tree includes every regular file beneath the candidate root, including
   `Eglur.toml`, sources, and non-source files. File contents are exact bytes and
   are not decoded, normalized, or line-ending converted for hashing.

### Portable Path-Collision Guard

The collision guard prevents one verified registry tree from becoming
ambiguous on common case-insensitive filesystems without changing Eglur path
identity:

1. The guard uses Unicode 17.0.0 `Simple_Case_Folding`, derived from the `C` and
   `S` mappings in that version's `CaseFolding.txt`. A scalar without such a
   mapping maps to itself. `F` and `T` mappings are not used.
2. Each complete path is mapped scalar by scalar. The mapping is
   locale-independent and applies no Unicode normalization before or after
   folding.
3. Two distinct complete paths whose mapped scalar sequences are equal make the
   candidate tree invalid. Exact duplicate paths remain the separate duplicate
   failure above.
4. This guard is only a rejection relation. It MUST NOT rewrite a path or affect
   exact path equality, tree-record ordering, module identity, diagnostics, or
   the path bytes committed by `RegistryTreeIntegrity`.

The pinned data source is Unicode 17.0.0
`https://www.unicode.org/Public/17.0.0/ucd/CaseFolding.txt`. A future Unicode
upgrade changes this portability relation and therefore requires an explicit
specification revision rather than an implementation-library update.

### Tree Digest

The canonical byte stream is constructed as follows:

1. Encode the ASCII domain separator `eglur-package-tree-v1` followed by one NUL
   byte.
2. Encode the number of regular-file records as one unsigned 64-bit big-endian
   integer.
3. Sort records by unsigned lexicographic order of their UTF-8 path bytes.
4. For each record, encode in order:
   1. the path-byte length as unsigned 64-bit big-endian,
   2. the path bytes,
   3. the content-byte length as unsigned 64-bit big-endian, and
   4. the exact content bytes.
5. Hash the complete stream with SHA-256 and render
   `tree-sha256:` followed by 64 lowercase hexadecimal digits.

An implementation MUST reject a tree that cannot be represented by this
contract. It MUST NOT substitute a partial tree digest.

### Verification Boundary

1. The expected digest is taken only from the structurally validated lock node.
2. A sealed `RegistryEntrySource` remains in bounded private staging. Its
   complete entry multiset and file kinds are validated before its tree digest
   can be accepted.
3. Raw path bytes that are not valid UTF-8 Unicode scalar text fail with
   `PACKAGE_REGISTRY_TREE_INVALID` and `reason = invalid_path_encoding`.
4. A candidate that otherwise violates the tree domain or portable
   path-collision guard
   MUST fail with `PACKAGE_REGISTRY_TREE_INVALID` before hashing. Such a failure
   is not an integrity mismatch because no canonical package tree exists.
5. For deterministic primary-error selection, reasons are considered in this
   order: `invalid_path_encoding`, `invalid_path`, `unsupported_entry_kind`,
   `duplicate_path`, `case_collision`, then `file_ancestor`. Within
   `invalid_path_encoding`, raw paths are compared by unsigned lexicographic byte
   order. Other offending paths or path pairs use unsigned lexicographic UTF-8
   byte order. Equal-path `unsupported_entry_kind` ties use this kind order:
   `symlink`, `hardlink`, `block_device`, `character_device`, `fifo`, `socket`,
   then `other_special`. For a pair, `existing_path` is the earlier path and
   `path` is the later path; for `file_ancestor`, `existing_path` is the
   ancestor.
6. After successful tree validation, the implementation computes the complete
   canonical tree digest. A mismatch MUST fail with
   `PACKAGE_REGISTRY_INTEGRITY_MISMATCH`; it is not ordinary lock staleness and
   MUST NOT be repaired by rewriting the lock.
7. No candidate file may be exposed as a verified cache entry, package root, or
   compiler input until the complete tree digest matches the lock.
8. After successful tree verification and target package validation,
   publication into a shared cache or package root MUST satisfy the verified
   snapshot lifecycle below.
9. Container ordering, compression, timestamps, permissions, and other
   transport metadata do not affect the tree digest. Repacked artifacts with
   the same canonical tree are accepted; any tree change fails.
10. Tree integrity does not authenticate the first selected digest. Registry
   trust, signatures, and provenance are separate concerns.

### Verified Snapshot Cache Lifecycle

A registry cache is a recoverable optimization. It does not select a package,
version, or tree integrity value and is not a durable source of package
authority.

1. Cache lookup for one complete `RegistrySourceCoordinate` returns either a
   miss or one `VerifiedTreeSnapshot` bound to that exact coordinate. A
   package/version-only key, integrity-only key, stale entry, or
   coordinate-mismatched entry MUST NOT produce a hit.
2. Only a snapshot committed after complete acquisition, canonical tree
   validation, tree-integrity comparison, and target package validation may be
   returned as a hit. Current compiler package, manifest, and source validation
   still applies after lookup; commit evidence does not freeze
   compiler-version-dependent validation forever.
3. An implementation MAY establish a hit by revalidating the cached tree, or by
   trusting implementation-owned commit evidence together with an enforced
   immutable-storage invariant. Cache layout, commit-receipt bytes, and the
   revalidation schedule are implementation-defined.
4. A reader observes either a miss or one complete immutable snapshot for the
   reader's use. Publishing, partial, losing, interrupted, stale, or otherwise
   unusable physical state MUST NOT be exposed as a package root, compiler
   input, or cache hit.
5. Publication for one coordinate has one logical linearization point from
   absent to committed. Once committed, a snapshot MUST NOT be replaced,
   mutated, or invalidated by a competing publication attempt.
6. Concurrent successful attempts for the same coordinate converge on the same
   logical snapshot. A winning attempt may publish its private candidate. Every
   losing attempt MUST discard its private candidate and use the committed
   snapshot. A failed, cancelled, or losing attempt MUST NOT damage a committed
   snapshot or leave state reachable as a hit.
7. Single-flight acquisition, duplicate provider requests, per-coordinate or
   wider locks, compare-and-publish, waiting policy, and the physical cleanup of
   losing candidates are implementation-defined. Provider request count and
   candidate identity are not portable concurrent-execution assertions.
8. Cache state recovered by a later invocation has only two semantic
   classifications: miss or hit with a valid `VerifiedTreeSnapshot`.
   Recognizable incomplete, uncommitted, losing, stale, or unusable state MUST
   become a miss and MUST remain unreachable from package lookup. Cleanup MAY
   occur eagerly, lazily, or through separate maintenance.
9. v0.1 does not require cache durability. Abrupt process termination, host
   failure, power loss, eviction, or cache migration may lose a committed entry
   and cause a later miss. v0.1 does not require `fsync`, a journal format, a
   clean-shutdown marker, or portable recovery metadata.
10. Failure to inspect, read, publish, or maintain an optional shared cache MUST
    NOT change package selection or become a `PACKAGE_*` diagnostic. An
    implementation MUST NOT use an entry whose verified-snapshot status cannot
    be established. It MAY report an implementation diagnostic, warning, or log
    event and reacquire the exact locked coordinate.
11. If acquisition, tree verification, and target package validation succeed,
    and a usable private verified snapshot remains available, failure to publish
    that snapshot to an optional shared cache MUST NOT fail compilation. The
    invocation continues from the private snapshot. If no usable verified
    package root can be retained for the current compilation, the invocation may
    fail as an implementation or environment failure, not as a registry
    transport or package-integrity diagnostic.
12. Cache failure MUST NOT downgrade, replace, or bypass provider outcomes,
    acquisition limits, tree validation, integrity comparison, target package
    validation, or source validation. Filesystem or hardware corruption and
    out-of-band cache mutation remain outside portable source conformance.
    Implementations SHOULD provide an audit or repair path for such state.

## Relative Path Resolution

A path dependency passes through three representations exactly once:

1. `DeclaredDependencyPath` is the exact decoded TOML string and its source span.
   It is retained for source blame and diagnostic `path` data.
2. `CanonicalPathSelector` is the portable lexical form used by manifest/lock
   comparison.
3. `ResolvedPackageRoot` is the host-local, fully resolved directory key used
   only for package-instance interning.

### Selector Validation and Normalization

For a present TOML string `path` value, validation and normalization are
host-independent:

1. The empty string is invalid with `PACKAGE_DEP_PATH_INVALID` and
   `reason = empty`.
2. A path is absolute or drive-qualified when it starts with `/`, starts with
   two backslashes, or starts with an ASCII letter followed by `:`. These paths
   are invalid with `reason = absolute`.
3. Any remaining path containing `\` is invalid with `reason = backslash`.
4. The checks above run in listed order. The diagnostic MUST report the selected
   dependency key and exact `DeclaredDependencyPath` as `dependency` and `path`.
5. The valid spelling is split on `/`. Empty and `.` components are discarded.
   Each `..` removes the nearest preceding ordinary component when one exists;
   otherwise that leading `..` is preserved. If no component remains, the
   canonical spelling is `.`. All other component text is preserved exactly.
6. The remaining components are joined with one `/`. Implementations MUST NOT
   apply case conversion, Unicode normalization, locale rules, environment
   expansion, tilde expansion, or host separator rewriting.
7. The result is `CanonicalPathSelector`. It remains relative and MAY begin
   with one or more `..` components; v0.1 does not confine dependencies beneath
   the declaring package root.

Lexical normalization precedes filesystem traversal. In particular, a removed
component is not first followed as a symbolic link.

### Filesystem Resolution

1. A `CanonicalPathSelector` is evaluated relative to the directory containing
   the `Eglur.toml` that declares it.
2. For a dependency manifest reached through an alias, child selectors use that
   dependency's `ResolvedPackageRoot`, not the incoming selector spelling.
3. Resolution MUST follow the complete symbolic-link chain and produce one
   `ResolvedPackageRoot` for the final directory. Distinct selectors known to
   reach the same final directory through dot spelling or symbolic links MUST
   compare equal for interning. Equivalence through hard links, bind mounts, or
   other host aliases is outside the v0.1 portable contract.
4. Resolution failure uses `PACKAGE_DEP_PATH_UNRESOLVED` with the dependency key,
   exact declared path, and one closed `reason`:
   1. `not_found` when a path component is absent without following a dangling
      symbolic link,
   2. `not_directory` when the resolved target or required intermediate target
      is not a directory,
   3. `broken_symlink` when a followed symbolic link has no target, or
   4. `symlink_cycle` when symbolic-link traversal repeats a link-resolution
      state.
5. The first failure encountered while resolving the selected dependency edge
   determines the reason.
6. Once resolution yields a directory, ordinary package-root and manifest
   validation applies. Missing `Eglur.toml`, missing `src/`, and malformed target
   manifest fields retain their existing package diagnostics; dependency-target
   root failures also report the selecting dependency and declared path.

## Resolver Determinism and Validation Order

v0.1 requires deterministic resolver observables.

Given fixed package inputs (`Eglur.toml`, `Eglur.lock`, dependency manifests,
registry package trees, source trees, and symbolic-link topology),
implementations MUST produce the same resolved package-instance/source/version
mapping or the same deterministic failure code.

Validation order is normative:

1. Validate the selected root and parse its `Eglur.toml`. Package-root failures
   use `PACKAGE_MANIFEST_MISSING` or `PACKAGE_SRC_ROOT_MISSING`.
2. Validate root manifest package metadata, the complete `[registries]` table,
   dependency keys, and dependency entry/lexical selector shapes without
   resolving paths or acquiring registry trees.
3. Require top-level `Eglur.lock`, then validate its complete lock-only syntax,
   closed schema, canonical labels and order, node variants, edges,
   reachability, and internal source-coordinate consistency. Invalid dependency
   keys retain `PACKAGE_DEP_KEY_INVALID`; other lock-only failures use
   `PACKAGE_LOCK_INVALID`.
4. Synchronize the root manifest with the `$root` lock node.
5. Traverse the validated locked graph in canonical first-discovery depth-first
   order. For each edge, compare its declaring manifest constraint, resolve its
   path or acquire and hash its registry tree, validate the target package root
   and manifest, then apply package interning, lock-topology convergence,
   direct-duplicate, active-stack cycle, registry-version, and package-name
   checks in that order.
6. Resolve modules and imports only from each owning lock node's validated
   dependency map, then run later compilation phases.

No filesystem traversal, registry acquisition, or nested manifest traversal may
occur before the lock structurally authorizes the selected edge. Nested
manifests constrain and provide source blame for locked selections; they never
add or select an omitted node or edge. Compilation MUST NOT generate, complete,
normalize, rewrite, or fall back from `Eglur.lock`.

For one authorized registry edge, the acquisition suborder is also normative:

1. A terminal `missing_release`, `transport_failure`, or `limit_exceeded`
   outcome fails immediately with its dedicated diagnostic. These closed
   outcomes are mutually exclusive and contain no candidate tree.
2. A `complete` outcome proceeds through raw path and entry-kind validation,
   regular-file tree validation, complete tree hashing, and lock-integrity
   comparison in that order.
3. Only a matching canonical tree proceeds to package-root, manifest, and source
   validation. Existing package diagnostics remain primary for failures in that
   later phase.

Dependency graph traversal order is also normative in v0.1:

1. Each lock node's dependency edges MUST be traversed in ascending
   dependency-key order under the
   [canonical lexical text order](invariants.md#canonical-lexical-text-order).
2. This order is independent of declaration order in `Eglur.toml` and table
   order in a noncanonical lock, which is rejected before traversal.
3. If multiple graph-phase failures are possible, the first failure under this
   traversal and the per-edge order above is primary.
4. Before traversing one package's edges, its reached manifest metadata,
   dependency keys, and entry/selector shapes are validated in the same
   manifest order. Within a phase, canonical dependency-key order selects the
   primary failure.
5. An unvisited valid instance is traversed recursively. A completely visited
   instance is reused without another traversal.
6. The root package establishes the first graph name binding with dependency
   label `<root>` before dependency traversal begins.

## Dependency Visibility Boundary

Import visibility across packages is direct-only in v0.1:

1. A package may import its own modules.
2. A package may import modules from dependencies declared directly in its own `[dependencies]`.
3. A package MUST NOT use a transitive-only dependency's key as an import root.
4. A package MAY import a public re-export through a direct dependency's module
   path even when the re-export's canonical declaration owner is transitive-only.
   This preserves the canonical target identity but does not add that owner's
   dependency key to the importing package's root set, as defined in
   `modules-resolution.md`.

When an import path uses a transitive-only dependency key as its first segment,
compilation MUST fail with `IMPORT_NOT_FOUND`.

## Dependency Graph Cycles

Package dependency cycles are forbidden in v0.1 package mode.

If dependency resolution encounters a package cycle, compilation MUST fail before module/type checking.

Package dependency-cycle failure MUST use `PACKAGE_DEP_CYCLE`. Cycle detection
uses `PackageInstanceId`, not package names or selector spellings.

## Lockfile Policy

`Eglur.lock` is required in package mode.

Canonical v0.1 lockfile shape:

```toml
version = 1
root = "$root"

[[packages]]
node = "$root"
name = "my_pkg"
version = "0.1.0"
source = "root"

[packages.dependencies]
dep_a = { target = "$root/dep_a" }
dep_b = { target = "$root/dep_b", path = "../dep_b" }

[[packages]]
node = "$root/dep_a"
name = "dep_a"
version = "1.2.3"
source = "registry"
package = "packages.example.com/acme/dep-a"
integrity = "tree-sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[[packages]]
node = "$root/dep_b"
name = "dep_b"
version = "1.0.0"
source = "path"
```

### Top-Level and Node Schema

1. Package-mode builds MUST include top-level `Eglur.lock`; absence fails with
   `PACKAGE_LOCK_MISSING`.
2. The top level contains exactly integer `version = 1`, string
   `root = "$root"`, and a non-empty `packages` array of tables.
3. Every package record has exactly these common fields: canonical `node`, valid
   `name`, canonical exact `version` under the manifest value domain above, and
   `source` equal to `root`, `path`, or `registry`.
4. The closed source variants are:
   1. root: common fields only; exactly one record with `node = "$root"`,
   2. path: common fields only, and
   3. registry: common fields plus canonical `package` and canonical
      `integrity`.
5. `source` is never inferred. `package` and `integrity` are required together
   only for registry nodes and forbidden otherwise. `path` is not a node field.
6. A node with no dependencies canonically omits
   `[packages.dependencies]`. A present dependency table MUST be non-empty.
7. Unknown fields, wrong types, missing variant fields, misplaced fields, and
   noncanonical values make the lock invalid.

### Locked Node IDs and Canonical Order

1. `LockedNodeId` is exactly `$root` or `$root/` followed by one or more
   slash-separated valid `DependencyKey` values. It is a generated lock-local
   label, not a filesystem path or stable package identity.
2. The first edge that discovers a node under canonical depth-first traversal
   assigns the dependency-key chain by which it was discovered. Later
   convergent edges reference that same label.
3. `packages` records occur exactly once per node in first-discovery order. The
   root record is first.
4. Each dependency table is serialized in canonical dependency-key order.
5. Every target exists, every non-root node is reachable from `$root`, and the
   lock contains no duplicate node or unreachable record. Backedges may target
   an active node and are later rejected as package cycles rather than dangling
   topology.
6. Registry records with equal complete `RegistrySourceCoordinate` values MUST
   be represented by one lock node. Two records with one registry ID and exact
   version but different integrity values are internally contradictory.
7. Invalid IDs, noncanonical discovery labels/order, duplicate/dangling nodes,
   unreachable nodes, and internally source-incompatible edges fail with
   `PACKAGE_LOCK_INVALID`; compilation never normalizes them into acceptance.

### Edge Schema

1. An edge key is one valid `DependencyKey`. An invalid decoded key uses
   `PACKAGE_DEP_KEY_INVALID` before other lock-invalid checks.
2. A path edge is exactly `{ target = "<LockedNodeId>", path =
   "<CanonicalPathSelector>" }`. Its target is a path node or `$root`.
3. A registry edge is exactly `{ target = "<LockedNodeId>" }`. Its target is a
   registry node; package, version, and integrity are not duplicated on the
   edge.
4. A path edge cannot target a registry node, and a registry edge cannot target
   a root or path node. Cross-source convergence is invalid.
5. A lock path is already canonical. Empty, absolute, drive-qualified,
   backslash-containing, or lexically noncanonical text fails with
   `PACKAGE_LOCK_INVALID`; it is never normalized during lock validation.

### Manifest Synchronization and Authority

1. The top-level lock selects every package node and direct edge available to
   compilation. Dependency-owned lockfiles are ignored.
2. Each reached manifest remains the declaration constraint and source-blame
   artifact for its node. Its package metadata and dependency key set MUST equal
   the corresponding lock record.
3. Each manifest path selector is normalized to `CanonicalPathSelector` and
   MUST equal the edge `path`. Each manifest registry selector is expanded to
   `RegistryPackageId` and, with its exact version, MUST equal the target
   registry node.
4. A lock-only node or edge, a manifest-only dependency, a metadata mismatch,
   or a selector mismatch fails with `PACKAGE_LOCK_OUT_OF_SYNC`.
5. Mismatch precedence is package metadata, then dependency key set, then
   dependency selector. Nodes and edges are considered in canonical traversal
   order; selector ties use dependency-key order.
6. `PACKAGE_LOCK_OUT_OF_SYNC` data includes the selected `node` and `reason`
   equal to `package_metadata`, `dependency_key_set`, or
   `dependency_selector`. Selector failures additionally identify `dependency`.
7. If two distinct locked path nodes resolve to one `ResolvedPackageRoot`, or
   incoming path edges to one locked node resolve to distinct roots, the lock's
   topology disagrees with the resolved graph and fails as
   `PACKAGE_LOCK_OUT_OF_SYNC` with `reason = dependency_selector`. The offending
   later edge is selected by canonical traversal.
8. Resolver selection is taken only from the validated lock. A verified cache
   may satisfy acquisition but cannot select a different source coordinate.
9. `Eglur.lock` MUST NOT contain `ResolvedPackageRoot`, `PackageInstanceId`,
   registry aliases, transport endpoints, or another host-local or graph-local
   identity value.

### Diagnostic Blame

1. `PACKAGE_LOCK_INVALID` points to the offending lock syntax or field.
2. For a fact shared by a manifest and a well-formed lock,
   `PACKAGE_LOCK_OUT_OF_SYNC` points to the exact manifest declaration and
   relates the corresponding lock field. A lock-only dependency necessarily
   points to its lock key.
3. A path false split or false merge caused only by an incorrect lock target
   points to that target and relates the selecting manifest dependency.
4. `PACKAGE_REGISTRY_RELEASE_MISSING`, `PACKAGE_REGISTRY_TRANSPORT_FAILED`, and
   `PACKAGE_REGISTRY_LIMIT_EXCEEDED` point to the selected registry node's lock
   `package` field and report `package` and `version`. The limit diagnostic also
   reports `resource`, `limit`, and `observed_at_least`.
5. `PACKAGE_REGISTRY_TREE_INVALID` points to the selected registry node's lock
   `package` field and reports `package`, `version`, and `reason`. It reports
   `path_hex` for `invalid_path_encoding`; other reasons report `path`.
   `unsupported_entry_kind` also reports `entry_kind`; `case_collision` and
   `file_ancestor` also report `existing_path`.
6. `PACKAGE_REGISTRY_INTEGRITY_MISMATCH` points to the expected lock integrity
   field and reports `package`, `version`, `expected`, and `actual`; both digest
   values use canonical `tree-sha256:` spelling.
7. Graph conflicts point to the later manifest edge under canonical traversal
   and relate the edge that established the conflicting fact.
