from io import BytesIO
from typing import Optional, Callable, Any, Hashable, Dict, Type, NamedTuple

import joblib


class Key(NamedTuple):
    key_type: Type
    key_val: Hashable

class CacheProvider:
    def store(self, key: Key, value: bytes):
        raise NotImplemented()

    def fetch(self, key: Key) -> bytes:
        raise NotImplemented()

    def contains(self, key: Key) -> bool:
        raise NotImplemented()

class SerializingCache:
    def __init__(self, provider: CacheProvider):
        self._provider = provider
        self._serializers: Dict[Type, Callable[[Any], bytes]] = {}
        self._deserializers: Dict[Type, Callable[[bytes], Any]] = {}

    def register_serializer(self, t: Type, serializer: Callable[[Any], bytes]):
        self._serializers[t] = serializer

    def register_deserializer(self, t: Type, deserializer: Callable[[bytes], Any]):
        self._deserializers[t] = deserializer

    def _serialize[T](self, obj: T) -> bytes:
        return self._serializers[type(obj)](obj)

    def _deserialize[T](self, t: Type, value: bytes) -> T:
        return self._deserializers[t](value)

    def store[T](self, key: Key, obj: T):
        self._provider.store(key, self._serialize(obj))

    def fetch[T](self, key: Key) -> T:

        return self._deserialize(key.key_type, self._provider.fetch(key))

    def contains(self, key: Key) -> bool:
        return self._provider.contains(key)

    def cache_return_value(self, return_type: Optional[Type] = None, key: Optional[Callable[..., Hashable]] = None):
        def decorator(original_function):
            k = key
            if k is None:
                k = lambda *args, **kwargs: tuple(sorted({**kwargs, **dict(zip(original_function.__code__.co_varnames, args))}.items()))

            ret_type = return_type
            if not ret_type:
                annotated_type = original_function.__annotations__.get('return')
                if not annotated_type:
                    raise ValueError("Must specify return_type or be used on function with return type annotation")
                ret_type = annotated_type

            def wrapper(*args, **kwargs):
                cache_key = Key(key_type=ret_type, key_val=k(*args, **kwargs))
                if self.contains(cache_key):
                    return self.fetch(cache_key)

                result = original_function(*args, **kwargs)
                self.store(cache_key, result)

                return result

            return wrapper

        return decorator


class JobLibSerializingCache(SerializingCache):
    def __init__(self, provider: CacheProvider):
        super().__init__(provider)

    def register_serializer(self, t: Type, serializer: Callable[[Any], bytes]):
        raise NotImplementedError("register_serializer() not allowed JobLibSerializingCache")

    def register_deserializer(self, t: Type, deserializer: Callable[[bytes], Any]):
        raise NotImplementedError("register_deserializer() not allowed JobLibSerializingCache")

    def _serialize[T](self, obj: T) -> bytes:
        bytes_container = BytesIO()
        joblib.dump(obj, bytes_container)
        bytes_container.seek(0)
        return bytes_container.read()

    def _deserialize[T](self, t: Type, value: bytes) -> T:
        bytes_container = BytesIO()
        bytes_container.write(value)
        bytes_container.seek(0)
        return joblib.load(bytes_container)


class DictProvider(CacheProvider):
    def __init__(self):
        self.dict: Dict[Key, bytes] = {}

    def store(self, key: Key, value: bytes):
        self.dict[key] = value

    def fetch(self, key: Key) -> bytes:
        return self.dict[key]

    def contains(self, key: Key) -> bool:
        return key in self.dict