# Set to True during tests in conftest.py
IS_TEST = False


def mark_testing():
    """Run by conftest.py to mark that we're in a test."""
    global IS_TEST
    IS_TEST = True
