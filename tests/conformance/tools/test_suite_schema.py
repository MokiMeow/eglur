"""Regression tests for conformance schema source validation."""

import unittest
from typing import cast

from eglur_conformance.suite.schema import (
    YamlValue,
    validate_assignment_targets,
    validate_diagnostic_assertion,
    validate_expect,
    validate_test_case,
    validate_value_assertion,
)


def test_case(
    code: str,
    compile_error_code: str | None = None,
    test_id: str = "TEST-assignment-validation",
) -> dict[str, YamlValue]:
    """Build the minimum source shape needed by assignment validation."""
    expect = (
        {"compile_error": {"code": compile_error_code}}
        if compile_error_code is not None
        else {"value": {"kind": "Ok", "repr": '"ok"'}}
    )
    return {"id": test_id, "code": code, "expect": expect}


class AssignmentTargetValidationTests(unittest.TestCase):
    """Exercise source-aware assignment-target validation."""

    def validate(
        self,
        code: str,
        compile_error_code: str | None = None,
        test_id: str = "TEST-assignment-validation",
    ) -> list[str]:
        """Return validation messages for a source snippet."""
        return [
            problem.message
            for problem in validate_assignment_targets(
                test_case(code, compile_error_code, test_id),
                "suite.tests[0]",
            )
        ]

    def test_accepts_discard_and_mutable_reassignment(self) -> None:
        code = """fn main!() -> Res[Str, Str] {
  let mut (left, right) = (1, 2)
  left = right
  _ = left
  yield Ok("ok")
}
"""

        self.assertEqual(self.validate(code), [])

    def test_rejects_unresolved_bare_assignment(self) -> None:
        problems = self.validate(
            """fn main!() -> Res[Str, Str] {
  temporary = work()?
  yield Ok("ok")
}
"""
        )

        self.assertEqual(len(problems), 1)
        self.assertIn("'temporary' resolves to an unresolved identifier", problems[0])

    def test_rejects_inline_underscore_assignment(self) -> None:
        problems = self.validate(
            """fn main!() -> Res[Str, Str] {
  let value = 1.tap { |_| _temporary = work() }
  yield Ok(to_str(value))
}
"""
        )

        self.assertEqual(len(problems), 1)
        self.assertIn("'_temporary' resolves to an unresolved identifier", problems[0])

    def test_rejects_inline_identifier_assignment(self) -> None:
        problems = self.validate(
            """fn main!() -> Res[Str, Str] {
  if true { temporary = work() } else { _ = () }
  yield Ok("ok")
}
"""
        )

        self.assertEqual(len(problems), 1)
        self.assertIn("'temporary' resolves to an unresolved identifier", problems[0])

    def test_rejects_assignment_after_union_typed_lambda_parameters(self) -> None:
        problems = self.validate(
            """fn main!() -> Res[Str, Str] {
  let value = apply { |item: Union[i64, Str]| temporary = item }
  yield Ok(to_str(value))
}
"""
        )

        self.assertEqual(len(problems), 1)
        self.assertIn("'temporary' resolves to an unresolved identifier", problems[0])

    def test_rejects_typed_bare_binding(self) -> None:
        problems = self.validate(
            """fn main!() -> Res[Str, Str] {
  temporary: i64 = 1
  yield Ok(to_str(temporary))
}
"""
        )

        self.assertEqual(len(problems), 1)
        self.assertIn("typed bare binding 'temporary'", problems[0])

    def test_rejects_inline_typed_bare_binding(self) -> None:
        problems = self.validate(
            """fn main!() -> Res[Str, Str] {
  if true { temporary: i64 = 1 }
  yield Ok("ok")
}
"""
        )

        self.assertEqual(len(problems), 1)
        self.assertIn("typed bare binding 'temporary'", problems[0])

    def test_registers_inline_declaration_after_statement_end(self) -> None:
        code = """fn main!() -> Res[Str, Str] {
  if true { let mut local = 1
    local = 2
  }
  yield Ok("ok")
}
"""

        self.assertEqual(self.validate(code), [])

    def test_does_not_expose_binding_inside_its_initializer(self) -> None:
        problems = self.validate(
            """fn main!() -> Res[Str, Str] {
  let mut local = if true { local = 1 } else { _ = 0 }
  yield Ok("ok")
}
"""
        )

        self.assertEqual(len(problems), 1)
        self.assertIn("'local' resolves to an unresolved identifier", problems[0])

    def test_rejects_immutable_reassignment(self) -> None:
        problems = self.validate(
            """fn main!() -> Res[Str, Str] {
  let value = 1
  value = 2
  yield Ok("ok")
}
"""
        )

        self.assertEqual(len(problems), 1)
        self.assertIn("'value' resolves to an immutable local", problems[0])

    def test_preserves_intentional_assignment_diagnostics(self) -> None:
        unresolved = self.validate(
            """fn main!() -> Res[Str, Str] {
  x = 1
  yield Ok("ok")
}
""",
            "ASSIGN_TARGET_NOT_ASSIGNABLE",
            "TYC-bare-assignment-does-not-introduce-local",
        )
        immutable = self.validate(
            """fn main!() -> Res[Str, Str] {
  let x = 1
  x = 2
  yield Ok("ok")
}
""",
            "IMMUTABLE_BINDING_ASSIGN",
            "TYC-immutable-let-binding-rejects-reassignment",
        )

        self.assertEqual(unresolved, [])
        self.assertEqual(immutable, [])

    def test_intentional_diagnostic_allowance_is_single_use(self) -> None:
        problems = self.validate(
            """fn main!() -> Res[Str, Str] {
  x = 1
  x = 2
  yield Ok("ok")
}
""",
            "ASSIGN_TARGET_NOT_ASSIGNABLE",
            "TYC-bare-assignment-does-not-introduce-local",
        )

        self.assertEqual(len(problems), 1)

    def test_other_assignment_diagnostic_does_not_allow_accidental_target(self) -> None:
        problems = self.validate(
            """fn one() -> i64 = 1
fn main!() -> Res[Str, Str] {
  accidental = 1
  one() = 2
  yield Ok("ok")
}
""",
            "ASSIGN_TARGET_NOT_ASSIGNABLE",
            "TYC-call-result-is-not-assignable-target",
        )

        self.assertEqual(len(problems), 1)
        self.assertIn("'accidental' resolves to an unresolved identifier", problems[0])

    def test_intentional_diagnostic_allowance_is_single_use_across_files(self) -> None:
        test: dict[str, YamlValue] = {
            "id": "TYC-bare-assignment-does-not-introduce-local",
            "files": {
                "first.eglur": "x = 1\n",
                "second.eglur": "x = 2\n",
            },
            "expect": {
                "compile_error": {
                    "code": "ASSIGN_TARGET_NOT_ASSIGNABLE",
                }
            },
        }

        problems = validate_assignment_targets(test, "suite.tests[0]")

        self.assertEqual(len(problems), 1)

    def test_ignores_named_arguments_in_multiline_calls(self) -> None:
        code = """fn main!() -> Res[Str, Str] {
  let output = call(
    input,
    mode=Strict,
    output_axis = axis(0)
  )
  yield Ok("ok")
}
"""

        self.assertEqual(self.validate(code), [])

    def test_ignores_non_assignment_underscore_identifiers(self) -> None:
        code = """type _Count = i64
struct Counter {
  field mut _value: i64
}
impl Counter {
  fn set!(self: &mut Self, value: i64) -> Unit {
    self._value = value
  }
}
fn one() -> _Count = 1
"""

        self.assertEqual(self.validate(code), [])

    def test_checks_statements_inside_multiline_lambda_arguments(self) -> None:
        problems = self.validate(
            """fn main!() -> Res[Str, Str] {
  let value = call(
    { |item|
      temporary = item
      item
    }
  )
  yield Ok(to_str(value))
}
"""
        )

        self.assertEqual(len(problems), 1)
        self.assertIn("'temporary' resolves to an unresolved identifier", problems[0])

    def test_mutable_binding_does_not_escape_lexical_block(self) -> None:
        problems = self.validate(
            """fn main!() -> Res[Str, Str] {
  if true {
    let mut local = 1
  }
  local = 2
  yield Ok("ok")
}
"""
        )

        self.assertEqual(len(problems), 1)
        self.assertIn("'local' resolves to an unresolved identifier", problems[0])

    def test_ignores_non_eglur_files_in_file_sets(self) -> None:
        test: dict[str, YamlValue] = {
            "files": {
                "main.eglur": """fn main!() -> Res[Str, Str] {
  _ = work()?
  yield Ok("ok")
}
""",
                "Eglur.toml": "name = 'fixture'\n",
            },
            "expect": {"value": {"kind": "Ok", "repr": '"ok"'}},
        }

        self.assertEqual(validate_assignment_targets(test, "suite.tests[0]"), [])


