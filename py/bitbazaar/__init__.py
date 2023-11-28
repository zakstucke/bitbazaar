"""BitBazaar."""

from importlib.metadata import version

__version__ = version("bitbazaar")

from . import utils


def hello() -> str:
    """Returns Hello, World!

    Returns:
        str: Hello, World!
    """
    return "Hello, World!"


__all__ = ["hello", "utils"]
