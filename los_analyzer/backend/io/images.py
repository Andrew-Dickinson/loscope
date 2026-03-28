from io import BytesIO

from flask import send_file

def serve_pil_image(pil_img, encoding="PNG", quality=100):
    img_io = BytesIO()
    pil_img.save(img_io, encoding, quality=quality)
    img_io.seek(0)
    return send_file(img_io, mimetype=f'image/{encoding.lower()}')