class DiagnosticAssertionValidationTests(unittest.TestCase):
    """Exercise the portable structured diagnostic schema."""

    def test_accepts_empty_portable_lists(self) -> None:
        diagnostic: YamlValue = {
            "code": "METHOD_NOT_FOUND",
            "message_contains": [],
            "data_keys": [],
        }

        self.assertEqual(
            validate_diagnostic_assertion(diagnostic, "diagnostic"),
            [],
        )

    def test_accepts_one_of_groups_and_optional_rendering_probes(self) -> None:
        diagnostic: YamlValue = {
            "code": "EFFECT_VIOLATION",
            "message_contains": [],
            "message_contains_any": [["non-bang", "Fn", "pure"]],
            "help_contains": ["make the caller bang"],
            "related_contains": ["effect source"],
            "data_keys": [],
        }

        self.assertEqual(
            validate_diagnostic_assertion(diagnostic, "diagnostic"),
            [],
        )

    def test_rejects_empty_one_of_group(self) -> None:
        diagnostic: YamlValue = {
            "code": "EFFECT_VIOLATION",
            "message_contains": [],
            "message_contains_any": [[]],
            "data_keys": [],
        }

        problems = validate_diagnostic_assertion(diagnostic, "diagnostic")

        self.assertEqual(len(problems), 1)
        self.assertIn("message_contains_any[0] must be non-empty", problems[0].message)

    def test_rejects_unknown_diagnostic_field(self) -> None:
        diagnostic: YamlValue = {
            "code": "EFFECT_SIGNATURE",
            "message_contains": ["&mut Self"],
            "data_keys": [],
            "message_prefix": "method",
        }

        problems = validate_diagnostic_assertion(diagnostic, "diagnostic")

        self.assertEqual(len(problems), 1)
        self.assertIn("message_prefix is not a supported key", problems[0].message)

    def test_diagnostic_data_equals_uses_str_i64_bool_domain(self) -> None:
        accepted = ("name", True, -(2**63), 2**63 - 1)
        for value in accepted:
            diagnostic: YamlValue = {
                "code": "NAME_NOT_FOUND",
                "message_contains": [],
                "data_keys": ["value"],
                "data_equals": {"value": value},
            }
            with self.subTest(accepted=value):
                self.assertEqual(
                    validate_diagnostic_assertion(diagnostic, "diagnostic"),
                    [],
                )

        rejected = (None, 1.0, -(2**63) - 1, 2**63)
        for value in rejected:
            diagnostic = {
                "code": "NAME_NOT_FOUND",
                "message_contains": [],
                "data_keys": ["value"],
                "data_equals": {"value": value},
            }
            with self.subTest(rejected=value):
                problems = validate_diagnostic_assertion(
                    diagnostic,
                    "diagnostic",
                )
                self.assertEqual(len(problems), 1)
                self.assertIn("must be Str, i64, or Bool", problems[0].message)


