#!/usr/bin/env python3
"""Run the full geocode-csv test suite, including the #[ignore]d integration tests.

The ignored tests exercise real external services: the Smarty API, a BigTable
cache, a local Redis cache, and the bundled libpostal model. We require the
necessary credentials in the environment and make sure a local Redis is
listening before handing off to `cargo test`. Nothing is skipped.
"""

import os
import shutil
import socket
import subprocess
import sys
import time
from dataclasses import dataclass

REDIS_HOST = "localhost"
REDIS_PORT = 6379

REQUIRED_ENVIRONMENT_VARIABLES = (
    "SMARTY_AUTH_ID",
    "SMARTY_AUTH_TOKEN",
    "BIGTABLE_CACHE_URL",
)


@dataclass
class CargoTest:
    """A single `cargo test` invocation: arguments for cargo itself, plus
    arguments passed through to the test harness after `--`."""

    description: str
    cargo_arguments: list
    test_harness_arguments: list

    def run(self):
        command = [
            "cargo",
            "test",
            *self.cargo_arguments,
            "--",
            *self.test_harness_arguments,
        ]
        print(f"\n=== {self.description} ===")
        print(f"$ {' '.join(command)}", flush=True)
        subprocess.run(command, check=True)


class RedisServer:
    """The local Redis that the cache integration tests connect to."""

    def is_listening(self):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
            probe.settimeout(0.5)
            return probe.connect_ex((REDIS_HOST, REDIS_PORT)) == 0

    def ensure_listening(self):
        if self.is_listening():
            print(f"Redis already listening on {REDIS_HOST}:{REDIS_PORT}.")
            return
        if shutil.which("redis-server") is None:
            sys.exit(
                "redis-server is required for the cache tests but is not installed "
                "(try `brew install redis`)."
            )
        print("Starting redis-server in the background...")
        subprocess.run(
            ["redis-server", "--daemonize", "yes", "--port", str(REDIS_PORT)],
            check=True,
        )
        for _ in range(50):
            if self.is_listening():
                print("Redis is up.")
                return
            time.sleep(0.1)
        sys.exit("redis-server was started but never began listening.")


def require_environment_variables():
    missing = [
        name
        for name in REQUIRED_ENVIRONMENT_VARIABLES
        if not os.environ.get(name)
    ]
    if missing:
        sys.exit(
            "the following environment variables must be set: "
            + ", ".join(missing)
        )


def main():
    require_environment_variables()
    os.environ.setdefault("RUST_LOG", "info")

    RedisServer().ensure_listening()

    print("\n=== Building workspace ===")
    subprocess.run(["cargo", "build", "--workspace"], check=True)

    suites = [
        CargoTest(
            description="Unit tests (binary + every crate library), including ignored",
            cargo_arguments=["--workspace", "--lib", "--bins"],
            test_harness_arguments=["--include-ignored"],
        ),
        CargoTest(
            description="Doc tests",
            cargo_arguments=["--workspace", "--doc"],
            test_harness_arguments=[],
        ),
        CargoTest(
            description="Integration: about",
            cargo_arguments=["--test", "about"],
            test_harness_arguments=["--include-ignored"],
        ),
        CargoTest(
            description="Integration: duplicate_columns",
            cargo_arguments=["--test", "duplicate_columns"],
            test_harness_arguments=["--include-ignored"],
        ),
        CargoTest(
            description="Integration: libpostal",
            cargo_arguments=["--test", "libpostal"],
            test_harness_arguments=["--include-ignored"],
        ),
        CargoTest(
            description="Integration: specs (Smarty + Redis)",
            cargo_arguments=["--test", "specs"],
            test_harness_arguments=["--include-ignored"],
        ),
        CargoTest(
            description="Integration: server",
            cargo_arguments=["--test", "server"],
            test_harness_arguments=["--include-ignored"],
        ),
        CargoTest(
            # Serialized to one thread because every test in this file writes to
            # the same shared BigTable table.
            description="Integration: bigtable_refresh",
            cargo_arguments=["--test", "bigtable_refresh"],
            test_harness_arguments=["--include-ignored", "--test-threads=1"],
        ),
    ]
    for suite in suites:
        suite.run()

    print("\nAll tests passed.")


if __name__ == "__main__":
    main()
