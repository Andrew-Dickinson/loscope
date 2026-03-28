from typing import Dict

import numpy as np
from shapely import Geometry, Polygon

from los_analyzer.backend.cache.private_cache import CacheProvider, Key, SerializingCache, JobLibSerializingCache, \
    DictProvider
from los_analyzer.lib.building.heightmap import RooftopHeightMap

def test_serializing_cache():
    serializing_cache = SerializingCache(DictProvider())
    serializing_cache.register_serializer(str, lambda s: s.encode())
    serializing_cache.register_deserializer(str, lambda b: b.decode())

    test_func_call_count = 0

    @serializing_cache.cache_return_value()
    def test_func(a: str, b: str) -> str:
        nonlocal test_func_call_count
        test_func_call_count += 1
        return a + b

    assert test_func_call_count == 0
    assert test_func("a", "b") == 'ab'
    assert test_func_call_count == 1
    assert test_func("a", "b") == 'ab'
    assert test_func_call_count == 1
    assert test_func("a", b="b") == 'ab'
    assert test_func_call_count == 1
    assert test_func("a", b="c") == 'ac'
    assert test_func_call_count == 2
    assert test_func("a", "c") == 'ac'
    assert test_func_call_count == 2

def test_joblib_cache():
    serializing_cache = JobLibSerializingCache(DictProvider())
    test_func_call_count = 0

    static_heightmap: RooftopHeightMap = {
        "bin_id": "foo_bar",
        "x_sw": 23,
        "y_sw": 58,
        "heightmap": np.array([1,2,3], dtype=np.uint16),
        "mask": np.array([0,0,1], dtype=np.bool),
        "poly_nys": Polygon(((0., 0.), (0., 1.), (1., 1.), (1., 0.), (0., 0.)))
    }

    @serializing_cache.cache_return_value()
    def test_func(foo: str) -> RooftopHeightMap:
        nonlocal test_func_call_count
        test_func_call_count += 1
        return static_heightmap

    assert test_func_call_count == 0
    assert test_func("a") == static_heightmap
    assert test_func_call_count == 1
    call2_val = test_func("a")
    np.testing.assert_array_equal(call2_val["heightmap"], static_heightmap["heightmap"])
    assert test_func_call_count == 1
    call3_val = test_func(foo="a")
    np.testing.assert_array_equal(call3_val["heightmap"], static_heightmap["heightmap"])
    assert test_func_call_count == 1
    call4_val = test_func("b")
    np.testing.assert_array_equal(call4_val["heightmap"], static_heightmap["heightmap"])
    assert test_func_call_count == 2
    call5_val = test_func(foo="b")
    np.testing.assert_array_equal(call5_val["heightmap"], static_heightmap["heightmap"])
    assert test_func_call_count == 2