class NumericCompareValidationTests(unittest.TestCase):
    """Exercise the closed typed numeric comparison object."""

    def validate(self, numeric_compare: YamlValue) -> list[str]:
        """Return numeric comparator validation messages."""
        return [
            problem.message
            for problem in validate_value_assertion(
                {
                    "kind": "Ok",
                    "numeric_compare": numeric_compare,
                },
                "expect.value",
            )
        ]

    def test_accepts_exact_and_approx_modes(self) -> None:
        for numeric_compare in (
            {
                "dtype": "i64",
                "shape": [2],
                "expected": [1, 2],
                "mode": "exact",
            },
            {
                "dtype": "f64",
                "expected": 1.0,
                "mode": "approx",
                "rtol": 1e-6,
                "atol": 0.0,
            },
        ):
            with self.subTest(numeric_compare=numeric_compare):
                self.assertEqual(self.validate(numeric_compare), [])

    def test_rejects_unknown_numeric_compare_mode(self) -> None:
        for mode in ("fuzzy", [], {}):
            with self.subTest(mode=mode):
                problems = self.validate(
                    {
                        "dtype": "f64",
                        "expected": 1.0,
                        "mode": mode,
                    }
                )

                self.assertEqual(len(problems), 1)
                self.assertIn("mode must be one of", problems[0])

    def test_rejects_nested_expected_values_and_shape_mismatch(self) -> None:
        nested = self.validate(
            {
                "dtype": "f64",
                "shape": [1],
                "expected": [[1.0]],
            }
        )
        mismatched = self.validate(
            {
                "dtype": "f64",
                "shape": [2, 2],
                "expected": [1.0, 2.0],
            }
        )

        self.assertTrue(any("must be a scalar" in problem for problem in nested))
        self.assertTrue(
            any(
                "does not match shape element count" in problem
                for problem in mismatched
            )
        )


class ExpectationPayloadValidationTests(unittest.TestCase):
    """Require scalar/list types for secondary expectation evidence."""

    def validate(self, expect: YamlValue) -> list[str]:
        """Return expectation validation messages."""
        return [
            problem.message
            for problem in validate_expect(
                expect,
                "expect",
            )
        ]

    def test_rejects_non_string_repr(self) -> None:
        problems = self.validate({"value": {"kind": "Ok", "repr": []}})

        self.assertTrue(any("repr must be a string" in problem for problem in problems))

    def test_rejects_non_list_secondary_payloads(self) -> None:
        side_effect_problems = self.validate(
            {
                "value": {"kind": "Ok", "repr": '"ok"'},
                "side_effects": {"first": "output"},
            }
        )
        diagnostic_problems = self.validate(
            {
                "value": {"kind": "Ok", "repr": '"ok"'},
                "diagnostic_contains": {"code": "WARNING"},
            }
        )

        self.assertTrue(
            any(
                "side_effects must be a list" in problem
                for problem in side_effect_problems
            )
        )
        self.assertTrue(
            any(
                "diagnostic_contains must be a list" in problem
                for problem in diagnostic_problems
            )
        )


