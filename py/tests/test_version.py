import bitbazaar


def test_version():
    """Just a default example version test."""
    from importlib.metadata import version

    assert bitbazaar.__version__ == version("bitbazaar")
