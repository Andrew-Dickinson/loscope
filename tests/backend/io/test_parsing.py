"""Tests for los_analyzer.backend.io.parsing"""
import pytest

from los_analyzer.backend.io.parsing import parse_coords, parse_obstruction_types


# ---------------------------------------------------------------------------
# parse_coords
# ---------------------------------------------------------------------------

class TestParseCoords:
    def test_valid_float_list_returns_tuple(self):
        assert parse_coords([1.0, 2.0, 3.0]) == (1.0, 2.0, 3.0)

    def test_integer_elements_are_converted_to_float(self):
        result = parse_coords([1, 2, 3])
        assert all(isinstance(v, float) for v in result)
        assert result == (1.0, 2.0, 3.0)

    def test_string_numbers_converted_to_float(self):
        result = parse_coords(["1.5", "2.5", "3.5"])
        assert result == (1.5, 2.5, 3.5)

    def test_raises_type_error_for_tuple(self):
        with pytest.raises(TypeError, match="array"):
            parse_coords((1.0, 2.0, 3.0))

    def test_raises_type_error_for_dict(self):
        with pytest.raises(TypeError):
            parse_coords({"x": 1.0, "y": 2.0, "z": 3.0})

    def test_raises_type_error_for_none(self):
        with pytest.raises(TypeError):
            parse_coords(None)

    def test_raises_value_error_for_two_elements(self):
        with pytest.raises(ValueError, match="3 elements"):
            parse_coords([1.0, 2.0])

    def test_raises_value_error_for_four_elements(self):
        with pytest.raises(ValueError):
            parse_coords([1.0, 2.0, 3.0, 4.0])

    def test_raises_value_error_for_empty_list(self):
        with pytest.raises(ValueError):
            parse_coords([])

    def test_negative_and_fractional_values_accepted(self):
        result = parse_coords([-73.8, 40.65, 100.0])
        assert result == (-73.8, 40.65, 100.0)


# ---------------------------------------------------------------------------
# parse_obstruction_types
# ---------------------------------------------------------------------------

class TestParseObstructionTypes:
    def test_list_of_strings_returns_list(self):
        result = parse_obstruction_types(["building", "construction"])
        assert result == ["building", "construction"]

    def test_wildcard_string_returns_wildcard(self):
        assert parse_obstruction_types("*") == "*"

    def test_empty_list_returns_empty_list(self):
        assert parse_obstruction_types([]) == []

    def test_non_string_items_converted_to_strings(self):
        assert parse_obstruction_types([1, 2, 3]) == ["1", "2", "3"]

    def test_raises_type_error_for_non_wildcard_string(self):
        with pytest.raises(TypeError, match="Obstruction types"):
            parse_obstruction_types("not_wildcard")

    def test_raises_type_error_for_none(self):
        with pytest.raises(TypeError):
            parse_obstruction_types(None)

    def test_raises_type_error_for_integer(self):
        with pytest.raises(TypeError):
            parse_obstruction_types(42)

    def test_raises_type_error_for_dict(self):
        with pytest.raises(TypeError):
            parse_obstruction_types({"key": "val"})