class FileSetRootValidationTests(unittest.TestCase):
    """Exercise deterministic roots for manifest-less multi-source file sets."""

    def validate(
        self,
        files: dict[str, YamlValue],
        *,
        file_set_root: YamlValue | None = None,
        mode: str | None = None,
    ) -> list[str]:
        """Return schema problems for one multi-file fixture."""
        fixture: dict[str, YamlValue] = {
            "id": "MOD-file-set-root-example",
            "category": "modules",
            "title": "file-set root example",
            "files": files,
            "entry": next(path for path in files if path.endswith(".eglur")),
            "expect": {"value": {"kind": "Ok", "repr": '"ok"'}},
        }
        if file_set_root is not None:
            fixture["file_set_root"] = file_set_root
        if mode is not None:
            fixture["mode"] = mode

        _, problems = validate_test_case(fixture, "core/modules.yaml", 0)
        return [problem.message for problem in problems]

    def test_accepts_explicit_root_for_ambiguous_multi_root_sources(self) -> None:
        problems = self.validate(
            {
                "app/main.eglur": "fn main() -> Unit {}\n",
                "lib/value.eglur": "fn value() -> i64 = 1\n",
            },
            file_set_root=".",
        )

        self.assertEqual(problems, [])

    def test_accepts_nested_root_and_non_source_outside_it(self) -> None:
        problems = self.validate(
            {
                "workspace/main.eglur": "fn main() -> Unit {}\n",
                "workspace/lib/value.eglur": "fn value() -> i64 = 1\n",
                "README.md": "fixture notes\n",
            },
            file_set_root="workspace",
        )

        self.assertEqual(problems, [])

    def test_rejects_non_scalar_source_path_and_entry(self) -> None:
        problems = self.validate({"bad\ud800.eglur": "fn main() -> Unit {}\n"})

        self.assertTrue(
            any("files has invalid path" in problem for problem in problems)
        )
        self.assertTrue(
            any("entry must be Unicode scalar text" in problem for problem in problems)
        )

    def test_requires_root_for_manifestless_multi_source_file_set(self) -> None:
        problems = self.validate(
            {
                "app/main.eglur": "fn main() -> Unit {}\n",
                "app/value.eglur": "fn value() -> i64 = 1\n",
            }
        )

        self.assertTrue(
            any("file_set_root is required" in problem for problem in problems)
        )

    def test_requires_root_when_file_set_mode_overrides_manifest(self) -> None:
        problems = self.validate(
            {
                "Eglur.toml": '[package]\nname = "fixture"\n',
                "app/main.eglur": "fn main() -> Unit {}\n",
                "app/value.eglur": "fn value() -> i64 = 1\n",
            },
            mode="file-set",
        )

        self.assertTrue(
            any("file_set_root is required" in problem for problem in problems)
        )

    def test_rejects_root_for_effective_package_mode(self) -> None:
        problems = self.validate(
            {
                "Eglur.toml": '[package]\nname = "fixture"\n',
                "src/main.eglur": "fn main() -> Unit {}\n",
                "src/value.eglur": "fn value() -> i64 = 1\n",
            },
            file_set_root="src",
        )

        self.assertTrue(
            any(
                "file_set_root is invalid in package mode" in problem
                for problem in problems
            )
        )

    def test_rejects_root_for_single_source_file_set(self) -> None:
        problems = self.validate(
            {"app/main.eglur": "fn main() -> Unit {}\n"},
            file_set_root=".",
        )

        self.assertTrue(
            any(
                "file_set_root is invalid for a single-source file set" in problem
                for problem in problems
            )
        )

    def test_rejects_noncanonical_roots(self) -> None:
        files: dict[str, YamlValue] = {
            "app/main.eglur": "fn main() -> Unit {}\n",
            "app/value.eglur": "fn value() -> i64 = 1\n",
        }

        for root in (
            "/app",
            "../app",
            "./app",
            "app/",
            "app//src",
            r"app\src",
            1,
        ):
            with self.subTest(root=root):
                problems = self.validate(files, file_set_root=root)
                self.assertTrue(
                    any(
                        "file_set_root must be a canonical relative POSIX path"
                        in problem
                        for problem in problems
                    )
                )

    def test_rejects_noncanonical_mapped_paths(self) -> None:
        for source_path in (
            "/app/main.eglur",
            "../app/main.eglur",
            "./app/main.eglur",
            "app//main.eglur",
            r"app\main.eglur",
        ):
            with self.subTest(source_path=source_path):
                problems = self.validate(
                    {
                        source_path: "fn main() -> Unit {}\n",
                        "app/value.eglur": "fn value() -> i64 = 1\n",
                    },
                    file_set_root=".",
                )
                self.assertTrue(
                    any(
                        f"files has invalid path {source_path}" in problem
                        for problem in problems
                    )
                )

    def test_rejects_source_outside_root(self) -> None:
        problems = self.validate(
            {
                "app/main.eglur": "fn main() -> Unit {}\n",
                "lib/value.eglur": "fn value() -> i64 = 1\n",
            },
            file_set_root="app",
        )

        self.assertTrue(
            any(
                "source lib/value.eglur is outside file_set_root app" in problem
                for problem in problems
            )
        )

    def test_rejects_root_with_inline_code(self) -> None:
        fixture: dict[str, YamlValue] = {
            "id": "MOD-inline-root-example",
            "category": "modules",
            "title": "inline root example",
            "code": "fn main() -> Unit {}\n",
            "file_set_root": ".",
            "expect": {"value": {"kind": "Ok", "repr": '"ok"'}},
        }

        _, problems = validate_test_case(fixture, "core/modules.yaml", 0)

        self.assertTrue(
            any(
                "file_set_root is invalid with code" in problem.message
                for problem in problems
            )
        )

    def test_rejects_null_and_container_modes_without_crashing(self) -> None:
        for mode in (None, [], {}):
            with self.subTest(mode=mode):
                fixture: dict[str, YamlValue] = {
                    "id": "MOD-invalid-mode-example",
                    "category": "modules",
                    "title": "invalid mode example",
                    "mode": mode,
                    "files": {
                        "app/main.eglur": "fn main() -> Unit {}\n",
                        "app/value.eglur": "fn value() -> i64 = 1\n",
                    },
                    "entry": "app/main.eglur",
                    "expect": {"value": {"kind": "Ok", "repr": '"ok"'}},
                }

                _, problems = validate_test_case(
                    fixture,
                    "core/modules.yaml",
                    0,
                )

                self.assertTrue(
                    any(
                        ".mode is unsupported" in problem.message
                        for problem in problems
                    )
                )

    def test_rejects_non_source_entry(self) -> None:
        fixture: dict[str, YamlValue] = {
            "id": "MOD-non-source-entry-example",
            "category": "modules",
            "title": "non-source entry example",
            "files": {
                "app/main.eglur": "fn main() -> Unit {}\n",
                "README.md": "fixture notes\n",
            },
            "entry": "README.md",
            "expect": {"value": {"kind": "Ok", "repr": '"ok"'}},
        }

        _, problems = validate_test_case(fixture, "core/modules.yaml", 0)

        self.assertTrue(
            any(
                ".entry must designate an .eglur source" in problem.message
                for problem in problems
            )
        )

    def test_package_mode_does_not_require_canonical_source_paths(self) -> None:
        problems = self.validate(
            {
                "Eglur.toml": '[package]\nname = "fixture"\n',
                "src/./main.eglur": "fn main() -> Unit {}\n",
            }
        )

        self.assertEqual(problems, [])

    def test_package_entry_must_be_under_existing_source_root(self) -> None:
        fixture: dict[str, YamlValue] = {
            "id": "PRG-package-entry-outside-src",
            "category": "program",
            "title": "package entry outside src",
            "mode": "package",
            "files": {
                "Eglur.toml": '[package]\nname = "fixture"\n',
                "src/library.eglur": "fn value() -> i64 = 1\n",
                "main.eglur": "fn main!() -> Unit = ()\n",
            },
            "entry": "main.eglur",
            "expect": {"value": {"kind": "Unit"}, "process_exit": 0},
        }

        _, problems = validate_test_case(fixture, "core/program.yaml", 0)

        self.assertTrue(
            any(
                ".entry must be under src/ in package mode" in problem.message
                for problem in problems
            )
        )

    def test_missing_package_source_root_preserves_compile_diagnostic(self) -> None:
        fixture: dict[str, YamlValue] = {
            "id": "MOD-package-mode-without-src-root",
            "category": "modules",
            "title": "package mode without src root",
            "mode": "package",
            "files": {
                "Eglur.toml": '[package]\nname = "fixture"\n',
                "main.eglur": "fn main!() -> Unit = ()\n",
            },
            "entry": "main.eglur",
            "expect": {
                "compile_error": {
                    "code": "PACKAGE_SRC_ROOT_MISSING",
                    "message_contains": ["package", "src", "missing"],
                    "data_keys": [],
                }
            },
        }

        _, problems = validate_test_case(fixture, "core/modules.yaml", 0)

        self.assertEqual(problems, [])


