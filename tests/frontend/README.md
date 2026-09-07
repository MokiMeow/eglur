# Syntax front-end corpus

`expectations.json` is the generated input contract for the production Eglur
lexer and parser. It projects every manifest-owned conformance source, linked
public example, and authored focused syntax case into three questions:

1. Where did this source unit come from?
2. Which lexer or parser entry point consumes it?
3. Should that front-end stage accept it, or emit a specific `LEX_*` or
   `PARSE_*` diagnostic?

A semantic compile error is therefore a front-end success. The corpus does not
claim that such a program compiles, executes, or conforms beyond syntax.

The conformance manifest, tracked `eglur fixture=<test-id>` Markdown fences,
and `syntax-cases.json` remain authoritative. Focused syntax cases cover lexer
and parser entry points that cannot be expressed as complete conformance
programs. Do not edit `expectations.json` by hand. Regenerate or verify it
through the Python specification environment:

```console
pixi -q run --locked generate-frontend-expectations
pixi -q run --locked validate-frontend-expectations
```

The checked-in copy lets the Rust front end consume one closed, deterministic
artifact without parsing YAML or Markdown. Its input digest and byte-exact
freshness check prevent it from becoming a second source of truth.
