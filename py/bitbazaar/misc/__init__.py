"""Miscellaneous utility functions for BitBazaar."""


__all__ = ["copy_sig"]

import os
import socket
import typing as tp

_T = tp.TypeVar("_T")


def copy_sig(f: _T) -> tp.Callable[[tp.Any], _T]:
    """Keep e.g. a class's __init__ signature when subclassing.

    From: https://github.com/python/typing/issues/769#issuecomment-903760354
    """
    return lambda x: x


_CI_ENV_VARS = ["GITHUB_ACTIONS", "TRAVIS", "CIRCLECI", "GITLAB_CI"]


def in_ci() -> bool:
    """Returns true if it looks like the program is running from a CI service, e.g. Github Actions."""
    return any([var in os.environ for var in _CI_ENV_VARS])


def is_tcp_port_listening(
    host: str, port: int
) -> bool:  # pragma: no cover (is covered but not in CI)
    """Check if something is listening on a certain tcp port or not."""
    try:
        # Create a TCP socket
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.settimeout(1)  # Set timeout to 1 second

        # Attempt to establish a connection to the port
        s.connect((host, port))

        # If connection is successful, something is listening on the port
        s.close()
        return True
    except OSError:
        return False
