import multiprocessing
import time

import pytest
import requests

from los_analyzer.backend.app import app as flask_app

TEST_FLASK_HOST = "127.0.0.1"
TEST_FLASK_PORT = 8091

def wait_for_app_startup(app_base_url, timeout_ms=1000):
    recent_exception = None
    start_time = time.time_ns()
    while (time.time_ns() - start_time) * 10**-6 < timeout_ms:
        try:
            response = requests.get(app_base_url + "/api/healthcheck")
            assert response.status_code == 200
            return
        except AssertionError as e:
            recent_exception = e
        except requests.RequestException as e:
            recent_exception = e

        time.sleep(0.05)

    raise RuntimeError(f"Timeout after {timeout_ms} ms waiting for health check") from recent_exception

def run_app():
    flask_app.run(host=TEST_FLASK_HOST, port=TEST_FLASK_PORT)

@pytest.fixture()
def temp_app_base_url(request):
    app_process = multiprocessing.Process(target=run_app)
    app_process.start()
    app_base_url = f"http://{TEST_FLASK_HOST}:{TEST_FLASK_PORT}"
    wait_for_app_startup(app_base_url)
    yield app_base_url
    app_process.terminate()


def test_hello(temp_app_base_url):
    response = requests.get(temp_app_base_url + "/api/healthcheck")
    assert response.status_code == 200