class RegistryAcquisitionValidationTests(unittest.TestCase):
    """Validate provider scripts without taking over compiler-owned accounting."""

    def validate(self, acquisitions: YamlValue) -> list[str]:
        """Return schema problems for transactional registry provider input."""
        fixture: dict[str, YamlValue] = {
            "id": "MOD-registry-acquisition-example",
            "category": "modules",
            "title": "registry acquisition example",
            "mode": "package",
            "files": {
                "Eglur.toml": '[package]\nname = "app"\nversion = "0.1.0"\n',
                "src/main.eglur": "fn main!() -> Unit = ()\n",
            },
            "registry_acquisitions": acquisitions,
            "entry": "src/main.eglur",
            "expect": {"value": {"kind": "Unit"}, "process_exit": 0},
        }

        _, problems = validate_test_case(fixture, "core/modules.yaml", 0)
        return [problem.message for problem in problems]

    def test_accepts_tree_domain_failures_for_compiler_validation(self) -> None:
        entries: list[dict[str, YamlValue]] = [
            {"kind": "file", "path": "src/Σ.eglur", "text": "upper"},
            {"kind": "file", "path": "src/ς.eglur", "text": "final"},
            {"kind": "file", "path": "src/dup.eglur", "text": "text"},
            {
                "kind": "file",
                "path": "src/dup.eglur",
                "content_hex": "00",
            },
            {"kind": "file", "path": "src/ancestor", "text": "parent"},
            {
                "kind": "file",
                "path": "src/ancestor/child",
                "content_hex": "01",
            },
            {"kind": "file", "path_hex": "ff", "text": "raw"},
            {
                "kind": "hardlink",
                "path": "src/link",
                "target_hex": "ff",
            },
            {"kind": "socket", "path": "src/socket"},
        ]
        problems = self.validate(
            [
                {
                    "package": "packages.example.com/acme/math",
                    "version": "1.2.3",
                    "provider_steps": [
                        *({"emit": entry} for entry in entries),
                        {"finish": "complete"},
                    ],
                }
            ]
        )

        self.assertEqual(problems, [])

    def test_rejects_non_scalar_candidate_paths(self) -> None:
        problems = self.validate(
            [
                {
                    "package": "packages.example.com/acme/math",
                    "version": "1.2.3",
                    "provider_steps": [
                        {
                            "emit": {
                                "kind": "file",
                                "path": "src/\ud800.eglur",
                                "text": "invalid",
                            }
                        },
                        {"finish": "complete"},
                    ],
                }
            ]
        )

        self.assertTrue(
            any(".path must be Unicode scalar text" in problem for problem in problems)
        )

    def test_rejects_noncanonical_registry_candidate_version(self) -> None:
        problems = self.validate(
            [
                {
                    "package": "packages.example.com/acme/math",
                    "version": "1.02.3",
                    "provider_steps": [
                        {
                            "emit": {
                                "kind": "file",
                                "path": "src/lib.eglur",
                                "text": "module lib\n",
                            }
                        },
                        {"finish": "complete"},
                    ],
                }
            ]
        )

        self.assertTrue(
            any("version must be an exact version" in problem for problem in problems)
        )

    def test_accepts_closed_provider_scripts_and_compact_repetition(self) -> None:
        problems = self.validate(
            [
                {
                    "package": "packages.example.com/acme/missing",
                    "version": "1.0.0",
                    "provider_steps": [{"finish": "missing_release"}],
                },
                {
                    "package": "packages.example.com/acme/failed",
                    "version": "1.0.0",
                    "provider_steps": [
                        {
                            "emit": {
                                "kind": "file",
                                "path": "partial",
                                "text": "data",
                            }
                        },
                        {"finish": "transport_failure"},
                    ],
                },
                {
                    "package": "packages.example.com/acme/large",
                    "version": "1.0.0",
                    "provider_steps": [
                        {
                            "repeat": {
                                "count": 65_537,
                                "emit": {
                                    "kind": "file",
                                    "path_repeat": {
                                        "byte_hex": "61",
                                        "count": 4_097,
                                    },
                                    "content_repeat": {
                                        "byte_hex": "00",
                                        "count": 134_217_729,
                                    },
                                },
                            }
                        },
                        {"finish": "complete"},
                    ],
                },
            ]
        )

        self.assertEqual(problems, [])

    def test_rejects_emissions_before_missing_release(self) -> None:
        problems = self.validate(
            [
                {
                    "package": "packages.example.com/acme/failed",
                    "version": "1.0.0",
                    "provider_steps": [
                        {
                            "emit": {
                                "kind": "file",
                                "path": "partial",
                                "text": "data",
                            }
                        },
                        {"finish": "missing_release"},
                    ],
                }
            ]
        )

        self.assertTrue(any("must contain only" in item for item in problems))

    def test_rejects_finish_before_the_final_step(self) -> None:
        problems = self.validate(
            [
                {
                    "package": "packages.example.com/acme/failed",
                    "version": "1.0.0",
                    "provider_steps": [
                        {"finish": "transport_failure"},
                        {
                            "emit": {
                                "kind": "file",
                                "path": "late",
                                "text": "data",
                            }
                        },
                    ],
                }
            ]
        )

        self.assertTrue(any("cannot finish before" in item for item in problems))
        self.assertTrue(
            any("must be the final finish step" in item for item in problems)
        )

    def test_rejects_duplicate_provider_response_coordinates(self) -> None:
        problems = self.validate(
            [
                {
                    "package": "packages.example.com/acme/repeated",
                    "version": "1.0.0",
                    "provider_steps": [{"finish": "missing_release"}],
                },
                {
                    "package": "packages.example.com/acme/repeated",
                    "version": "1.0.0",
                    "provider_steps": [{"finish": "transport_failure"}],
                },
            ]
        )

        self.assertTrue(
            any("duplicates registry coordinate" in item for item in problems)
        )

    def test_rejects_predeclared_limit_outcome(self) -> None:
        problems = self.validate(
            [
                {
                    "package": "packages.example.com/acme/large",
                    "version": "1.0.0",
                    "outcome": "limit_exceeded",
                    "resource": "entry_count",
                    "limit": 1,
                    "observed_at_least": 2,
                }
            ]
        )

        self.assertTrue(any("unsupported=['limit'" in problem for problem in problems))
        self.assertTrue(
            any("missing=['provider_steps']" in problem for problem in problems)
        )

    def test_rejects_invalid_repeat_counts_and_bytes(self) -> None:
        problems = self.validate(
            [
                {
                    "package": "packages.example.com/acme/large",
                    "version": "1.0.0",
                    "provider_steps": [
                        {
                            "repeat": {
                                "count": 0,
                                "emit": {
                                    "kind": "directory",
                                    "path_repeat": {
                                        "byte_hex": "GG",
                                        "count": 0,
                                    },
                                },
                            }
                        },
                        {"finish": "complete"},
                    ],
                }
            ]
        )

        self.assertTrue(
            any("repeat.count must be a positive i64" in p for p in problems)
        )
        self.assertTrue(any("byte_hex must be exactly one" in p for p in problems))
        self.assertTrue(
            any("path_repeat.count must be a positive i64" in p for p in problems)
        )


