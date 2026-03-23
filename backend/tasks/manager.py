"""Simple threading-based async task manager."""
from __future__ import annotations

import threading
import traceback
from dataclasses import dataclass, field
from typing import Any, Callable
from uuid import uuid4


@dataclass
class Job:
    job_id: str
    status: str = "pending"   # pending | running | done | error
    progress_pct: int = 0
    message: str = ""
    result: dict | None = None
    error: str | None = None


class TaskManager:
    def __init__(self) -> None:
        self._jobs: dict[str, Job] = {}
        self._lock = threading.Lock()

    def submit(self, fn: Callable, *args, job_id: str | None = None, **kwargs) -> str:
        job_id = job_id or uuid4().hex
        job = Job(job_id=job_id, status="pending")
        with self._lock:
            self._jobs[job_id] = job

        def progress_cb(pct: int, msg: str = "") -> None:
            with self._lock:
                j = self._jobs.get(job_id)
                if j:
                    j.progress_pct = pct
                    j.message = msg
                    j.status = "running"

        def run() -> None:
            with self._lock:
                self._jobs[job_id].status = "running"
            try:
                result = fn(progress_cb, *args, **kwargs)
                with self._lock:
                    j = self._jobs[job_id]
                    j.status = "done"
                    j.progress_pct = 100
                    j.result = result
            except Exception:
                err = traceback.format_exc()
                with self._lock:
                    j = self._jobs[job_id]
                    j.status = "error"
                    j.error = err
                    j.message = str(traceback.format_exc().strip().splitlines()[-1])

        thread = threading.Thread(target=run, daemon=True)
        thread.start()
        return job_id

    def get(self, job_id: str) -> Job | None:
        with self._lock:
            return self._jobs.get(job_id)

    def to_dict(self, job: Job) -> dict:
        return {
            "job_id": job.job_id,
            "status": job.status,
            "progress_pct": job.progress_pct,
            "message": job.message,
            "result": job.result,
            "error": job.error,
        }
