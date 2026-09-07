"""Tests for metadata-bearing Markdown visibility."""

import tempfile
import unittest
from pathlib import Path

from eglur_conformance import markdown_document


class MarkdownSourceTests(unittest.TestCase):
    """Keep hidden or indented examples out of machine metadata."""

    def test_preserves_fence_character_and_minimum_length(self) -> None:
        source = """\
visible one
~~~~
hidden one
~~~
hidden two
~~~~~
visible two
````
hidden three
``` not a close
hidden four
````
visible three

    indented code
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source.md"
            path.write_text(source, encoding="utf-8")

            visible = list(markdown_document.visible_markdown_lines(path))

        self.assertEqual(
            visible,
            [
                (1, "visible one"),
                (7, "visible two"),
                (13, "visible three"),
                (14, ""),
            ],
        )

    def test_preserves_list_prose_and_hides_nested_code(self) -> None:
        source = """\
1. outer item
   1. nested item continues onto
      [visible link](target.md)

          [indented code](missing.md)
      ```markdown
      [fenced code](missing.md)
      ```
2. next item
"""
        visible = list(
            markdown_document.visible_markdown_lines_from_text(
                source,
                "source.md",
            )
        )

        self.assertEqual(
            visible,
            [
                (1, "1. outer item"),
                (2, "   1. nested item continues onto"),
                (3, "      [visible link](target.md)"),
                (4, ""),
                (9, "2. next item"),
            ],
        )

    def test_comment_regions_cannot_open_fences(self) -> None:
        source = """\
<!--
```
hidden
```
-->
visible
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source.md"
            path.write_text(source, encoding="utf-8")

            visible = list(markdown_document.visible_markdown_lines(path))

        self.assertEqual(visible, [(6, "visible")])

    def test_comment_tokens_inside_matched_code_spans_remain_visible(self) -> None:
        source = """\
literal `<!--` token
literal ``embedded `<!--` token`` tail
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source.md"
            path.write_text(source, encoding="utf-8")

            visible = list(markdown_document.visible_markdown_lines(path))

        self.assertEqual(
            visible,
            [
                (1, "literal `<!--` token"),
                (2, "literal ``embedded `<!--` token`` tail"),
            ],
        )

    def test_unmatched_code_delimiter_does_not_hide_a_real_comment(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source.md"
            path.write_text("literal ` then <!--\n", encoding="utf-8")

            with self.assertRaisesRegex(ValueError, "unterminated HTML comment"):
                list(markdown_document.visible_markdown_lines(path))

    def test_comment_token_in_fence_info_is_literal(self) -> None:
        source = """\
```example <!-- literal
hidden
```
visible
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source.md"
            path.write_text(source, encoding="utf-8")

            visible = list(markdown_document.visible_markdown_lines(path))

        self.assertEqual(visible, [(4, "visible")])

    def test_extracts_visible_fenced_code_with_source_metadata(self) -> None:
        source = """\
<!--
```eglur fixture=HIDDEN
hidden
```
-->
~~~eglur fixture=VISIBLE
first

second
~~~~
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source.md"
            path.write_text(source, encoding="utf-8")

            blocks = list(markdown_document.fenced_code_blocks(path))

        self.assertEqual(
            blocks,
            [
                markdown_document.FencedCodeBlock(
                    opening_line=6,
                    info="eglur fixture=VISIBLE",
                    code="first\n\nsecond\n",
                )
            ],
        )


if __name__ == "__main__":
    unittest.main()
