from . import utils

def hello() -> str:
    """Returns Hello, World!

    Returns:
        str: Hello, World!
    """
    ...

__version__: str

__all__ = [
    "__version__",
    "utils",
    "hello",
]
