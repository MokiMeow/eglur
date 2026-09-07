"""Semantic tests for repository-local Markdown links."""

import unittest
from pathlib import PurePosixPath
from urllib.parse import quote

from eglur_conformance.documentation import links, model


def source(path: str, text: str) -> model.MarkdownSource:
    """Build one canonical in-memory Markdown source."""
    return model.MarkdownSource(path=PurePosixPath(path), text=text)


class DocumentationLinkTests(unittest.TestCase):
    """Validate links against exact tracked paths and heading identities."""

    def test_accepts_files_directories_queries_and_heading_fragments(self) -> None:
        sources = (
            source("README.md", "# Root\n"),
            source(
                "docs/source.md",
                """\
# Source

[root](../README.md)
[target](target.md?view=1#target-heading)
[space](space%20name.md)
[directory](guide/)
[same](#source)
[duplicate](target.md#repeat-1)
[empty]()
""",
            ),
            source(
                "docs/target.md",
                "# Target Heading\n\n## Repeat\n\n## Repeat\n",
            ),
            source("docs/space name.md", "# Encoded\n"),
        )
        targets = frozenset(
            {
                PurePosixPath("."),
                PurePosixPath("README.md"),
                PurePosixPath("docs"),
                PurePosixPath("docs/source.md"),
                PurePosixPath("docs/target.md"),
                PurePosixPath("docs/space name.md"),
                PurePosixPath("docs/guide"),
            }
        )

        self.assertEqual(links.find_violations(sources, targets), [])

    def test_rejects_missing_escaping_absolute_case_and_fragment_targets(self) -> None:
        sources = (
            source(
                "docs/source.md",
                """\
[missing](missing.md)
[escape](../../outside.md)
[encoded escape](%2e%2e/%2e%2e/outside.md)
[absolute](/README.md)
[encoded absolute](%2FREADME.md)
[triple-slash absolute](///README.md)
[case](Target.md)
[fragment](target.md#missing)
[non-Markdown fragment](data.yaml#part)
""",
            ),
            source("docs/target.md", "# Present\n"),
        )
        targets = frozenset(
            {
                PurePosixPath("docs"),
                PurePosixPath("docs/source.md"),
                PurePosixPath("docs/target.md"),
                PurePosixPath("docs/data.yaml"),
                PurePosixPath("README.md"),
            }
        )

        violations = links.find_violations(sources, targets)

        self.assertEqual(len(violations), 9)
        rendered = [violation.render() for violation in violations]
        self.assertTrue(any("target is not tracked" in item for item in rendered))
        self.assertEqual(
            sum("escapes repository root" in item for item in rendered),
            5,
        )
        self.assertTrue(any("fragment does not exist" in item for item in rendered))
        self.assertTrue(
            any("fragment requires a Markdown target" in item for item in rendered)
        )

    def test_ignores_hidden_content_and_external_urls(self) -> None:
        sources = (
            source(
                "source.md",
                """\
`[inline](missing.md)`
```markdown
[fenced](missing.md)
```
    [indented](missing.md)
<!-- [commented](missing.md) -->
[web](https://example.com/missing)
[mail](mailto:docs@example.com)
[cdn](//cdn.example.com/image.png)
""",
            ),
        )

        self.assertEqual(
            links.find_violations(sources, frozenset({PurePosixPath("source.md")})),
            [],
        )

    def test_validates_wrapped_links_inside_nested_lists(self) -> None:
        sources = (
            source(
                "docs/source.md",
                """\
1. outer item
   1. nested item continues onto
      [missing](missing.md)

          [indented code](ignored.md)
""",
            ),
        )

        violations = links.find_violations(
            sources,
            frozenset({PurePosixPath("docs/source.md")}),
        )

        self.assertEqual(len(violations), 1)
        self.assertEqual(violations[0].line, 3)
        self.assertEqual(violations[0].destination, "missing.md")

    def test_preserves_columns_after_inline_html_comments(self) -> None:
        line = "<!-- hidden --> [visible](missing.md)"
        sources = (source("source.md", f"{line}\n"),)

        violations = links.find_violations(
            sources,
            frozenset({PurePosixPath("source.md")}),
        )

        self.assertEqual(len(violations), 1)
        self.assertEqual(violations[0].column, line.index("missing.md") + 1)

    def test_resolves_reference_links_and_rejects_undefined_references(self) -> None:
        sources = (
            source(
                "docs/source.md",
                """\
[Guide][guide]
[Collapsed][]
[Shortcut]
[Undefined][missing]

[guide]: target.md#target
[Collapsed]: image.png
[Shortcut]: <space%20name.md>
[unused]: missing.md
""",
            ),
            source("docs/target.md", "# Target\n"),
            source("docs/space name.md", "# Space\n"),
        )
        targets = frozenset(
            {
                PurePosixPath("docs"),
                PurePosixPath("docs/source.md"),
                PurePosixPath("docs/target.md"),
                PurePosixPath("docs/image.png"),
                PurePosixPath("docs/space name.md"),
            }
        )

        violations = links.find_violations(sources, targets)

        self.assertEqual(len(violations), 1)
        self.assertIn("reference is undefined", violations[0].render())

    def test_orders_source_locations_deterministically(self) -> None:
        sources = (
            source("z.md", "[later](missing.md)\n"),
            source("a.md", "\n[second](missing.md)\n[first](absent.md)\n"),
        )
        targets = frozenset({PurePosixPath("a.md"), PurePosixPath("z.md")})

        violations = links.find_violations(sources, targets)

        self.assertEqual(
            [(item.path.as_posix(), item.line) for item in violations],
            [("a.md", 2), ("a.md", 3), ("z.md", 1)],
        )
        self.assertRegex(violations[0].render(), r"^a\.md:2:\d+: ")

    def test_edge_round_one_destination_and_anchor_composition(self) -> None:
        unicode_fragment = quote("데이터-흐름")
        sources = (
            source(
                "docs/source.md",
                f"""\
[nested](guide_(v1).md)
[title](guide_(v1).md "Guide")
[angle](<space name.md> "Space")
[bom](bom.md#bom-heading)
[unicode](unicode.md#{unicode_fragment})
[crlf](crlf.md#crlf-heading)
""",
            ),
            source("docs/guide_(v1).md", "# Guide\n"),
            source("docs/space name.md", "# Space\n"),
            source("docs/bom.md", "\ufeff# BOM Heading\n"),
            source("docs/unicode.md", "# 데이터 흐름\n"),
            source("docs/crlf.md", "# CRLF Heading\r\n"),
        )
        targets = frozenset(
            {
                PurePosixPath("docs"),
                *(item.path for item in sources),
            }
        )

        self.assertEqual(links.find_violations(sources, targets), [])

    def test_edge_round_two_reference_encoding_and_malformed_regions(self) -> None:
        reference_sources = (
            source(
                "source.md",
                """\
[case][Mixed Label]
[collapsed][]
[Shortcut]
[external][web]
``[hidden](missing.md)``

[mixed   label]: target.md
[collapsed]: target.md
[Shortcut]: target.md
[web]: git+https://example.com/docs
""",
            ),
            source("target.md", "# Target\n"),
        )
        targets = frozenset(item.path for item in reference_sources)
        self.assertEqual(
            links.find_violations(reference_sources, targets),
            [],
        )

        invalid_sources = (
            source(
                "invalid.md",
                "[backslash](%5Coutside.md)\n[nul](target.md%00)\n",
            ),
        )
        invalid = links.find_violations(
            invalid_sources,
            frozenset(
                {
                    PurePosixPath("invalid.md"),
                    PurePosixPath("target.md"),
                }
            ),
        )
        self.assertEqual(len(invalid), 2)
        self.assertTrue(
            all("not a canonical POSIX path" in item.reason for item in invalid)
        )

        with self.assertRaisesRegex(ValueError, "unterminated Markdown code fence"):
            links.find_violations(
                (source("broken.md", "```markdown\n[hidden](missing.md)\n"),),
                frozenset({PurePosixPath("broken.md")}),
            )

    def test_edge_round_three_multi_link_comment_and_unicode_paths(self) -> None:
        encoded_path = quote("데이터.md")
        source_text = f"""\
[one](../one.md) [two](../two.md)
``[hidden](../missing.md)``
<!-- hidden
still hidden --> [visible](../one.md)
[normalized](./child/../../two.md)
[unicode](../{encoded_path})
[external](data:text/plain,missing)
"""
        sources = (
            source("nested/source.md", source_text),
            source("one.md", "# One\n"),
            source("two.md", "# Two\n"),
            source("데이터.md", "# Data\n"),
        )
        targets = frozenset(
            {
                PurePosixPath("nested"),
                *(item.path for item in sources),
            }
        )

        self.assertEqual(links.find_violations(sources, targets), [])


if __name__ == "__main__":
    unittest.main()
