"""Miscellaneous utility functions for BitBazaar."""


__all__ = ["copy_sig"]

import typing as tp

_T = tp.TypeVar("_T")


def copy_sig(f: _T) -> tp.Callable[[tp.Any], _T]:
    """Keep e.g. a class's __init__ signature when subclassing.

    From: https://github.com/python/typing/issues/769#issuecomment-903760354
    """
    return lambda x: x
