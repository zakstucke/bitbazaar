import typing as tp

# Set to True during tests in conftest.py
IS_TEST = False

BUF: list[tp.Any] = []


def mark_testing():
    """Run by conftest.py to mark that we're in a test."""
    global IS_TEST
    IS_TEST = True
