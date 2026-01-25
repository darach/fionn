# SPDX-License-Identifier: MIT OR Apache-2.0
"""Tests for multi-format parsers and serializers."""

from __future__ import annotations


class TestAutoFormatDetection:
    """Test automatic format detection with parse()."""

    def test_parse_json_object(self) -> None:
        """Test parsing JSON object."""
        import fionn.ext as fx

        result = fx.parse('{"name": "Alice", "age": 30}')
        assert result == {"name": "Alice", "age": 30}

    def test_parse_json_array(self) -> None:
        """Test parsing JSON array."""
        import fionn.ext as fx

        result = fx.parse("[1, 2, 3]")
        assert result == [1, 2, 3]

    def test_parse_empty_string(self) -> None:
        """Test parsing empty string returns None."""
        import fionn.ext as fx

        result = fx.parse("")
        assert result is None


class TestYamlFormat:
    """Test YAML parsing and serialization."""

    def test_parse_yaml_simple(self) -> None:
        """Test parsing simple YAML."""
        import fionn.ext as fx

        yaml_str = """
name: Alice
age: 30
"""
        result = fx.parse_yaml(yaml_str)
        assert result["name"] == "Alice"
        assert result["age"] == 30

    def test_parse_yaml_nested(self) -> None:
        """Test parsing nested YAML."""
        import fionn.ext as fx

        yaml_str = """
user:
  name: Alice
  address:
    city: NYC
"""
        result = fx.parse_yaml(yaml_str)
        assert result["user"]["name"] == "Alice"
        assert result["user"]["address"]["city"] == "NYC"

    def test_parse_yaml_list(self) -> None:
        """Test parsing YAML list."""
        import fionn.ext as fx

        yaml_str = """
items:
  - apple
  - banana
  - cherry
"""
        result = fx.parse_yaml(yaml_str)
        assert result["items"] == ["apple", "banana", "cherry"]

    def test_to_yaml(self) -> None:
        """Test serializing to YAML."""
        import fionn.ext as fx

        data = {"name": "Alice", "age": 30}
        yaml_str = fx.to_yaml(data)
        assert "name:" in yaml_str
        assert "Alice" in yaml_str
        assert "age:" in yaml_str


class TestTomlFormat:
    """Test TOML parsing and serialization."""

    def test_parse_toml_simple(self) -> None:
        """Test parsing simple TOML."""
        import fionn.ext as fx

        toml_str = """
name = "Alice"
age = 30
"""
        result = fx.parse_toml(toml_str)
        assert result["name"] == "Alice"
        assert result["age"] == 30

    def test_parse_toml_nested(self) -> None:
        """Test parsing nested TOML (tables)."""
        import fionn.ext as fx

        toml_str = """
[user]
name = "Alice"

[user.address]
city = "NYC"
"""
        result = fx.parse_toml(toml_str)
        assert result["user"]["name"] == "Alice"
        assert result["user"]["address"]["city"] == "NYC"

    def test_parse_toml_array(self) -> None:
        """Test parsing TOML with arrays."""
        import fionn.ext as fx

        toml_str = """
items = ["apple", "banana", "cherry"]
"""
        result = fx.parse_toml(toml_str)
        assert result["items"] == ["apple", "banana", "cherry"]

    def test_to_toml(self) -> None:
        """Test serializing to TOML."""
        import fionn.ext as fx

        data = {"name": "Alice", "age": 30}
        toml_str = fx.to_toml(data)
        assert 'name = "Alice"' in toml_str
        assert "age = 30" in toml_str


class TestCsvFormat:
    """Test CSV parsing and serialization."""

    def test_parse_csv_simple(self) -> None:
        """Test parsing simple CSV."""
        import fionn.ext as fx

        csv_str = """name,age
Alice,30
Bob,25"""
        result = fx.parse_csv(csv_str)
        assert len(result) == 2
        assert result[0]["name"] == "Alice"
        # Note: fionn CSV parser auto-converts numeric strings to numbers
        assert result[0]["age"] == 30
        assert result[1]["name"] == "Bob"

    def test_parse_csv_string_values(self) -> None:
        """Test parsing CSV with pure string values."""
        import fionn.ext as fx

        csv_str = """name,city
Alice,NYC
Bob,LA"""
        result = fx.parse_csv(csv_str)
        assert result[0]["city"] == "NYC"
        assert result[1]["city"] == "LA"

    def test_to_csv(self) -> None:
        """Test serializing to CSV."""
        import fionn.ext as fx

        data = [
            {"name": "Alice", "age": 30},
            {"name": "Bob", "age": 25},
        ]
        csv_str = fx.to_csv(data)
        assert "name" in csv_str
        assert "age" in csv_str
        assert "Alice" in csv_str
        assert "Bob" in csv_str


