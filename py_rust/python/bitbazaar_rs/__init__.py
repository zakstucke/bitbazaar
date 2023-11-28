# ruff: noqa

# https://www.maturin.rs/project_layout

# Import the rust modules and top level fns:
from ._rs import *

# Setup docs and __all__, note this might need modifying if we start adding pure python in here too:
__doc__ = _rs.__doc__
if hasattr(_rs, "__all__"):
    __all__ = _rs.__all__