class SymlinkTopologyValidationTests(unittest.TestCase):
    """Exercise symbolic-link topology without resolving compiler test states."""

    def validate(
        self,
        symlinks: YamlValue,
        *,
        extra_files: dict[str, YamlValue] | None = None,
    ) -> list[str]:
        """Return schema problems for one file-set topology."""
        files: dict[str, YamlValue] = {"src/main.eglur": "fn main() -> Unit = ()\n"}
        if extra_files is not None:
            files.update(extra_files)
        fixture: dict[str, YamlValue] = {
            "id": "MOD-symlink-topology-example",
            "category": "modules",
            "title": "symlink topology example",
            "files": files,
            "symlinks": symlinks,
            "entry": "src/main.eglur",
            "expect": {"value": {"kind": "Ok", "repr": '"ok"'}},
        }

        _, problems = validate_test_case(fixture, "core/modules.yaml", 0)
        return [problem.message for problem in problems]

    def test_accepts_alias_broken_target_and_link_cycle(self) -> None:
        problems = self.validate(
            {
                "deps/alias": "../real",
                "broken": "missing",
                "loops/a": "b",
                "loops/b": "a",
            }
        )

        self.assertEqual(problems, [])

    def test_rejects_symlinks_outside_files_source_form(self) -> None:
        for source in (
            {"code": "fn main() -> Unit = ()\n"},
            {"code_bytes_hex": "00"},
        ):
            with self.subTest(source=source):
                fixture: dict[str, YamlValue] = {
                    "id": "MOD-invalid-symlink-source-form",
                    "category": "modules",
                    "title": "invalid symlink source form",
                    "symlinks": {"alias": "target"},
                    "expect": {"value": {"kind": "Ok", "repr": '"ok"'}},
                    **source,
                }

                _, problems = validate_test_case(
                    fixture,
                    "core/modules.yaml",
                    0,
                )

                self.assertTrue(
                    any(
                        ".symlinks is invalid with" in problem.message
                        for problem in problems
                    )
                )

    def test_requires_non_empty_string_mapping(self) -> None:
        cases: tuple[tuple[YamlValue, str], ...] = (
            ([], "must be a mapping"),
            ({}, "must be non-empty"),
            ({1: "target"}, "must use string keys"),
            ({"alias": 1}, "must be a string"),
        )
        for symlinks, expected in cases:
            with self.subTest(symlinks=symlinks):
                problems = self.validate(symlinks)
                self.assertTrue(any(expected in problem for problem in problems))

    def test_rejects_noncanonical_link_paths(self) -> None:
        for link_path in (
            "",
            "/alias",
            "../alias",
            "./alias",
            "deps//alias",
            "deps/alias/",
            r"deps\alias",
        ):
            with self.subTest(link_path=link_path):
                problems = self.validate({link_path: "target"})
                self.assertTrue(
                    any("symlinks has invalid path" in problem for problem in problems)
                )

    def test_rejects_invalid_or_escaping_targets(self) -> None:
        for target in (
            "",
            "/target",
            r"..\target",
            "target//child",
            "target/",
            "../../escape",
        ):
            with self.subTest(target=target):
                problems = self.validate({"deps/alias": target})
                self.assertTrue(
                    any(
                        "must be a non-empty relative POSIX target" in problem
                        for problem in problems
                    )
                )

    def test_rejects_file_conflicts_and_nested_links(self) -> None:
        cases: tuple[tuple[dict[str, YamlValue], dict[str, YamlValue], str], ...] = (
            (
                {"deps/alias": "target"},
                {"deps/./alias": "regular file"},
                "conflicts with regular file",
            ),
            (
                {"deps/alias": "target"},
                {"deps": "regular file"},
                "is below regular file",
            ),
            (
                {"deps": "target"},
                {"deps/child.txt": "regular file"},
                "is below symlink",
            ),
            (
                {"deps": "target", "deps/alias": "target"},
                {},
                "must not be nested",
            ),
        )
        for symlinks, extra_files, expected in cases:
            with self.subTest(symlinks=symlinks, extra_files=extra_files):
                problems = self.validate(symlinks, extra_files=extra_files)
                self.assertTrue(any(expected in problem for problem in problems))