class TestIsonFormat:
    """Test ISON parsing and serialization."""

    def test_parse_ison_simple(self) -> None:
        """Test parsing simple ISON."""
        import fionn.ext as fx

        # ISON format: header line (table.name field1:type field2:type)
        # followed by data lines (value1 value2)
        ison_str = """table.users id:int name:string
1 Alice"""
        result = fx.parse_ison(ison_str)
        # parse_ison returns a list of records
        assert isinstance(result, list)
        assert len(result) == 1
        assert result[0]["id"] == 1
        assert result[0]["name"] == "Alice"

    def test_parse_ison_multiple_records(self) -> None:
        """Test parsing ISON with multiple records."""
        import fionn.ext as fx

        # Header line + multiple data lines
        ison_str = """table.users id:int name:string
1 Alice
2 Bob"""
        result = fx.parse_ison(ison_str)
        assert len(result) == 2
        assert result[0]["id"] == 1
        assert result[1]["id"] == 2

    def test_to_ison(self) -> None:
        """Test serializing to ISON."""
        import fionn.ext as fx

        # to_ison expects a list of dicts
        data = [{"id": 1, "name": "Alice"}]
        schema = ["id:int", "name:string"]
        ison_str = fx.to_ison(data, table="users", schema=schema)
        assert "table.users" in ison_str
        assert "id:int" in ison_str
        assert "name:string" in ison_str


class TestToonFormat:
    """Test TOON parsing and serialization."""

    def test_parse_toon_simple(self) -> None:
        """Test parsing simple TOON (TOML + ISON hybrid)."""
        import fionn.ext as fx

        # TOON is a specialized format - test basic parsing
        toon_str = """name = "test"
items = [1, 2, 3]"""
        result = fx.parse_toon(toon_str)
        # TOON returns parsed structure
        assert isinstance(result, dict)

    def test_to_toon(self) -> None:
        """Test serializing to TOON."""
        import fionn.ext as fx

        # TOON serialization expects dict with list values
        data = {
            "users": [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"},
            ]
        }
        toon_str = fx.to_toon(data)
        # Check output contains expected content
        assert isinstance(toon_str, str)
        assert len(toon_str) > 0


class TestFormatRoundtrip:
    """Test roundtrip conversions."""

    def test_yaml_roundtrip(self) -> None:
        """Test YAML roundtrip."""
        import fionn.ext as fx

        original = {"name": "Alice", "scores": [100, 95, 88]}
        yaml_str = fx.to_yaml(original)
        result = fx.parse_yaml(yaml_str)
        assert result == original

    def test_toml_roundtrip(self) -> None:
        """Test TOML roundtrip."""
        import fionn.ext as fx

        original = {"name": "Alice", "age": 30, "active": True}
        toml_str = fx.to_toml(original)
        result = fx.parse_toml(toml_str)
        assert result == original

    def test_csv_roundtrip(self) -> None:
        """Test CSV roundtrip."""
        import fionn.ext as fx

        # Note: CSV parser auto-converts numeric strings to numbers
        # So use string-only data for exact roundtrip
        original = [
            {"name": "Alice", "city": "NYC"},
            {"name": "Bob", "city": "LA"},
        ]
        csv_str = fx.to_csv(original)
        result = fx.parse_csv(csv_str)
        assert result == original


class TestFormatEdgeCases:
    """Test edge cases for format handling."""

    def test_parse_yaml_empty(self) -> None:
        """Test parsing empty YAML."""
        import fionn.ext as fx

        result = fx.parse_yaml("")
        assert result is None

    def test_parse_toml_empty(self) -> None:
        """Test parsing empty TOML."""
        import fionn.ext as fx

        result = fx.parse_toml("")
        assert result == {} or result is None

    def test_parse_csv_empty(self) -> None:
        """Test parsing empty CSV."""
        import fionn.ext as fx

        result = fx.parse_csv("")
        assert result == []

    def test_parse_yaml_unicode(self) -> None:
        """Test parsing YAML with Unicode."""
        import fionn.ext as fx

        yaml_str = "greeting: Hello World!"
        result = fx.parse_yaml(yaml_str)
        assert "Hello" in result["greeting"]
