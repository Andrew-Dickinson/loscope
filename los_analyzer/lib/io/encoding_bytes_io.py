from io import BytesIO


class EncodingBytesIO(BytesIO):
    def write(self, string: str):
        super().write(string.encode())