class ProcessExitValidationTests(unittest.TestCase):
    """Exercise process-status assertions against runner outcome channels."""

    def validate(
        self,
        expect: dict[str, YamlValue],
        *,
        runner: str = "main",
    ) -> list[str]:
        """Return validation messages for one complete fixture."""
        fixture: dict[str, YamlValue] = {
            "id": "PRG-process-exit-example",
            "category": "program",
            "title": "process exit example",
            "runner": runner,
            "code": "fn main!() -> Unit = ()\n",
            "expect": expect,
        }
        _, problems = validate_test_case(fixture, "core/program.yaml", 0)
        return [problem.message for problem in problems]

    def test_accepts_unit_without_textual_snapshot(self) -> None:
        problems = self.validate(
            {
                "value": {"kind": "Unit"},
                "process_exit": 0,
            }
        )

        self.assertEqual(problems, [])

    def test_accepts_res_and_runtime_projections(self) -> None:
        cases: tuple[dict[str, YamlValue], ...] = (
            {"value": {"kind": "Ok", "repr": '"ok"'}, "process_exit": 0},
            {"value": {"kind": "Err", "repr": '"boom"'}, "process_exit": 1},
            {
                "runtime_error": {
                    "code": "RUNTIME_ABORT",
                    "message_contains": ["abort"],
                    "data_keys": [],
                },
                "process_exit": 1,
            },
        )
        for expect in cases:
            with self.subTest(expect=expect):
                self.assertEqual(self.validate(expect), [])

    def test_rejects_non_integer_or_out_of_range_status(self) -> None:
        for process_exit in (False, 2, "0"):
            with self.subTest(process_exit=process_exit):
                problems = self.validate(
                    {
                        "value": {"kind": "Unit"},
                        "process_exit": process_exit,
                    }
                )

                self.assertTrue(
                    any(
                        "process_exit must be integer 0 or 1" in problem
                        for problem in problems
                    )
                )

    def test_rejects_status_for_compile_and_test_runner_outcomes(self) -> None:
        compile_problems = self.validate(
            {
                "compile_error": {
                    "code": "PROGRAM_ENTRY_MISSING",
                    "message_contains": ["program", "entry", "missing"],
                    "data_keys": [],
                },
                "process_exit": 1,
            }
        )
        test_problems = self.validate(
            {
                "test_run": {
                    "order": [],
                    "passed": [],
                    "skipped": [],
                    "failed": [],
                },
                "process_exit": 0,
            },
            runner="test",
        )

        self.assertTrue(
            any(
                "process_exit is invalid with" in problem
                for problem in compile_problems
            )
        )
        self.assertTrue(
            any(
                "process_exit is only valid for runner main" in problem
                for problem in test_problems
            )
        )

    def test_rejects_status_mismatched_with_observed_outcome(self) -> None:
        cases: tuple[tuple[dict[str, YamlValue], str], ...] = (
            (
                {"value": {"kind": "Ok", "repr": '"ok"'}, "process_exit": 1},
                "must be 0 with value kind Ok",
            ),
            (
                {"value": {"kind": "Err", "repr": '"boom"'}, "process_exit": 0},
                "must be 1 with value kind Err",
            ),
            (
                {
                    "runtime_error": {
                        "code": "RUNTIME_ABORT",
                        "message_contains": ["abort"],
                        "data_keys": [],
                    },
                    "process_exit": 0,
                },
                "must be 1 with runtime_error",
            ),
        )
        for expect, expected_message in cases:
            with self.subTest(expect=expect):
                problems = self.validate(expect)
                self.assertTrue(
                    any(expected_message in problem for problem in problems)
                )

    def test_rejects_status_for_non_entrypoint_value_kind(self) -> None:
        problems = self.validate(
            {
                "value": {"kind": "Some", "repr": "1"},
                "process_exit": 0,
            }
        )

        self.assertTrue(
            any(
                "process_exit requires value kind Unit, Ok, or Err" in problem
                for problem in problems
            )
        )

    def test_unit_without_status_still_requires_value_assertion(self) -> None:
        problems = self.validate({"value": {"kind": "Unit"}})

        self.assertTrue(any("value must include" in problem for problem in problems))


