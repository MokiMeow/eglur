"""Load authoritative YAML while rejecting duplicate mapping keys."""

from collections.abc import Hashable
from datetime import date, datetime
from pathlib import Path
from typing import TypeAlias, cast

import yaml
from yaml.constructor import ConstructorError, SafeConstructor
from yaml.nodes import MappingNode

YamlScalar: TypeAlias = str | int | float | bool | date | datetime | None
# The explicit string-key arm preserves schema-narrowed mutable mappings.
YamlValue: TypeAlias = (
    YamlScalar
    | list["YamlValue"]
    | dict[str, "YamlValue"]
    | dict[YamlScalar, "YamlValue"]
)
SafeLoaderBase = cast(
    type[SafeConstructor],
    getattr(yaml, "CSafeLoader", yaml.SafeLoader),
)


class StrictSafeLoader(SafeLoaderBase):
    """Safe YAML loader that treats duplicate keys as malformed evidence."""

    def construct_mapping(
        self,
        node: MappingNode,
        deep: bool = False,
    ) -> dict[Hashable, object]:
        """Construct one mapping and reject duplicate keys at their source."""
        self.flatten_mapping(node)
        mapping: dict[Hashable, object] = {}
        for key_node, value_node in node.value:
            key = self.construct_object(key_node, deep=deep)
            if not isinstance(key, Hashable):
                raise ConstructorError(
                    "while constructing a mapping",
                    node.start_mark,
                    "found an unhashable mapping key",
                    key_node.start_mark,
                )
            if key in mapping:
                raise ConstructorError(
                    "while constructing a mapping",
                    node.start_mark,
                    f"found duplicate key {key!r}",
                    key_node.start_mark,
                )
            mapping[key] = self.construct_object(value_node, deep=deep)
        return mapping


def load_yaml(path: Path) -> YamlValue:
    """Load one UTF-8 YAML document with duplicate-key rejection."""
    with path.open(encoding="utf-8") as stream:
        return cast(YamlValue, yaml.load(stream, Loader=StrictSafeLoader))


def load_yaml_text(text: str) -> YamlValue:
    """Load YAML text with the same duplicate-key rejection as file input."""
    return cast(YamlValue, yaml.load(text, Loader=StrictSafeLoader))
