"""Flask application entry point."""
from __future__ import annotations

from flask import Flask
from flask_cors import CORS

app = Flask(__name__)
CORS(app)

@app.get("/api/healthcheck")
def hello():
    return "Healthy"


if __name__ == "__main__":
    app.run()