class PrimarySpanValidationTests(unittest.TestCase):
    """Exercise marker and exact-byte assertions for compile diagnostics."""

    def validate(self, test: dict[str, YamlValue]) -> list[str]:
        """Return schema problems for one complete fixture."""
        _, problems = validate_test_case(test, "core/example.yaml", 0)
        return [problem.message for problem in problems]

    def fixture(
        self,
        code: str,
        marker: str | None,
    ) -> dict[str, YamlValue]:
        """Build a minimum compile-error fixture with an optional marker assertion."""
        compile_error: dict[str, YamlValue] = {
            "code": "OWNERSHIP_DOUBLE_MOVE",
            "message_contains": ["ownership", "move", "already consumed"],
            "data_keys": [],
        }
        if marker is not None:
            compile_error["primary_span"] = {"marker": marker}
        return {
            "id": "OWN-primary-marker-example",
            "category": "ownership",
            "title": "primary marker example",
            "code": code,
            "expect": {"compile_error": compile_error},
        }

    def test_accepts_unique_referenced_marker(self) -> None:
        test = self.fixture(
            "fn bad(x: Own[i64]) -> i64 {\n"
            "  let y = move x // @primary second-move\n"
            "  return 0\n"
            "}\n",
            "second-move",
        )

        self.assertEqual(self.validate(test), [])

    def test_accepts_marker_in_crlf_source(self) -> None:
        test = self.fixture(
            "fn bad(x: Own[i64]) -> i64 {\r\n"
            "  let y = move x // @primary second-move\r\n"
            "  return 0\r\n"
            "}\r\n",
            "second-move",
        )

        self.assertEqual(self.validate(test), [])

    def test_accepts_marker_after_lone_cr_source_line(self) -> None:
        test = self.fixture(
            "fn bad(x: Own[i64]) -> i64 {\r"
            "  let y = move x // @primary second-move\r"
            "  return 0\r"
            "}\r",
            "second-move",
        )

        self.assertEqual(self.validate(test), [])

    def test_accepts_exact_byte_span_for_raw_source(self) -> None:
        test = self.fixture("", None)
        test.pop("code")
        test["code_bytes_hex"] = "ff"
        compile_error = cast(
            dict[str, YamlValue],
            cast(dict[str, YamlValue], test["expect"])["compile_error"],
        )
        compile_error["primary_span"] = {
            "source": "code",
            "bytes": {"start": 0, "end": 1},
        }

        self.assertEqual(self.validate(test), [])

    def test_rejects_mixed_marker_and_byte_span_forms(self) -> None:
        test = self.fixture("bad() // @primary bad-site\n", "bad-site")
        compile_error = cast(
            dict[str, YamlValue],
            cast(dict[str, YamlValue], test["expect"])["compile_error"],
        )
        compile_error["primary_span"] = {
            "marker": "bad-site",
            "source": "code",
            "bytes": {"start": 0, "end": 1},
        }

        problems = self.validate(test)

        self.assertTrue(
            any(
                "must contain exactly marker or source and bytes" in problem
                for problem in problems
            )
        )

    def test_rejects_reversed_or_out_of_bounds_byte_span(self) -> None:
        for byte_range, expected in (
            ({"start": 1, "end": 0}, "must not precede start"),
            ({"start": 0, "end": 2}, "beyond source length 1"),
        ):
            with self.subTest(byte_range=byte_range):
                test = self.fixture("", None)
                test.pop("code")
                test["code_bytes_hex"] = "ff"
                compile_error = cast(
                    dict[str, YamlValue],
                    cast(dict[str, YamlValue], test["expect"])["compile_error"],
                )
                compile_error["primary_span"] = {
                    "source": "code",
                    "bytes": byte_range,
                }

                self.assertTrue(
                    any(expected in problem for problem in self.validate(test))
                )

    def test_rejects_byte_span_for_unknown_source_unit(self) -> None:
        test = self.fixture("", None)
        test.pop("code")
        test["code_bytes_hex"] = "ff"
        compile_error = cast(
            dict[str, YamlValue],
            cast(dict[str, YamlValue], test["expect"])["compile_error"],
        )
        compile_error["primary_span"] = {
            "source": "other.eglur",
            "bytes": {"start": 0, "end": 1},
        }

        problems = self.validate(test)

        self.assertTrue(
            any("does not identify a fixture source unit" in p for p in problems)
        )

    def test_rejects_non_scalar_source_before_byte_bound_validation(self) -> None:
        test = self.fixture("\ud800", None)
        compile_error = cast(
            dict[str, YamlValue],
            cast(dict[str, YamlValue], test["expect"])["compile_error"],
        )
        compile_error["primary_span"] = {
            "source": "code",
            "bytes": {"start": 0, "end": 1},
        }

        problems = self.validate(test)

        self.assertEqual(
            problems,
            ["core/example.yaml.tests[0].code must be Unicode scalar text"],
        )

    def test_rejects_non_string_byte_range_keys_without_raising(self) -> None:
        test = self.fixture("bad()\n", None)
        compile_error = cast(
            dict[str, YamlValue],
            cast(dict[str, YamlValue], test["expect"])["compile_error"],
        )
        compile_error["primary_span"] = cast(
            YamlValue,
            {"source": "code", "bytes": {0: 0, "end": 1}},
        )

        problems = self.validate(test)

        self.assertEqual(len(problems), 1)
        self.assertIn("bytes must be a string-keyed mapping", problems[0])

    def test_rejects_missing_marker_declaration(self) -> None:
        problems = self.validate(self.fixture("fn main() -> Unit {}\n", "missing"))

        self.assertEqual(len(problems), 1)
        self.assertIn("missing is not declared in code or files", problems[0])

    def test_rejects_duplicate_marker_declarations(self) -> None:
        test = self.fixture(
            "first() // @primary duplicate\nsecond() // @primary duplicate\n",
            "duplicate",
        )

        problems = self.validate(test)

        self.assertEqual(len(problems), 1)
        self.assertIn(
            "primary marker duplicate is declared more than once", problems[0]
        )

    def test_rejects_duplicate_marker_across_source_files(self) -> None:
        test = self.fixture("", "shared-marker")
        test.pop("code")
        test["files"] = {
            "first.eglur": "first() // @primary shared-marker\n",
            "second.eglur": "second() // @primary shared-marker\n",
        }
        test["file_set_root"] = "."
        test["entry"] = "first.eglur"

        problems = self.validate(test)

        self.assertEqual(len(problems), 1)
        self.assertIn(
            "first.eglur:1, second.eglur:1",
            problems[0],
        )

    def test_rejects_unreferenced_source_marker(self) -> None:
        problems = self.validate(self.fixture("bad() // @primary stale-marker\n", None))

        self.assertEqual(len(problems), 1)
        self.assertIn(
            "source primary marker stale-marker is not referenced", problems[0]
        )

    def test_rejects_malformed_source_marker(self) -> None:
        problems = self.validate(
            self.fixture("bad() // @primary Bad_Name\n", "bad-name")
        )

        self.assertEqual(len(problems), 2)
        self.assertTrue(
            any("malformed primary marker at code:1" in problem for problem in problems)
        )
        self.assertTrue(
            any("bad-name is not declared" in problem for problem in problems)
        )

    def test_ignores_marker_text_outside_line_comments(self) -> None:
        test = self.fixture(
            'let text = "// @primary string-marker"\n'
            "/* // @primary block-marker */\n"
            "bad() // @primary actual-marker\n",
            "actual-marker",
        )

        self.assertEqual(self.validate(test), [])

    def test_rejects_primary_span_on_non_compile_diagnostic(self) -> None:
        diagnostic: YamlValue = {
            "code": "RUNTIME_ABORT",
            "message_contains": [],
            "data_keys": [],
            "primary_span": {"marker": "runtime-site"},
        }

        problems = validate_diagnostic_assertion(diagnostic, "runtime_error")

        self.assertEqual(len(problems), 1)
        self.assertIn("primary_span is not a supported key", problems[0].message)


if __name__ == "__main__":
    unittest.main()
